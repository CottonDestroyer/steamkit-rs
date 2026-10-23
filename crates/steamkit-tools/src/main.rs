//! `steamkit-lang-gen` - generates the Rust SteamLanguage enums.
//!
//! ```text
//! steamkit-lang-gen [options]
//!
//! Options:
//!   --lang-dir <dir>   Directory that holds the `.steamd` definition files.
//!                      Default: <crate>/lang
//!   --entry <file>     The definition file to start from.
//!                      Default: steammsg.steamd (relative to --lang-dir)
//!   --out <file>       Where to write the generated Rust file.
//!                      Default: crates/steamkit/src/base/generated/steam_language.rs
//!   --stdout           Write the generated code to stdout instead of a file.
//!   --help             Show this message.
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use steamkit_tools::{default_language_dir, default_output_file, parse_file, Generator};

struct Options {
    language_dir: PathBuf,
    entry: String,
    output: Option<PathBuf>,
}

fn print_usage() {
    println!(
        "steamkit-lang-gen - generate Rust enums from Valve SteamLanguage definitions\n\
         \n\
         Usage:\n\
         \x20 steamkit-lang-gen [--lang-dir <dir>] [--entry <file>] [--out <file>] [--stdout]\n\
         \n\
         Options:\n\
         \x20 --lang-dir <dir>  Directory holding the `.steamd` files (default: <crate>/lang)\n\
         \x20 --entry <file>    Entry definition file (default: steammsg.steamd)\n\
         \x20 --out <file>      Output Rust file (default: crates/steamkit/src/base/generated/steam_language.rs)\n\
         \x20 --stdout          Write to stdout instead of a file\n\
         \x20 --help            Show this message"
    );
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut language_dir: Option<PathBuf> = None;
    let mut entry: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut to_stdout = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--stdout" => to_stdout = true,
            "--lang-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--lang-dir requires a directory".to_owned())?;
                language_dir = Some(PathBuf::from(value));
            }
            "--entry" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--entry requires a file name".to_owned())?;
                entry = Some(value);
            }
            "--out" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--out requires a file name".to_owned())?;
                output = Some(PathBuf::from(value));
            }
            other => {
                if let Some(value) = other.strip_prefix("--lang-dir=") {
                    language_dir = Some(PathBuf::from(value));
                } else if let Some(value) = other.strip_prefix("--entry=") {
                    entry = Some(value.to_owned());
                } else if let Some(value) = other.strip_prefix("--out=") {
                    output = Some(PathBuf::from(value));
                } else {
                    return Err(format!("unknown argument '{}'", other));
                }
            }
        }
    }

    if to_stdout {
        output = None;
    } else if output.is_none() {
        output = Some(default_output_file());
    }

    Ok(Some(Options {
        language_dir: language_dir.unwrap_or_else(default_language_dir),
        entry: entry.unwrap_or_else(|| "steammsg.steamd".to_owned()),
        output,
    }))
}

fn run(options: &Options) -> Result<(), String> {
    let entry = options.language_dir.join(&options.entry);
    if !entry.is_file() {
        return Err(format!(
            "entry definition file '{}' does not exist",
            entry.display()
        ));
    }

    let root = parse_file(&entry, &options.language_dir).map_err(|error| error.to_string())?;

    let source_label = format!("lang/{}", options.entry);

    let generator = Generator::new(root, source_label);
    let code = generator.generate().map_err(|error| error.to_string())?;

    let enum_count = code
        .lines()
        .filter(|line| line.trim_start().starts_with("pub enum "))
        .count();

    match &options.output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!("unable to create '{}': {}", parent.display(), error)
                })?;
            }
            std::fs::write(path, &code)
                .map_err(|error| format!("unable to write '{}': {}", path.display(), error))?;
            println!(
                "Wrote {} ({} enums, {} lines)",
                path.display(),
                enum_count,
                code.lines().count()
            );
        }
        None => print!("{}", code),
    }

    Ok(())
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(None) => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Ok(Some(options)) => options,
        Err(message) => {
            eprintln!("error: {}", message);
            eprintln!();
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {}", message);
            ExitCode::FAILURE
        }
    }
}
