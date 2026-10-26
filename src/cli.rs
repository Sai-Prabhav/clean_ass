//! Hand-rolled command-line parsing. No dependencies, no magic.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::config::{HexCase, Overrides};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Options {
    pub files: Vec<PathBuf>,
    pub in_place: bool,
    pub check: bool,
    pub stdin: bool,
    pub config: Option<PathBuf>,
    pub overrides: Overrides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
}

pub const HELP: &str = "\
clean_ass - format NASM/YASM assembly (Intel syntax)

USAGE:
    clean_ass [OPTIONS] [FILE...]

OPTIONS:
    -i, --in-place          rewrite files instead of writing to stdout
    -c, --check             exit 1 if any file would change; write nothing
        --stdin             read stdin (also: no paths, or \"-\")
        --config <PATH>     explicit config file
        --indent <N>        instruction indent (default 4)
        --align-gap <N>     gap after the longest mnemonic (default 8)
        --min-gap <N>       minimum gap between fields (default 2)
        --soft-limit <N>    mnemonic length before the gap shrinks (default 8)
        --align-operands    align arg2, arg3, ... as columns
        --operand-gap <N>   gap between operands (default: adaptive gap)
        --comment-gap <N>   gap before trailing comments (default 4)
        --comment-column <N> fixed comment column (default: auto)
        --bracket-spacing   put spaces inside brackets
        --hex-case <CASE>   preserve | lower | upper (default preserve)
    -h, --help              print this help
    -V, --version           print the version

EXIT CODES:
    0  success
    1  --check found differences
    2  usage or I/O error
";

/// Parse arguments (without the program name).
pub fn parse<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut options = Options::default();
    let mut iter = args.into_iter();
    let mut positional_only = false;

    while let Some(arg) = iter.next() {
        if positional_only {
            options.files.push(PathBuf::from(arg));
            continue;
        }

        if let Some(text) = arg.to_str() {
            if text == "--" {
                positional_only = true;
                continue;
            }
            if text == "-" {
                options.files.push(PathBuf::from(arg));
                continue;
            }
            if !text.starts_with('-') || text.is_empty() {
                options.files.push(PathBuf::from(arg));
                continue;
            }
        } else {
            if arg.to_string_lossy().starts_with('-') {
                return Err("arguments must be valid UTF-8".to_string());
            }
            options.files.push(PathBuf::from(arg));
            continue;
        }

        let Some(text) = arg.to_str() else {
            continue;
        };

        if !text.starts_with('-') {
            options.files.push(PathBuf::from(arg));
            continue;
        }

        if let Some(long) = text.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            match name {
                "help" => {
                    require_no_inline(inline, "--help")?;
                    return Ok(Command::Help);
                }
                "version" => {
                    require_no_inline(inline, "--version")?;
                    return Ok(Command::Version);
                }
                "in-place" => {
                    require_no_inline(inline, "--in-place")?;
                    options.in_place = true;
                }
                "check" => {
                    require_no_inline(inline, "--check")?;
                    options.check = true;
                }
                "stdin" => {
                    require_no_inline(inline, "--stdin")?;
                    options.stdin = true;
                }
                "align-operands" => {
                    require_no_inline(inline, "--align-operands")?;
                    options.overrides.align_operands = Some(true);
                }
                "bracket-spacing" => {
                    require_no_inline(inline, "--bracket-spacing")?;
                    options.overrides.bracket_spacing = Some(true);
                }
                "config" => {
                    let value = take_value(&mut iter, inline, "--config")?;
                    options.config = Some(PathBuf::from(value));
                }
                "indent" => {
                    options.overrides.indent = Some(parse_usize(
                        take_value(&mut iter, inline, "--indent")?,
                        "--indent",
                    )?);
                }
                "align-gap" => {
                    options.overrides.align_gap = Some(parse_usize(
                        take_value(&mut iter, inline, "--align-gap")?,
                        "--align-gap",
                    )?);
                }
                "min-gap" => {
                    options.overrides.min_gap = Some(parse_usize(
                        take_value(&mut iter, inline, "--min-gap")?,
                        "--min-gap",
                    )?);
                }
                "soft-limit" => {
                    options.overrides.soft_limit = Some(parse_usize(
                        take_value(&mut iter, inline, "--soft-limit")?,
                        "--soft-limit",
                    )?);
                }
                "operand-gap" => {
                    options.overrides.operand_gap = Some(parse_usize(
                        take_value(&mut iter, inline, "--operand-gap")?,
                        "--operand-gap",
                    )?);
                }
                "comment-gap" => {
                    options.overrides.comment_gap = Some(parse_usize(
                        take_value(&mut iter, inline, "--comment-gap")?,
                        "--comment-gap",
                    )?);
                }
                "comment-column" => {
                    options.overrides.comment_column = Some(parse_usize(
                        take_value(&mut iter, inline, "--comment-column")?,
                        "--comment-column",
                    )?);
                }
                "hex-case" => {
                    let value = take_value(&mut iter, inline, "--hex-case")?;
                    let value = value.to_str().ok_or_else(|| {
                        "--hex-case: invalid UTF-8 value".to_string()
                    })?;
                    let parsed = HexCase::parse(value).ok_or_else(|| {
                        format!("--hex-case: expected preserve, lower, or upper, got `{value}`")
                    })?;
                    options.overrides.hex_case = Some(parsed);
                }
                "max-width" => {
                    options.overrides.max_width = Some(parse_usize(
                        take_value(&mut iter, inline, "--max-width")?,
                        "--max-width",
                    )?);
                }
                other => return Err(format!("unknown option `--{other}`")),
            }
            continue;
        }

        // Short flags, combinable: -i, -c, -h, -V.
        for flag in text[1..].chars() {
            match flag {
                'i' => options.in_place = true,
                'c' => options.check = true,
                'h' => return Ok(Command::Help),
                'V' => return Ok(Command::Version),
                other => return Err(format!("unknown option `-{other}`")),
            }
        }
    }

    if options.in_place && options.check {
        return Err("--in-place and --check cannot be combined".to_string());
    }

    if options
        .files
        .iter()
        .any(|path| path.as_os_str() == OsStr::new("-"))
    {
        if options.files.len() > 1 {
            return Err("cannot mix stdin (`-`) with file paths".to_string());
        }
        options.files.clear();
        options.stdin = true;
    }

    if options.stdin && !options.files.is_empty() {
        return Err("--stdin cannot be combined with file paths".to_string());
    }
    if options.files.is_empty() {
        options.stdin = true;
    }
    if options.stdin && options.in_place {
        return Err("cannot use --in-place with stdin".to_string());
    }
    if options.files.len() > 1 && !options.in_place && !options.check {
        return Err("multiple files require --in-place or --check".to_string());
    }

    Ok(Command::Run(options))
}

fn require_no_inline(inline: Option<&str>, flag: &str) -> Result<(), String> {
    if inline.is_some() {
        return Err(format!("{flag} does not take a value"));
    }
    Ok(())
}

fn take_value<I>(
    iter: &mut I,
    inline: Option<&str>,
    flag: &str,
) -> Result<OsString, String>
where
    I: Iterator<Item = OsString>,
{
    match inline {
        Some(value) => Ok(OsString::from(value)),
        None => iter.next().ok_or_else(|| format!("{flag}: missing value")),
    }
}

fn parse_usize(value: OsString, flag: &str) -> Result<usize, String> {
    let text = value
        .to_str()
        .ok_or_else(|| format!("{flag}: invalid UTF-8 value"))?;
    text.parse::<usize>().map_err(|_| {
        format!("{flag}: expected a non-negative integer, got `{text}`")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    fn run(args: &[&str]) -> Options {
        match parse_args(args).unwrap() {
            Command::Run(options) => options,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn no_args_means_stdin() {
        let options = run(&[]);
        assert!(options.stdin);
        assert!(options.files.is_empty());
        assert!(!options.in_place);
        assert!(!options.check);
    }

    #[test]
    fn in_place_allows_multiple_files() {
        let options = run(&["-i", "a.asm", "b.asm"]);
        assert!(options.in_place);
        assert_eq!(options.files.len(), 2);
        assert!(!options.stdin);
    }

    #[test]
    fn check_allows_multiple_files() {
        let options = run(&["--check", "a.asm", "b.asm"]);
        assert!(options.check);
        assert_eq!(options.files.len(), 2);
    }

    #[test]
    fn multiple_files_without_mode_is_an_error() {
        assert!(parse_args(&["a.asm", "b.asm"]).is_err());
    }

    #[test]
    fn in_place_and_check_conflict() {
        assert!(parse_args(&["-i", "-c", "a.asm"]).is_err());
    }

    #[test]
    fn stdin_conflicts_with_files() {
        assert!(parse_args(&["--stdin", "a.asm"]).is_err());
        assert!(parse_args(&["-", "a.asm"]).is_err());
        assert!(parse_args(&["--stdin", "-i"]).is_err());
    }

    #[test]
    fn dash_alone_means_stdin() {
        let options = run(&["-"]);
        assert!(options.stdin);
        assert!(options.files.is_empty());
    }

    #[test]
    fn parses_value_flags_both_forms() {
        let options = run(&[
            "--indent=2",
            "--align-gap",
            "4",
            "--min-gap=1",
            "--soft-limit",
            "6",
            "--operand-gap",
            "3",
            "--comment-gap=2",
            "--comment-column",
            "40",
            "--hex-case",
            "upper",
            "--align-operands",
            "--bracket-spacing",
            "--config",
            "cfg.toml",
        ]);
        let overrides = &options.overrides;
        assert_eq!(overrides.indent, Some(2));
        assert_eq!(overrides.align_gap, Some(4));
        assert_eq!(overrides.min_gap, Some(1));
        assert_eq!(overrides.soft_limit, Some(6));
        assert_eq!(overrides.operand_gap, Some(3));
        assert_eq!(overrides.comment_gap, Some(2));
        assert_eq!(overrides.comment_column, Some(40));
        assert_eq!(overrides.hex_case, Some(HexCase::Upper));
        assert_eq!(overrides.align_operands, Some(true));
        assert_eq!(overrides.bracket_spacing, Some(true));
        assert_eq!(options.config, Some(PathBuf::from("cfg.toml")));
    }

    #[test]
    fn double_dash_ends_option_parsing() {
        let options = run(&["-i", "--", "-weird.asm"]);
        assert_eq!(options.files, vec![PathBuf::from("-weird.asm")]);
    }

    #[test]
    fn help_and_version_win_early() {
        assert_eq!(parse_args(&["--help"]).unwrap(), Command::Help);
        assert_eq!(parse_args(&["-V"]).unwrap(), Command::Version);
        assert_eq!(parse_args(&["-h"]).unwrap(), Command::Help);
    }

    #[test]
    fn rejects_unknown_and_malformed_options() {
        assert!(parse_args(&["--nope"]).is_err());
        assert!(parse_args(&["-z"]).is_err());
        assert!(parse_args(&["--indent"]).is_err());
        assert!(parse_args(&["--indent", "x"]).is_err());
        assert!(parse_args(&["--hex-case", "sideways"]).is_err());
        assert!(parse_args(&["--stdin=true"]).is_err());
    }
}
