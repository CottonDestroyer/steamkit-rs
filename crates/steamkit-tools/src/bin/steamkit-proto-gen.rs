//! `steamkit-proto-gen` - generates Rust source from Valve's protobuf definitions.
//!
//! ```text
//! steamkit-proto-gen --namespace <ns> --proto <file> --output <file>
//! steamkit-proto-gen --namespace <ns> --proto-dir <dir> --output <file>
//!
//! Options:
//!   --namespace <ns>   The namespace the generated types live in.
//!   --proto <file>     The `.proto` file to generate code for.
//!   --proto-dir <dir>  Generate one file that holds every `.proto` of the directory.
//!   --output <file>    Where to write the generated Rust file.
//!   --include <dir>    An extra directory to search for imported files (repeatable).
//!   --stdout           Write the generated code to stdout instead of a file.
//!   --help             Show this message.
//! ```
//!
//! Turns Valve's `.proto` files into `prost` message types.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use steamkit_tools::proto_gen::{proto_files_in, ProtoSet, RustCodeGenerator};

struct Options {
    namespace: Option<String>,
    proto: Option<PathBuf>,
    proto_dir: Option<PathBuf>,
    output: Option<PathBuf>,
    includes: Vec<PathBuf>,
}

fn print_usage() {
    println!(
        "steamkit-proto-gen - generate Rust source from Valve protobuf definitions\n\
         \n\
         Usage:\n\
         \x20 steamkit-proto-gen --namespace <ns> (--proto <file> | --proto-dir <dir>) [--output <file>]\n\
         \n\
         Options:\n\
         \x20 --namespace <ns>   Namespace the generated types live in\n\
         \x20 --proto <file>     The `.proto` file to generate code for\n\
         \x20 --proto-dir <dir>  Generate one file that holds every `.proto` of the directory\n\
         \x20 --output <file>    Where to write the generated Rust file (default: stdout)\n\
         \x20 --include <dir>    An extra directory to search for imported files (repeatable)\n\
         \x20 --stdout           Write to stdout instead of a file\n\
         \x20 --help             Show this message"
    );
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut namespace: Option<String> = None;
    let mut proto: Option<PathBuf> = None;
    let mut proto_dir: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut includes: Vec<PathBuf> = Vec::new();
    let mut to_stdout = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--stdout" => to_stdout = true,
            "--namespace" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--namespace requires a value".to_owned())?;
                namespace = Some(value);
            }
            "--proto" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--proto requires a file name".to_owned())?;
                proto = Some(PathBuf::from(value));
            }
            "--proto-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--proto-dir requires a directory".to_owned())?;
                proto_dir = Some(PathBuf::from(value));
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--output requires a file name".to_owned())?;
                output = Some(PathBuf::from(value));
            }
            "--include" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--include requires a directory".to_owned())?;
                includes.push(PathBuf::from(value));
            }
            other => {
                if let Some(value) = other.strip_prefix("--namespace=") {
                    namespace = Some(value.to_owned());
                } else if let Some(value) = other.strip_prefix("--proto=") {
                    proto = Some(PathBuf::from(value));
                } else if let Some(value) = other.strip_prefix("--proto-dir=") {
                    proto_dir = Some(PathBuf::from(value));
                } else if let Some(value) = other.strip_prefix("--output=") {
                    output = Some(PathBuf::from(value));
                } else if let Some(value) = other.strip_prefix("--include=") {
                    includes.push(PathBuf::from(value));
                } else {
                    return Err(format!("unknown argument '{other}'"));
                }
            }
        }
    }

    if proto.is_some() && proto_dir.is_some() {
        return Err("--proto and --proto-dir cannot be used together".to_owned());
    }

    if proto.is_none() && proto_dir.is_none() {
        return Err("either --proto or --proto-dir is required".to_owned());
    }

    if to_stdout {
        output = None;
    }

    Ok(Some(Options {
        namespace,
        proto,
        proto_dir,
        output,
        includes,
    }))
}

fn run(options: &Options) -> Result<(), String> {
    let (protos, mut includes) = match (&options.proto, &options.proto_dir) {
        (Some(path), None) => {
            if !path.is_file() {
                return Err(format!("'{}' does not exist", path.display()));
            }

            let directory = path.parent().unwrap_or_else(|| Path::new(""));
            (vec![path.clone()], vec![directory.to_path_buf()])
        }
        (None, Some(directory)) => {
            if !directory.is_dir() {
                return Err(format!("'{}' is not a directory", directory.display()));
            }

            let files = proto_files_in(directory).map_err(|error| error.to_string())?;
            if files.is_empty() {
                return Err(format!(
                    "'{}' does not hold any .proto files",
                    directory.display()
                ));
            }

            (files, vec![directory.clone()])
        }
        _ => return Err("either --proto or --proto-dir is required".to_owned()),
    };

    includes.extend(options.includes.iter().cloned());

    let set = ProtoSet::compile(&protos, &includes).map_err(|error| error.to_string())?;

    let files = match &options.proto {
        Some(path) => {
            let descriptor = set
                .file_by_path(path)
                .ok_or_else(|| format!("'{}' was not compiled", path.display()))?;
            let name = descriptor.proto.name.clone().unwrap_or_default();
            set.closure(&name).map_err(|error| error.to_string())?
        }
        None => set.files.clone(),
    };

    let generator = RustCodeGenerator::new(options.namespace.clone());
    let code = generator.generate(files).map_err(|error| error.to_string())?;

    match &options.output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        format!("unable to create '{}': {}", parent.display(), error)
                    })?;
                }
            }

            std::fs::write(path, &code)
                .map_err(|error| format!("unable to write '{}': {}", path.display(), error))?;

            println!(
                "Wrote {} ({} lines)",
                path.display(),
                code.lines().count()
            );
        }
        None => print!("{code}"),
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
