//! Configuration: defaults, the `.clean_ass.toml` subset, discovery, and CLI overrides.

use std::fs;
use std::path::{Path, PathBuf};

/// How hexadecimal literals (`0x...`) are cased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexCase {
    Preserve,
    Lower,
    Upper,
}

impl HexCase {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "preserve" => Some(Self::Preserve),
            "lower" => Some(Self::Lower),
            "upper" => Some(Self::Upper),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Lower => "lower",
            Self::Upper => "upper",
        }
    }
}

/// Formatting configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub indent: usize,
    pub align_gap: usize,
    pub min_gap: usize,
    pub soft_limit: usize,
    pub align_operands: bool,
    pub operand_gap: Option<usize>,
    pub comment_gap: usize,
    pub comment_column: Option<usize>,
    pub bracket_spacing: bool,
    pub hex_case: HexCase,
    pub max_width: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            indent: 4,
            align_gap: 8,
            min_gap: 2,
            soft_limit: 8,
            align_operands: false,
            operand_gap: None,
            comment_gap: 4,
            comment_column: None,
            bracket_spacing: false,
            hex_case: HexCase::Preserve,
            max_width: 0,
        }
    }
}

impl Config {
    /// Reject configurations that would make columns ambiguous or panic.
    pub fn validate(&self) -> Result<(), String> {
        if self.align_gap == 0 {
            return Err("align_gap must be at least 1".to_string());
        }
        if self.min_gap == 0 {
            return Err("min_gap must be at least 1".to_string());
        }
        if self.min_gap > self.align_gap {
            return Err("min_gap must not exceed align_gap".to_string());
        }
        if self.operand_gap == Some(0) {
            return Err("operand_gap must be at least 1".to_string());
        }
        if self.comment_gap == 0 {
            return Err("comment_gap must be at least 1".to_string());
        }
        if self.comment_column == Some(0) {
            return Err("comment_column must be at least 1".to_string());
        }
        if self.indent == usize::MAX {
            return Err("indent is invalid".to_string());
        }
        Ok(())
    }
}

/// Values supplied on the command line; everything is optional so the file
/// (or the defaults) can fill the rest in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub indent: Option<usize>,
    pub align_gap: Option<usize>,
    pub min_gap: Option<usize>,
    pub soft_limit: Option<usize>,
    pub align_operands: Option<bool>,
    pub operand_gap: Option<usize>,
    pub comment_gap: Option<usize>,
    pub comment_column: Option<usize>,
    pub bracket_spacing: Option<bool>,
    pub hex_case: Option<HexCase>,
    pub max_width: Option<usize>,
}

impl Overrides {
    pub fn apply(&self, config: &mut Config) {
        if let Some(value) = self.indent {
            config.indent = value;
        }
        if let Some(value) = self.align_gap {
            config.align_gap = value;
        }
        if let Some(value) = self.min_gap {
            config.min_gap = value;
        }
        if let Some(value) = self.soft_limit {
            config.soft_limit = value;
        }
        if let Some(value) = self.align_operands {
            config.align_operands = value;
        }
        if let Some(value) = self.operand_gap {
            config.operand_gap = Some(value);
        }
        if let Some(value) = self.comment_gap {
            config.comment_gap = value;
        }
        if let Some(value) = self.comment_column {
            config.comment_column = Some(value);
        }
        if let Some(value) = self.bracket_spacing {
            config.bracket_spacing = value;
        }
        if let Some(value) = self.hex_case {
            config.hex_case = value;
        }
        if let Some(value) = self.max_width {
            config.max_width = value;
        }
    }
}

/// Parse the supported TOML subset. Unknown keys are an error so typos are not
/// silently ignored.
pub fn parse_toml(text: &str) -> Result<Config, String> {
    let mut config = Config::default();

    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(format!(
                    "line {line_number}: malformed table header"
                ));
            }
            // Table names are accepted but ignored: all keys are flat.
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {line_number}: expected `key = value`"));
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err(format!("line {line_number}: expected `key = value`"));
        }

        let parsed = match key {
            "indent" => parse_int(key, value)?,
            "align_gap" => parse_int(key, value)?,
            "min_gap" => parse_int(key, value)?,
            "soft_limit" => parse_int(key, value)?,
            "operand_gap" => parse_int(key, value)?,
            "comment_gap" => parse_int(key, value)?,
            "comment_column" => parse_int(key, value)?,
            "max_width" => parse_int(key, value)?,
            _ => 0,
        };

        match key {
            "indent" => config.indent = parsed,
            "align_gap" => config.align_gap = parsed,
            "min_gap" => config.min_gap = parsed,
            "soft_limit" => config.soft_limit = parsed,
            "operand_gap" => config.operand_gap = Some(parsed),
            "comment_gap" => config.comment_gap = parsed,
            "comment_column" => config.comment_column = Some(parsed),
            "max_width" => config.max_width = parsed,
            "align_operands" => {
                config.align_operands = parse_bool(key, value)?
            }
            "bracket_spacing" => {
                config.bracket_spacing = parse_bool(key, value)?
            }
            "hex_case" => {
                let parsed = parse_string(value)?;
                config.hex_case = HexCase::parse(&parsed).ok_or_else(|| {
                    format!(
                        "line {line_number}: hex_case must be preserve, lower, or upper"
                    )
                })?;
            }
            other => {
                return Err(format!(
                    "line {line_number}: unknown key `{other}`"
                ));
            }
        }
    }

    config.validate()?;
    Ok(config)
}

/// Read and parse a config file, prefixing errors with its path.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    parse_toml(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Walk up from `start` (a file or directory) looking for `.clean_ass.toml`.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let start = start
        .canonicalize()
        .ok()
        .unwrap_or_else(|| start.to_path_buf());
    let mut dir = if start.is_dir() {
        start
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        let candidate = dir.join(".clean_ass.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote = 0u8;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if quote == 0 {
            if byte == b'"' || byte == b'\'' {
                quote = byte;
            } else if byte == b'#' {
                return &line[..index];
            }
        } else if byte == b'\\' {
            index += 1;
        } else if byte == quote {
            quote = 0;
        }
        index += 1;
    }
    line
}

fn parse_int(key: &str, value: &str) -> Result<usize, String> {
    if value.starts_with('-') {
        return Err(format!(
            "{key}: expected a non-negative integer, got `{value}`"
        ));
    }
    value.parse::<usize>().map_err(|_| {
        format!("{key}: expected a non-negative integer, got `{value}`")
    })
}

fn parse_bool(key: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{key}: expected true or false, got `{value}`")),
    }
}

fn parse_string(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return Err(format!("expected a quoted string, got `{value}`"));
    }

    let mut out = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                return Err(format!(
                    "unsupported escape `\\{other}` in string"
                ));
            }
            None => return Err("unterminated escape in string".to_string()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let config = Config::default();
        assert_eq!(config.indent, 4);
        assert_eq!(config.align_gap, 8);
        assert_eq!(config.min_gap, 2);
        assert_eq!(config.soft_limit, 8);
        assert_eq!(config.comment_gap, 4);
        assert!(!config.align_operands);
        assert!(!config.bracket_spacing);
        assert_eq!(config.hex_case, HexCase::Preserve);
        assert_eq!(config.operand_gap, None);
        assert_eq!(config.comment_column, None);
        config.validate().unwrap();
    }

    #[test]
    fn parses_flat_keys_and_comments() {
        let config = parse_toml(
            r#"
            # a comment
            indent = 2
            align_gap = 4   # trailing
            min_gap = 1
            soft_limit = 6
            align_operands = true
            operand_gap = 3
            comment_gap = 2
            comment_column = 40
            bracket_spacing = true
            hex_case = "upper"
            "#,
        )
        .unwrap();

        assert_eq!(config.indent, 2);
        assert_eq!(config.align_gap, 4);
        assert_eq!(config.min_gap, 1);
        assert_eq!(config.soft_limit, 6);
        assert!(config.align_operands);
        assert_eq!(config.operand_gap, Some(3));
        assert_eq!(config.comment_gap, 2);
        assert_eq!(config.comment_column, Some(40));
        assert!(config.bracket_spacing);
        assert_eq!(config.hex_case, HexCase::Upper);
    }

    #[test]
    fn accepts_and_ignores_table_headers() {
        let config = parse_toml("[format]\nindent = 8\n").unwrap();
        assert_eq!(config.indent, 8);
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(parse_toml("nope = 1\n").is_err());
        assert!(parse_toml("indent = -1\n").is_err());
        assert!(parse_toml("align_operands = maybe\n").is_err());
        assert!(parse_toml("hex_case = nope\n").is_err());
        assert!(parse_toml("indent\n").is_err());
        assert!(parse_toml("[format\nindent = 2\n").is_err());
    }

    #[test]
    fn rejects_impossible_gaps() {
        assert!(parse_toml("min_gap = 9\nalign_gap = 4\n").is_err());
        assert!(parse_toml("align_gap = 0\n").is_err());
        assert!(parse_toml("operand_gap = 0\n").is_err());
        assert!(parse_toml("comment_column = 0\n").is_err());
    }

    #[test]
    fn overrides_win_over_file_values() {
        let mut config =
            parse_toml("indent = 2\nhex_case = \"lower\"\n").unwrap();
        let overrides = Overrides {
            indent: Some(6),
            hex_case: Some(HexCase::Upper),
            ..Overrides::default()
        };
        overrides.apply(&mut config);
        assert_eq!(config.indent, 6);
        assert_eq!(config.hex_case, HexCase::Upper);
    }

    #[test]
    fn discovery_finds_the_nearest_file() {
        let root = std::env::temp_dir()
            .join(format!("clean_ass_config_{}", std::process::id()));
        let nested = root.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join(".clean_ass.toml"), "indent = 2\n").unwrap();
        fs::write(nested.join("x.asm"), "").unwrap();

        let found = discover(&nested.join("x.asm")).unwrap();
        assert_eq!(found, root.join(".clean_ass.toml"));
        assert_eq!(load(&found).unwrap().indent, 2);

        let _ = fs::remove_dir_all(&root);
    }
}
