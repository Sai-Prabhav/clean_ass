//! Line-oriented lexer. Turns one raw line into fields, or leaves it raw.
//!
//! Everything works on bytes: non-UTF-8 comments and string data are preserved.

use crate::config::{Config, HexCase};

/// One logical source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Blank,
    /// A line whose only content is a comment.
    Comment(Vec<u8>),
    /// A `%...` line, already placed at column 0.
    Preproc(Vec<u8>),
    LabelOnly {
        label: Vec<u8>,
        comment: Option<Vec<u8>>,
    },
    Code(CodeLine),
    /// Input the lexer will not touch beyond trimming trailing whitespace.
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLine {
    /// `Some("foo:")` for colon labels, `Some("foo")` for `equ` labels.
    pub label: Option<Vec<u8>>,
    /// Lowercased, with any instruction prefixes folded in.
    pub mnemonic: Vec<u8>,
    pub operands: Vec<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
    /// `times` and `equ` keep the rest of the line as one opaque operand.
    pub opaque: bool,
    /// Assembler directives like `section`, `global`, `default` start at column 0.
    pub flush_left: bool,
}

/// Lex a single line (without its line ending).
pub fn lex_line(input: &[u8], config: &Config) -> Line {
    let line = trim_end(input);
    if line.is_empty() {
        return Line::Blank;
    }

    let (code, comment) = match split_comment(line) {
        Ok(parts) => parts,
        Err(()) => return Line::Raw(line.to_vec()),
    };
    let comment = comment.map(|part| normalize_comment(trim_end(part)));
    let code = trim_end(code);
    let start = trim_start(code);

    if start.is_empty() {
        return match comment {
            Some(comment) => Line::Comment(comment),
            None => Line::Blank,
        };
    }

    if start.starts_with(b"%") && !start.starts_with(b"%%") {
        let mut text = start.to_vec();
        lowercase_first_token(&mut text);
        if let Some(comment) = comment {
            text.push(b' ');
            text.extend_from_slice(&comment);
        }
        return Line::Preproc(text);
    }
    if start.starts_with(b"%%") {
        if let Some((label, rest)) = split_label(start) {
            if rest.is_empty() {
                return Line::LabelOnly { label, comment };
            }
            if let Ok(line) =
                parse_code(rest, Some(label.clone()), comment.clone(), config)
            {
                return line;
            }
            let mut raw = start.to_vec();
            if let Some(c) = comment {
                raw.push(b' ');
                raw.extend_from_slice(&c);
            }
            return Line::Raw(raw);
        }
        let mut text = start.to_vec();
        lowercase_first_token(&mut text);
        if let Some(c) = comment {
            text.push(b' ');
            text.extend_from_slice(&c);
        }
        return Line::Preproc(text);
    }

    let parsed = if let Some((label, rest)) = split_label(start) {
        if rest.is_empty() {
            Ok(Line::LabelOnly { label, comment })
        } else {
            parse_code(rest, Some(label), comment, config)
        }
    } else if let Some((label, value)) = split_equ(start) {
        let operands = if value.is_empty() {
            Vec::new()
        } else {
            vec![normalize_opaque(value, config)]
        };
        Ok(Line::Code(CodeLine {
            label: Some(label),
            mnemonic: b"equ".to_vec(),
            operands,
            comment,
            opaque: true,
            flush_left: false,
        }))
    } else {
        parse_code(start, None, comment, config)
    };

    parsed.unwrap_or_else(|()| Line::Raw(line.to_vec()))
}

fn parse_code(
    input: &[u8],
    label: Option<Vec<u8>>,
    comment: Option<Vec<u8>>,
    config: &Config,
) -> Result<Line, ()> {
    let mut rest = input;
    let mut prefixes: Vec<u8> = Vec::new();

    loop {
        let (token, after) = next_token(rest);
        if token.is_empty() {
            return Err(());
        }
        if is_prefix(token) && !after.is_empty() {
            if !prefixes.is_empty() {
                prefixes.push(b' ');
            }
            for &byte in token {
                prefixes.push(byte.to_ascii_lowercase());
            }
            rest = after;
        } else {
            break;
        }
    }

    let (token, after) = next_token(rest);
    if token.is_empty() || !valid_mnemonic(token) || is_prefix(token) {
        return Err(());
    }

    let opaque = token.eq_ignore_ascii_case(b"times")
        || token.eq_ignore_ascii_case(b"equ");

    let mut mnemonic = prefixes.clone();
    if !mnemonic.is_empty() {
        mnemonic.push(b' ');
    }
    for &byte in token {
        mnemonic.push(byte.to_ascii_lowercase());
    }

    let operands = if after.is_empty() {
        Vec::new()
    } else if opaque {
        vec![normalize_opaque(after, config)]
    } else {
        let parts = split_operands(after)?;
        parts
            .into_iter()
            .map(|part| normalize_operand(part, config))
            .collect()
    };

    let flush_left = prefixes.is_empty()
        && matches!(
            mnemonic.as_slice(),
            b"section"
                | b"segment"
                | b"global"
                | b"extern"
                | b"common"
                | b"static"
                | b"required"
                | b"export"
                | b"import"
                | b"bits"
                | b"use16"
                | b"use32"
                | b"use64"
                | b"default"
                | b"cpu"
                | b"float"
                | b"org"
                | b"absolute"
                | b"align"
                | b"pad"
                | b"pushsection"
                | b"popsection"
                | b"init"
                | b"fini"
        );

    Ok(Line::Code(CodeLine {
        label,
        mnemonic,
        operands,
        comment,
        opaque,
        flush_left,
    }))
}

fn split_comment(input: &[u8]) -> Result<(&[u8], Option<&[u8]>), ()> {
    let mut quote: Option<u8> = None;
    let mut escaped = false;

    for (index, &byte) in input.iter().enumerate() {
        match quote {
            None => match byte {
                b'\'' | b'"' | b'`' => quote = Some(byte),
                b';' => return Ok((&input[..index], Some(&input[index..]))),
                _ => {}
            },
            Some(q) => {
                if q == b'`' {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == q {
                        quote = None;
                    }
                } else if byte == q {
                    quote = None;
                }
            }
        }
    }

    if quote.is_some() {
        Err(())
    } else {
        Ok((input, None))
    }
}

fn split_operands(input: &[u8]) -> Result<Vec<&[u8]>, ()> {
    let mut operands = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut escaped = false;

    for (index, &byte) in input.iter().enumerate() {
        if let Some(q) = quote {
            if q == b'`' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == q {
                    quote = None;
                }
            } else if byte == q {
                quote = None;
            }
            continue;
        }

        match byte {
            b'\'' | b'"' | b'`' => quote = Some(byte),
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return Err(());
                }
            }
            b',' if depth == 0 => {
                let operand = trim(&input[start..index]);
                if operand.is_empty() {
                    return Err(());
                }
                operands.push(operand);
                start = index + 1;
            }
            _ => {}
        }
    }

    if quote.is_some() || depth != 0 {
        return Err(());
    }

    let operand = trim(&input[start..]);
    if operand.is_empty() {
        return Err(());
    }
    operands.push(operand);
    Ok(operands)
}

fn split_label(input: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let start = trim_start(input);
    let mut end = 0;
    while end < start.len() && !is_space(start[end]) && start[end] != b':' {
        end += 1;
    }
    if end == 0 {
        return None;
    }

    let token = &start[..end];
    if !valid_label(token) {
        return None;
    }

    let mut colon = end;
    while colon < start.len() && is_space(start[colon]) {
        colon += 1;
    }
    if colon >= start.len() || start[colon] != b':' {
        return None;
    }

    let mut label = token.to_vec();
    label.push(b':');
    Some((label, trim_start(&start[colon + 1..])))
}

fn split_equ(input: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let (token, rest) = next_token(input);
    if token.is_empty() || !valid_label(token) {
        return None;
    }
    let (keyword, value) = next_token(rest);
    if !keyword.eq_ignore_ascii_case(b"equ") {
        return None;
    }
    Some((token.to_vec(), value))
}

fn next_token(input: &[u8]) -> (&[u8], &[u8]) {
    let start = trim_start(input);
    let end = start
        .iter()
        .position(|&byte| is_space(byte))
        .unwrap_or(start.len());
    (&start[..end], trim_start(&start[end..]))
}

fn valid_label(token: &[u8]) -> bool {
    let Some(&first) = token.first() else {
        return false;
    };
    if first.is_ascii_digit() {
        return false;
    }
    token.iter().all(|&byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'_' | b'.' | b'$' | b'?' | b'@' | b'#' | b'~' | b'%'
            )
    })
}

fn valid_mnemonic(token: &[u8]) -> bool {
    !token.is_empty()
        && token.iter().all(|&byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'.' | b'?' | b'@' | b'$' | b'#' | b'~' | b'%'
                )
        })
}

const PREFIXES: &[&[u8]] = &[
    b"lock",
    b"rep",
    b"repe",
    b"repz",
    b"repne",
    b"repnz",
    b"xacquire",
    b"xrelease",
    b"bnd",
    b"notrack",
    b"a16",
    b"a32",
    b"a64",
    b"o16",
    b"o32",
    b"o64",
    b"cs",
    b"ds",
    b"es",
    b"fs",
    b"gs",
    b"ss",
];

fn is_prefix(token: &[u8]) -> bool {
    PREFIXES
        .iter()
        .any(|prefix| token.eq_ignore_ascii_case(prefix))
}

fn normalize_operand(input: &[u8], config: &Config) -> Vec<u8> {
    let tightened = tighten(input);
    let spaced = if config.bracket_spacing {
        space_inside_brackets(&tightened)
    } else {
        tightened
    };
    normalize_hex(&spaced, config.hex_case)
}

fn normalize_opaque(input: &[u8], config: &Config) -> Vec<u8> {
    normalize_hex(trim(input), config.hex_case)
}

fn tighten(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut pending_space = false;
    let mut depth = 0usize;

    for &byte in input {
        if let Some(q) = quote {
            out.push(byte);
            if q == b'`' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == q {
                    quote = None;
                }
            } else if byte == q {
                quote = None;
            }
            continue;
        }

        if is_space(byte) {
            pending_space = true;
            continue;
        }

        if matches!(byte, b'\'' | b'"' | b'`') {
            if pending_space {
                push_separator(&mut out, depth);
            }
            pending_space = false;
            out.push(byte);
            quote = Some(byte);
            continue;
        }

        match byte {
            b'[' => {
                if pending_space {
                    push_separator(&mut out, depth);
                }
                pending_space = false;
                out.push(byte);
                depth += 1;
                continue;
            }
            b']' => {
                pending_space = false;
                out.push(byte);
                depth = depth.saturating_sub(1);
                continue;
            }
            b'+' | b'-' | b'*' if depth > 0 => {
                pending_space = false;
                out.push(byte);
                continue;
            }
            _ => {}
        }

        if pending_space {
            push_separator(&mut out, depth);
        }
        pending_space = false;
        out.push(byte);
    }

    out
}

fn push_separator(out: &mut Vec<u8>, depth: usize) {
    let Some(&previous) = out.last() else {
        return;
    };
    if previous == b'[' || previous == b' ' {
        return;
    }
    if depth > 0 && matches!(previous, b'+' | b'-' | b'*') {
        return;
    }
    out.push(b' ');
}

fn space_inside_brackets(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 4);
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut index = 0;

    while index < input.len() {
        let byte = input[index];
        if let Some(q) = quote {
            out.push(byte);
            if q == b'`' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == q {
                    quote = None;
                }
            } else if byte == q {
                quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'\'' | b'"' | b'`' => {
                quote = Some(byte);
                out.push(byte);
            }
            b'[' => {
                out.push(byte);
                if !matches!(
                    input.get(index + 1),
                    Some(b' ') | Some(b']') | None
                ) {
                    out.push(b' ');
                }
            }
            b']' => {
                if out.last() != Some(&b' ') {
                    out.push(b' ');
                }
                out.push(b']');
            }
            _ => out.push(byte),
        }
        index += 1;
    }

    out
}

fn is_hex_boundary_before(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let byte = bytes[index - 1];
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b',' | b'['
                | b'('
                | b'{'
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b':'
                | b';'
        )
}

fn is_hex_boundary_after(bytes: &[u8], index: usize) -> bool {
    if index >= bytes.len() {
        return true;
    }
    let byte = bytes[index];
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b',' | b']'
                | b')'
                | b'}'
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b';'
                | b':'
        )
}

fn normalize_hex(input: &[u8], case: HexCase) -> Vec<u8> {
    if case == HexCase::Preserve {
        return input.to_vec();
    }
    let upper = case == HexCase::Upper;
    let mut out = Vec::with_capacity(input.len());
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut index = 0;

    while index < input.len() {
        let byte = input[index];
        if let Some(q) = quote {
            out.push(byte);
            if q == b'`' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == q {
                    quote = None;
                }
            } else if byte == q {
                quote = None;
            }
            index += 1;
            continue;
        }

        if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
            out.push(byte);
            index += 1;
            continue;
        }

        if byte == b'0'
            && index + 2 < input.len()
            && matches!(input[index + 1], b'x' | b'X')
            && input[index + 2].is_ascii_hexdigit()
            && is_hex_boundary_before(input, index)
        {
            let mut end = index + 2;
            while end < input.len()
                && (input[end].is_ascii_hexdigit() || input[end] == b'_')
            {
                end += 1;
            }
            if is_hex_boundary_after(input, end) {
                out.push(b'0');
                out.push(if upper { b'X' } else { b'x' });
                index += 2;
                while index < end {
                    out.push(if upper {
                        input[index].to_ascii_uppercase()
                    } else {
                        input[index].to_ascii_lowercase()
                    });
                    index += 1;
                }
                continue;
            }
        }

        out.push(byte);
        index += 1;
    }

    out
}

fn normalize_comment(comment: &[u8]) -> Vec<u8> {
    let mut out = comment.to_vec();
    if out.len() >= 2 && out[0] == b';' && out[1].is_ascii_alphanumeric() {
        out.insert(1, b' ');
    }
    out
}

fn lowercase_first_token(text: &mut [u8]) {
    let end = text
        .iter()
        .position(|&byte| is_space(byte))
        .unwrap_or(text.len());
    for byte in &mut text[..end] {
        *byte = byte.to_ascii_lowercase();
    }
}

fn is_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn trim_start(input: &[u8]) -> &[u8] {
    let start = input
        .iter()
        .position(|&byte| !is_space(byte))
        .unwrap_or(input.len());
    &input[start..]
}

fn trim_end(input: &[u8]) -> &[u8] {
    let end = input
        .iter()
        .rposition(|&byte| !is_space(byte))
        .map_or(0, |index| index + 1);
    &input[..end]
}

fn trim(input: &[u8]) -> &[u8] {
    trim_end(trim_start(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::default()
    }

    fn code(input: &str) -> CodeLine {
        match lex_line(input.as_bytes(), &config()) {
            Line::Code(line) => line,
            other => panic!("expected code, got {other:?}"),
        }
    }

    #[test]
    fn splits_a_trailing_comment() {
        let line = code("mov eax, 1 ; c");
        assert_eq!(line.mnemonic, b"mov");
        assert_eq!(line.operands, vec![b"eax".to_vec(), b"1".to_vec()]);
        assert_eq!(line.comment, Some(b"; c".to_vec()));
    }

    #[test]
    fn semicolons_inside_quotes_are_not_comments() {
        assert_eq!(code("db ';'").operands, vec![b"';'".to_vec()]);
        assert_eq!(
            code(r#"db "a;b", 0"#).operands,
            vec![b"\"a;b\"".to_vec(), b"0".to_vec()]
        );
        assert_eq!(code(r#"db `\n;`"#).comment, None);
        assert!(matches!(lex_line(b"db 'abc", &config()), Line::Raw(_)));
    }

    #[test]
    fn splits_commas_only_at_depth_zero() {
        assert_eq!(code("db 1, 2, 3").operands.len(), 3);
        assert_eq!(code("db \"a,b\", 0").operands.len(), 2);
        assert_eq!(code("mov rax, [rax+(rbx*4)]").operands.len(), 2);
        assert_eq!(code("db (a,b)").operands.len(), 1);
        assert_eq!(code("db {k1, k2}").operands.len(), 1);
    }

    #[test]
    fn recognizes_labels_without_recasing() {
        assert_eq!(
            lex_line(b"_start:", &config()),
            Line::LabelOnly {
                label: b"_start:".to_vec(),
                comment: None
            }
        );
        assert_eq!(
            lex_line(b".loop: ret", &config()),
            Line::Code(CodeLine {
                label: Some(b".loop:".to_vec()),
                mnemonic: b"ret".to_vec(),
                operands: vec![],
                comment: None,
                opaque: false,
                flush_left: false,
            })
        );
        assert_eq!(
            lex_line(b"%%done: nop", &config()),
            Line::Code(CodeLine {
                label: Some(b"%%done:".to_vec()),
                mnemonic: b"nop".to_vec(),
                operands: vec![],
                comment: None,
                opaque: false,
                flush_left: false,
            })
        );
        assert_eq!(
            lex_line(b"..@x:", &config()),
            Line::LabelOnly {
                label: b"..@x:".to_vec(),
                comment: None
            }
        );
        assert_eq!(
            lex_line(b"foo: ; note", &config()),
            Line::LabelOnly {
                label: b"foo:".to_vec(),
                comment: Some(b"; note".to_vec())
            }
        );
    }

    #[test]
    fn recognizes_equ_labels() {
        let line = code("foo equ 1 + 2");
        assert_eq!(line.label, Some(b"foo".to_vec()));
        assert_eq!(line.mnemonic, b"equ");
        assert_eq!(line.operands, vec![b"1 + 2".to_vec()]);
        assert!(line.opaque);
    }

    #[test]
    fn folds_prefixes_into_the_mnemonic() {
        assert_eq!(code("lock inc dword [x]").mnemonic, b"lock inc");
        assert_eq!(code("rep movsb").mnemonic, b"rep movsb");
        assert_eq!(code("a32 rep movsd").mnemonic, b"a32 rep movsd");
        assert_eq!(code("cs movsb").mnemonic, b"cs movsb");
        assert!(matches!(lex_line(b"rep", &config()), Line::Raw(_)));
    }

    #[test]
    fn times_is_opaque() {
        let line = code("times 510-($-$$) db 0");
        assert_eq!(line.mnemonic, b"times");
        assert_eq!(line.operands, vec![b"510-($-$$) db 0".to_vec()]);
        assert!(line.opaque);
    }

    #[test]
    fn keeps_preprocessor_lines_at_column_zero() {
        assert_eq!(
            lex_line(b"%DEFINE FOO 1", &config()),
            Line::Preproc(b"%define FOO 1".to_vec())
        );
        assert_eq!(
            lex_line(b"  %macro FOO 0", &config()),
            Line::Preproc(b"%macro FOO 0".to_vec())
        );
        assert_eq!(
            lex_line(b"%define BAR 1 ; c", &config()),
            Line::Preproc(b"%define BAR 1 ; c".to_vec())
        );
    }

    #[test]
    fn lowercases_mnemonics_only() {
        let line = code("MOV EAX, 1");
        assert_eq!(line.mnemonic, b"mov");
        assert_eq!(line.operands, vec![b"EAX".to_vec(), b"1".to_vec()]);
        assert_eq!(code("section .TEXT").operands, vec![b".TEXT".to_vec()]);
    }

    #[test]
    fn tightens_brackets_and_effective_addresses() {
        assert_eq!(code("mov rax,[ rbp - 8 ]").operands[1], b"[rbp-8]");
        assert_eq!(
            code("mov rcx, byte [ rbp - 8 ]").operands[1],
            b"byte [rbp-8]"
        );
        assert_eq!(code("db [rax + rbx * 4]").operands[0], b"[rax+rbx*4]");
        assert_eq!(code("db [rel msg]").operands[0], b"[rel msg]");
    }

    #[test]
    fn can_add_space_inside_brackets() {
        let mut config = config();
        config.bracket_spacing = true;
        let line = match lex_line(b"mov rax, [rbp-8]", &config) {
            Line::Code(line) => line,
            other => panic!("expected code, got {other:?}"),
        };
        assert_eq!(line.operands[1], b"[ rbp-8 ]");
    }

    #[test]
    fn normalizes_hex_outside_quotes_only() {
        let mut config = config();
        config.hex_case = HexCase::Upper;
        let line = match lex_line(b"mov rax, 0xff", &config) {
            Line::Code(line) => line,
            other => panic!("expected code, got {other:?}"),
        };
        assert_eq!(line.operands[1], b"0XFF");

        let line = match lex_line(b"db \"0xff\", 0xab", &config) {
            Line::Code(line) => line,
            other => panic!("expected code, got {other:?}"),
        };
        assert_eq!(line.operands[0], b"\"0xff\"");
        assert_eq!(line.operands[1], b"0XAB");

        config.hex_case = HexCase::Lower;
        let line = match lex_line(b"mov rax, 0XFF", &config) {
            Line::Code(line) => line,
            other => panic!("expected code, got {other:?}"),
        };
        assert_eq!(line.operands[1], b"0xff");
    }

    #[test]
    fn normalizes_comment_spacing() {
        assert_eq!(
            lex_line(b";foo", &config()),
            Line::Comment(b"; foo".to_vec())
        );
        assert_eq!(
            lex_line(b";---", &config()),
            Line::Comment(b";---".to_vec())
        );
        assert_eq!(
            lex_line(b";;x", &config()),
            Line::Comment(b";;x".to_vec())
        );
    }

    #[test]
    fn malformed_input_stays_raw() {
        assert!(matches!(lex_line(b"db 'abc", &config()), Line::Raw(_)));
        assert!(matches!(
            lex_line(b"mov eax, [rbp", &config()),
            Line::Raw(_)
        ));
        assert!(matches!(lex_line(b"mov eax, 1,", &config()), Line::Raw(_)));
    }
}
