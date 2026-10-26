//! CLI entry point: file loop, stdin/stdout, `--in-place`, `--check`.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clean_ass::cli::{self, Command, Options};
use clean_ass::config::{self, Config};
use clean_ass::format_source;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("clean_ass: {message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match cli::parse(args)? {
        Command::Help => {
            write_stdout(cli::HELP.as_bytes())?;
            Ok(0)
        }
        Command::Version => {
            let version = format!("clean_ass {}\n", env!("CARGO_PKG_VERSION"));
            write_stdout(version.as_bytes())?;
            Ok(0)
        }
        Command::Run(options) => format_files(options),
    }
}

fn format_files(options: Options) -> Result<u8, String> {
    let explicit = match &options.config {
        Some(path) => Some(config::load(path)?),
        None => None,
    };

    if options.stdin {
        let mut config = match &explicit {
            Some(config) => config.clone(),
            None => discovered_config(None)?,
        };
        options.overrides.apply(&mut config);
        config.validate()?;

        let mut input = Vec::new();
        io::stdin()
            .read_to_end(&mut input)
            .map_err(|error| format!("stdin: {error}"))?;
        let output = format_source(&input, &config);

        if options.check {
            return Ok(if output == input { 0 } else { 1 });
        }
        write_stdout(&output)?;
        return Ok(0);
    }

    let mut differences = false;
    for path in &options.files {
        let input = fs::read(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let mut config = match &explicit {
            Some(config) => config.clone(),
            None => discovered_config(Some(path))?,
        };
        options.overrides.apply(&mut config);
        config.validate()?;

        let output = format_source(&input, &config);
        if options.check {
            if output != input {
                differences = true;
                eprintln!("would reformat {}", path.display());
            }
        } else if options.in_place {
            if output != input {
                fs::write(path, &output)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
            }
        } else {
            write_stdout(&output)?;
        }
    }

    Ok(if options.check && differences { 1 } else { 0 })
}

fn discovered_config(path: Option<&Path>) -> Result<Config, String> {
    let start: PathBuf = match path {
        Some(path) => {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                env::current_dir()
                    .map_err(|error| format!("current directory: {error}"))?
                    .join(path)
            }
        }
        None => env::current_dir()
            .map_err(|error| format!("current directory: {error}"))?,
    };
    match config::discover(&start) {
        Some(found) => config::load(&found),
        None => Ok(Config::default()),
    }
}

fn write_stdout(bytes: &[u8]) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    match stdout.write_all(bytes) {
        Ok(()) => stdout.flush().map_err(|error| format!("stdout: {error}")),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(format!("stdout: {error}")),
    }
}
