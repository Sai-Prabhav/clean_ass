//! Measure pass and emit pass. Columns come from tokens, never from input padding.

use crate::config::Config;
use crate::lexer::{CodeLine, Line, lex_line};

/// A physical output line before comments are placed.
enum Physical {
    Blank,
    /// Preprocessor, comment-only, and raw lines: emitted as-is at column 0.
    Fixed(Vec<u8>),
    Code {
        code: Vec<u8>,
        comment: Option<Vec<u8>>,
    },
}

/// Format a whole source file.
pub fn format_source(input: &[u8], config: &Config) -> Vec<u8> {
    let (raw_lines, eol) = split_lines(input);

    // Lex, then collapse blank runs and drop leading/trailing blanks.
    let mut lines: Vec<Line> = Vec::new();
    for raw in raw_lines {
        let line = lex_line(raw, config);
        if matches!(line, Line::Blank)
            && (lines.is_empty() || matches!(lines.last(), Some(Line::Blank)))
        {
            continue;
        }
        lines.push(line);
    }
    while matches!(lines.last(), Some(Line::Blank)) {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }

    let longest = lines
        .iter()
        .filter_map(|line| match line {
            Line::Code(code) if !code.flush_left => Some(code.mnemonic.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let flush_longest_mnemonic = lines
        .iter()
        .filter_map(|line| match line {
            Line::Code(code) if code.flush_left => Some(code.mnemonic.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let gap = adaptive_gap(config, longest);
    let arg1_col = config.indent + longest + gap;
    let operand_gap = config.operand_gap.unwrap_or(gap);
    let widths = if config.align_operands {
        operand_widths(&lines)
    } else {
        Vec::new()
    };

    // Build every physical line except trailing comments.
    let mut emitted: Vec<Physical> = Vec::new();
    for line in &lines {
        match line {
            Line::Blank => emitted.push(Physical::Blank),
            Line::Comment(text) | Line::Preproc(text) | Line::Raw(text) => {
                emitted.push(Physical::Fixed(text.clone()));
            }
            Line::LabelOnly { label, comment } => {
                emitted.push(Physical::Code {
                    code: label.clone(),
                    comment: comment.clone(),
                })
            }
            Line::Code(code) => {
                emitted.push(build_code(
                    code,
                    config,
                    gap,
                    arg1_col,
                    &widths,
                    operand_gap,
                    flush_longest_mnemonic,
                ));
            }
        }
    }

    let max_code = emitted
        .iter()
        .filter_map(|line| match line {
            Physical::Code { code, .. } => Some(code.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let comment_col = config
        .comment_column
        .unwrap_or(max_code + config.comment_gap);

    let mut output: Vec<u8> = Vec::with_capacity(input.len());
    for (index, line) in emitted.iter().enumerate() {
        if index > 0 {
            output.extend_from_slice(eol);
        }
        match line {
            Physical::Blank => {}
            Physical::Fixed(text) => output.extend_from_slice(text),
            Physical::Code { code, comment } => {
                output.extend_from_slice(code);
                if let Some(comment) = comment {
                    let target = comment_col.max(code.len() + config.min_gap);
                    if code.len() < target {
                        output.resize(
                            output.len() + (target - code.len()),
                            b' ',
                        );
                    }
                    output.extend_from_slice(comment);
                }
            }
        }
    }
    output.extend_from_slice(eol);
    output
}

fn adaptive_gap(config: &Config, longest: usize) -> usize {
    let excess = longest.saturating_sub(config.soft_limit);
    let gap = config.align_gap.saturating_sub(excess);
    gap.max(config.min_gap).min(config.align_gap)
}

fn operand_widths(lines: &[Line]) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::new();
    for line in lines {
        let Line::Code(code) = line else {
            continue;
        };
        for (index, operand) in code.operands.iter().enumerate() {
            let width =
                operand.len() + usize::from(index + 1 < code.operands.len());
            if widths.len() <= index {
                widths.resize(index + 1, 0);
            }
            widths[index] = widths[index].max(width);
        }
    }
    widths
}

fn build_code(
    code: &CodeLine,
    config: &Config,
    _gap: usize,
    arg1_col: usize,
    widths: &[usize],
    operand_gap: usize,
    flush_longest_mnemonic: usize,
) -> Physical {
    let mut out: Vec<u8> = Vec::new();

    if let Some(label) = &code.label {
        if !code.flush_left {
            out.resize(config.indent, b' ');
        }
        out.extend_from_slice(label);
        if !code.mnemonic.is_empty() {
            if code.flush_left {
                let mnemonic_start = label.len() + 1;
                if out.len() < mnemonic_start {
                    out.resize(mnemonic_start, b' ');
                }
            } else {
                if out.len() < arg1_col {
                    out.resize(arg1_col, b' ');
                }
            }
        }
    } else if code.flush_left {
        // flush left
    } else if config.indent > 0 {
        out.resize(config.indent, b' ');
    }
    out.extend_from_slice(&code.mnemonic);

    if !code.operands.is_empty() {
        let first_col = if code.flush_left {
            if flush_longest_mnemonic == 0 {
                code.mnemonic.len() + 2
            } else {
                let g = adaptive_gap(config, flush_longest_mnemonic);
                let mut col = code.mnemonic.len() + g;
                let min_col = code.mnemonic.len() + 2;
                if col < min_col {
                    col = min_col;
                }
                col
            }
        } else {
            arg1_col.max(out.len() + config.min_gap)
        };
        if out.len() < first_col {
            out.resize(first_col, b' ');
        }

        if config.align_operands {
            for (index, operand) in code.operands.iter().enumerate() {
                if index > 0 {
                    let offset: usize = widths[..index]
                        .iter()
                        .map(|width| width + operand_gap)
                        .sum();
                    let target = first_col + offset;
                    let target = target.max(out.len() + config.min_gap);
                    if out.len() < target {
                        out.resize(target, b' ');
                    }
                }
                out.extend_from_slice(operand);
                if index + 1 < code.operands.len() {
                    out.push(b',');
                }
            }
        } else {
            for (index, operand) in code.operands.iter().enumerate() {
                if index == 0 {
                    out.extend_from_slice(operand);
                } else {
                    // Add comma and space before subsequent operands
                    out.push(b',');
                    out.push(b' ');
                    out.extend_from_slice(operand);
                }
            }
        }
    }

    Physical::Code {
        code: out,
        comment: code.comment.clone(),
    }
}

fn split_lines(input: &[u8]) -> (Vec<&[u8]>, &'static [u8]) {
    let mut crlf = false;
    if let Some(index) = input.iter().position(|&byte| byte == b'\n') {
        crlf = index > 0 && input[index - 1] == b'\r';
    }
    let eol: &'static [u8] = if crlf { b"\r\n" } else { b"\n" };

    let mut lines = Vec::new();
    let mut start = 0;
    for (index, &byte) in input.iter().enumerate() {
        if byte != b'\n' {
            continue;
        }
        let mut end = index;
        if end > start && input[end - 1] == b'\r' {
            end -= 1;
        }
        lines.push(&input[start..end]);
        start = index + 1;
    }
    if start < input.len() {
        lines.push(&input[start..]);
    }
    (lines, eol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, HexCase};

    fn pad(prefix: &str, column: usize, text: &str) -> String {
        assert!(prefix.len() <= column);
        format!("{prefix}{}{text}", " ".repeat(column - prefix.len()))
    }

    fn format(input: &str, config: &Config) -> String {
        String::from_utf8(format_source(input.as_bytes(), config)).unwrap()
    }

    #[test]
    fn short_mnemonics_share_one_operand_column() {
        let input =
            "section .text\nglobal _start\n_start:\nmov eax,1\nsyscall\n";
        let expected = format!(
            "section        .text\nglobal        _start\n_start:\n{}\n    syscall\n",
            pad("    mov", 19, "eax, 1"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn the_longest_mnemonic_sets_the_column() {
        let input = "mov rax,10\nverylonginstruction 100\n";
        let expected = format!(
            "{}\n{}\n",
            pad("    mov", 25, "rax, 10"),
            pad("    verylonginstruction", 25, "100"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn adaptive_gap_matches_the_table() {
        let cases = [
            ("mov eax,1\n", "    mov", 15, "eax, 1"),
            ("section .text\n", "section", 15, ".text"),
            ("cmpxchg16b [rdi]\n", "    cmpxchg16b", 20, "[rdi]"),
            (
                "vpbroadcastmb2q [rax]\n",
                "    vpbroadcastmb2q",
                21,
                "[rax]",
            ),
            (
                "verylonginstruction 100\n",
                "    verylonginstruction",
                25,
                "100",
            ),
        ];
        for (input, prefix, column, operand) in cases {
            if prefix == "section" {
                assert_eq!(
                    format(input, &Config::default()),
                    format!(
                        "section{}{}\n",
                        " ".repeat(column - prefix.len()),
                        operand
                    ),
                    "{input}"
                );
            } else {
                let expected = format!("{}\n", pad(prefix, column, operand));
                assert_eq!(
                    format(input, &Config::default()),
                    expected,
                    "{input}"
                );
            }
        }
    }

    #[test]
    fn tighter_align_gap_matches_the_table() {
        let config = Config {
            align_gap: 4,
            ..Config::default()
        };
        let cases = [
            ("mov eax,1\n", "    mov", 11, "eax, 1"),
            ("section .text\n", "section", 11, ".text"),
            ("cmpxchg16b [rdi]\n", "    cmpxchg16b", 16, "[rdi]"),
            (
                "vpbroadcastmb2q [rax]\n",
                "    vpbroadcastmb2q",
                21,
                "[rax]",
            ),
            (
                "verylonginstruction 100\n",
                "    verylonginstruction",
                25,
                "100",
            ),
        ];
        for (input, prefix, column, operand) in cases {
            if prefix == "section" {
                assert_eq!(
                    format(input, &config),
                    format!(
                        "section{}{}\n",
                        " ".repeat(column - prefix.len()),
                        operand
                    ),
                    "{input}"
                );
            } else {
                let expected = format!("{}\n", pad(prefix, column, operand));
                assert_eq!(format(input, &config), expected, "{input}");
            }
        }
    }

    #[test]
    fn aligns_operand_columns_when_asked() {
        let config = Config {
            align_gap: 2,
            align_operands: true,
            ..Config::default()
        };
        let input = "add rax, rbx\nmov rcx, 5\nimul rdx, rsi, 10\n";
        let expected = concat!(
            "    add   rax,  rbx\n",
            "    mov   rcx,  5\n",
            "    imul  rdx,  rsi,  10\n",
        );
        assert_eq!(format(input, &config), expected);
    }

    #[test]
    fn operand_gap_can_differ_from_align_gap() {
        let config = Config {
            align_gap: 4,
            align_operands: true,
            operand_gap: Some(4),
            ..Config::default()
        };
        let expected =
            format!("{}{}rbx\n", pad("    add", 11, "rax,"), " ".repeat(4));
        assert_eq!(format("add rax, rbx\n", &config), expected);
    }

    #[test]
    fn comments_align_to_the_longest_code_line() {
        let input = "mov eax,1 ; SYS_write\nsyscall ; invoke\n";
        let expected = concat!(
            "    mov            eax, 1    ; SYS_write\n",
            "    syscall                  ; invoke\n",
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn standalone_comments_stay_at_column_zero() {
        let input = "mov eax,1;foo\n;---\n";
        let expected =
            format!("{}    ; foo\n;---\n", pad("    mov", 15, "eax, 1"));
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn label_only_lines_with_comments_align() {
        let expected = "foo:    ; note\n";
        assert_eq!(format("foo: ; note\n", &Config::default()), expected);
    }

    #[test]
    fn inline_labels_are_moved_to_their_own_line() {
        let input = "section .text\nfoo: mov eax,1\n";
        let expected = format!(
            "section        .text\n{}\n",
            pad("    foo:", 15, "mov  eax, 1"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn equ_labels_stay_on_one_line() {
        let expected = format!("{}\n", pad("    foo:", 15, "equ  1"));
        assert_eq!(format("foo: equ 1\n", &Config::default()), expected);
    }

    #[test]
    fn blank_runs_collapse_and_edges_are_trimmed() {
        let expected = format!("{}\n", pad("    mov", 15, "eax, 1"));
        assert_eq!(
            format("\n\nmov eax,1\n\n\n", &Config::default()),
            expected
        );
        assert_eq!(format("", &Config::default()), "");
        assert_eq!(format("\n\n", &Config::default()), "");
    }

    #[test]
    fn crlf_is_preserved() {
        let input = b"mov eax,1\r\nret\r\n";
        let expected = b"    mov        eax, 1\r\n    ret\r\n";
        assert_eq!(format_source(input, &Config::default()), expected);
    }

    #[test]
    fn bracket_spacing_is_opt_in() {
        let config = Config {
            bracket_spacing: true,
            ..Config::default()
        };
        let expected = format!("{}\n", pad("    mov", 15, "rax, [ rbp-8 ]"));
        assert_eq!(format("mov rax,[rbp-8]\n", &config), expected);
    }

    #[test]
    fn hex_case_only_touches_operands() {
        let config = Config {
            hex_case: HexCase::Upper,
            ..Config::default()
        };
        let expected = format!("{}\n", pad("    mov", 15, "rax, 0XFF"));
        assert_eq!(format("mov rax,0xff\n", &config), expected);
        let expected = format!("{}\n", pad("    db", 14, "\"0xff\""));
        assert_eq!(format("db \"0xff\"\n", &config), expected);
    }

    #[test]
    fn preprocessor_lines_are_excluded_from_measurement() {
        let input = "%macro veryveryverylong 0\nmov eax,1\n%endmacro\n";
        let expected = format!(
            "%macro veryveryverylong 0\n{}\n%endmacro\n",
            pad("    mov", 15, "eax, 1"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn conditional_bodies_get_the_normal_indent() {
        let input = "%IFDEF X\nmov eax,1\n%ELSE\nmov eax,2\n%ENDIF\n";
        let expected = format!(
            "%ifdef X\n{}\n%else\n{}\n%endif\n",
            pad("    mov", 15, "eax, 1"),
            pad("    mov", 15, "eax, 2"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn raw_lines_pass_through_and_are_not_measured() {
        let input = "db 'unterminated\nmov eax, [rbp\ndb \"a,b\", 0\n";
        let expected = format!(
            "db 'unterminated\nmov eax, [rbp\n{}\n",
            pad("    db", 14, "\"a,b\", 0"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn times_and_brackets() {
        let input = "mov rax,[ rbp - 8 ]\ntimes 510-($-$$) db 0\n";
        let expected = format!(
            "{}\n{}\n",
            pad("    mov", 17, "rax, [rbp-8]"),
            pad("    times", 17, "510-($-$$) db 0"),
        );
        assert_eq!(format(input, &Config::default()), expected);
    }

    #[test]
    fn formatting_is_idempotent() {
        let input = concat!(
            "section .text\n",
            "global _start\n",
            "\n\n",
            "start: mov eax,1 ; go\n",
            "  db 'a;b', 0x0f\n",
            "%macro push 1\n",
            "push rax\n",
            "%endmacro\n",
            "mov rcx, [ rbp - 8 ]\n",
        );
        let once = format_source(input.as_bytes(), &Config::default());
        let twice = format_source(&once, &Config::default());
        assert_eq!(once, twice);
    }
}
