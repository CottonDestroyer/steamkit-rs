//! `steamkit-proto-dump` - dumps the protobuf definitions of Steam's binaries.
//!
//! ```text
//! steamkit-proto-dump [options] [<target>...] [<output-dir>]
//!
//! Options:
//!   -v             Print the version.
//!   -dump          Also write the raw bytes of every descriptor that was found.
//!   --protos <dir> Read the `.proto` files of <dir> instead of scanning a binary.
//!   --stdout       Write the dumped definitions to stdout instead of files.
//!   --help         Show this message.
//! ```
//!
//! Finds the protobuf descriptors embedded in a Steam binary and writes them
//! back out as `.proto` files.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use steamkit_tools::proto_dumper::executable_scanner::scan_path;
use steamkit_tools::proto_dumper::protobuf_collector::{CandidateResult, ProtobufCollector};
use steamkit_tools::proto_dumper::protobuf_dumper::ProtobufDumper;
use steamkit_tools::proto_gen::{proto_files_in, ProtoSet};

struct Options {
    targets: Vec<String>,
    output: Option<String>,
    has_dump_candidates: bool,
    protos: Option<PathBuf>,
    to_stdout: bool,
}

fn print_usage() {
    println!(
        "steamkit-proto-dump - dump the protobuf definitions of Steam's binaries\n\
         \n\
         Usage:\n\
         \x20 steamkit-proto-dump [-v] [-dump] [--stdout] <target>... [<output-dir>]\n\
         \x20 steamkit-proto-dump [--protos <dir>] [--stdout] [<output-dir>]\n\
         \n\
         Options:\n\
         \x20 -v              Print the version\n\
         \x20 -dump           Also write the raw bytes of every descriptor that was found\n\
         \x20 --protos <dir> Read the `.proto` files of <dir> instead of scanning a binary\n\
         \x20 --stdout        Write the dumped definitions to stdout instead of files\n\
         \x20 --help          Show this message"
    );
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut named: Vec<String> = Vec::new();
    let mut unnamed: Vec<String> = Vec::new();
    let mut protos: Option<PathBuf> = None;
    let mut to_stdout = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--protos" {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| "--protos requires a directory".to_owned())?;
            protos = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--protos=") {
            protos = Some(PathBuf::from(value));
        } else if arg == "--stdout" {
            to_stdout = true;
        } else if arg == "--help" || arg == "-h" {
            return Ok(None);
        } else if arg.starts_with('-') {
            named.push(arg.clone());
        } else {
            unnamed.push(arg.clone());
        }

        index += 1;
    }

    let has_dump_candidates = named
        .iter()
        .any(|arg| arg.eq_ignore_ascii_case("-dump"));

    if protos.is_some() {
        let mut output = None;
        if let Some(value) = unnamed.first() {
            output = Some(value.clone());
        }

        return Ok(Some(Options {
            targets: Vec::new(),
            output,
            has_dump_candidates,
            protos,
            to_stdout,
        }));
    }

    if unnamed.is_empty() {
        if named.iter().any(|arg| arg == "-v") {
            println!(
                "ProtobufDumper version: {}",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(Some(Options {
                targets: Vec::new(),
                output: None,
                has_dump_candidates,
                protos: None,
                to_stdout,
            }));
        }

        println!("No target specified.");
        return Err(String::new());
    }

    let mut targets: Vec<String> = Vec::new();
    let mut output: Option<String> = None;

    for (position, value) in unnamed.iter().enumerate() {
        let exists = Path::new(value).is_file();
        let is_last = position + 1 == unnamed.len();

        if position == 0 || !is_last {
            targets.push(value.clone());

            if exists {
                continue;
            }

            println!("Could not find file {value}");
            return Err(String::new());
        } else {
            output = Some(value.clone());

            if !exists {
                continue;
            }

            println!("Output directory path is not valid {value}");
            return Err(String::new());
        }
    }

    Ok(Some(Options {
        targets,
        output,
        has_dump_candidates,
        protos,
        to_stdout,
    }))
}

fn run(options: &Options) -> Result<(), String> {
    if let Some(directory) = &options.protos {
        return dump_proto_directory(options, directory);
    }

    if options.targets.is_empty() {
        return Ok(());
    }

    let output_dir = match &options.output {
        Some(output) => PathBuf::from(output),
        None => PathBuf::from(
            Path::new(&options.targets[0])
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    };

    let mut collector = ProtobufCollector::new();

    for target in &options.targets {
        println!("Loading binary '{target}'...");

        let dump_candidates = options.has_dump_candidates;
        let output_dir = output_dir.clone();
        let collector_ref = &mut collector;

        scan_path(Path::new(target), |name, buffer| {
            if collector_ref
                .candidates
                .iter()
                .any(|candidate| candidate.proto.name.as_deref() == Some(name))
            {
                return Some(0);
            }

            print!("{name}... ");

            let (result, error, bytes_consumed) =
                collector_ref.try_parse_candidate(buffer);

            match result {
                CandidateResult::Ok => {
                    println!("{}", green("OK!"));
                }
                CandidateResult::Invalid => {
                    println!(
                        "{}",
                        red(&format!(
                            "is invalid: {}",
                            error.unwrap_or_else(|| "unknown error".to_owned())
                        ))
                    );
                }
            }

            if dump_candidates {
                let file_name = output_dir.join(format!("{name}.dump"));
                match std::fs::create_dir_all(
                    file_name.parent().unwrap_or_else(|| Path::new(".")),
                ) {
                    Ok(()) => match std::fs::write(&file_name, buffer) {
                        Ok(()) => println!("  ! Dumping to '{}'!", file_name.display()),
                        Err(error) => println!("Unable to dump: {error}"),
                    },
                    Err(error) => println!("Unable to dump: {error}"),
                }
            }

            Some(bytes_consumed)
        })
        .map_err(|error| error.to_string())?;
    }

    let mut dumper = ProtobufDumper::new(collector.candidates.clone());

    if !dumper.analyze() {
        println!(
            "{}",
            red("Dump failed. Not all dependencies and types were found.")
        );
        return Err(String::new());
    }

    dumper.dump_files(|name, buffer| {
        if options.to_stdout {
            println!("// {name}");
            println!("{buffer}");
            return;
        }

        let output_file = output_dir.join(name);
        println!("  ! Outputting proto to '{}'", output_file.display());

        if let Some(parent) = output_file.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                println!("Unable to write '{}': {}", output_file.display(), error);
                return;
            }
        }

        if let Err(error) = std::fs::write(&output_file, buffer) {
            println!("Unable to write '{}': {}", output_file.display(), error);
        }
    });

    Ok(())
}

/// Dumps the definitions of the `.proto` files of a directory.
fn dump_proto_directory(options: &Options, directory: &Path) -> Result<(), String> {
    if !directory.is_dir() {
        return Err(format!("'{}' is not a directory", directory.display()));
    }

    let protos = proto_files_in(directory).map_err(|error| error.to_string())?;
    if protos.is_empty() {
        return Err(format!(
            "'{}' does not hold any .proto files",
            directory.display()
        ));
    }

    let set = ProtoSet::compile(&protos, &[directory.to_path_buf()])
        .map_err(|error| error.to_string())?;

    let output_dir = match &options.output {
        Some(output) => PathBuf::from(output),
        None => PathBuf::from(
            directory
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    };

    let mut dumper = ProtobufDumper::new(set.files.clone());

    if !dumper.analyze() {
        println!(
            "{}",
            red("Dump failed. Not all dependencies and types were found.")
        );
        return Err(String::new());
    }

    dumper.dump_files(|name, buffer| {
        if options.to_stdout {
            println!("// {name}");
            println!("{buffer}");
            return;
        }

        let output_file = output_dir.join(name);
        println!("  ! Outputting proto to '{}'", output_file.display());

        if let Some(parent) = output_file.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                println!("Unable to write '{}': {}", output_file.display(), error);
                return;
            }
        }

        if let Err(error) = std::fs::write(&output_file, buffer) {
            println!("Unable to write '{}': {}", output_file.display(), error);
        }
    });

    Ok(())
}

/// Wraps a message in the ANSI colour for a successful result.
fn green(text: &str) -> String {
    colorize(text, "\u{1b}[32m", "\u{1b}[0m")
}

/// Wraps a message in the ANSI colour for a failed result.
fn red(text: &str) -> String {
    colorize(text, "\u{1b}[31m", "\u{1b}[0m")
}

/// Wraps a message in an ANSI colour when the output is a terminal.
fn colorize(text: &str, start: &str, end: &str) -> String {
    if std::io::stdout().is_terminal() {
        format!("{start}{text}{end}")
    } else {
        text.to_owned()
    }
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(None) => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Ok(Some(options)) => options,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("error: {}", message);
            }
            return ExitCode::FAILURE;
        }
    };

    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("error: {}", message);
            }
            ExitCode::FAILURE
        }
    }
}
