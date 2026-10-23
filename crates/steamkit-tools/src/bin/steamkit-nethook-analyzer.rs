//! `steamkit-nethook-analyzer` - analyses the capture files of a NetHook2 dump.
//!
//! ```text
//! steamkit-nethook-analyzer <dump directory> [options]
//!
//! Options:
//!   --list                  List the captured packets (the default).
//!   --item <sequence>       Show the decoded detail of one packet.
//!   --all                   Show the decoded detail of every packet.
//!   --direction <in|out>    Only consider the packets of one direction.
//!   --filter <text>         Only consider the packets whose message name or
//!                           inner message name contains the text.
//!   --json                  Write the result as JSON.
//!   --show-unset-fields     Show the fields that were not set on the wire.
//!   --help                  Show this message.
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use steamkit_tools::nethook::tree::write_tree;
use steamkit_tools::nethook::{
    build_tree, default_specializations, NetHookDump, NetHookItem, PacketDirection, TreeNode,
};

/// What the tool should show.
#[derive(Clone, Copy, Debug)]
enum Mode {
    /// The list of the captured packets.
    List,
    /// The detail of one packet.
    Item(u32),
    /// The detail of every packet.
    All,
}

struct Options {
    directory: PathBuf,
    mode: Mode,
    direction: Option<PacketDirection>,
    filter: Option<String>,
    json: bool,
    show_unset_fields: bool,
}

fn print_usage() {
    println!(
        "steamkit-nethook-analyzer - analyse the capture files of a NetHook2 dump\n\
         \n\
         Usage:\n\
         \x20 steamkit-nethook-analyzer <dump directory> [options]\n\
         \n\
         Options:\n\
         \x20 --list                  List the captured packets (the default)\n\
         \x20 --item <sequence>       Show the decoded detail of one packet\n\
         \x20 --all                   Show the decoded detail of every packet\n\
         \x20 --direction <in|out>    Only consider the packets of one direction\n\
         \x20 --filter <text>         Only consider the packets whose message name or\n\
         \x20                          inner message name contains the text\n\
         \x20 --json                  Write the result as JSON\n\
         \x20 --show-unset-fields     Show the fields that were not set on the wire\n\
         \x20 --help                  Show this message"
    );
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut directory: Option<PathBuf> = None;
    let mut mode = Mode::List;
    let mut direction: Option<PacketDirection> = None;
    let mut filter: Option<String> = None;
    let mut json = false;
    let mut show_unset_fields = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--list" => mode = Mode::List,
            "--all" => mode = Mode::All,
            "--json" => json = true,
            "--show-unset-fields" => show_unset_fields = true,
            "--item" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--item requires a sequence number".to_owned())?;
                let sequence = value
                    .parse::<u32>()
                    .map_err(|_| format!("'{value}' is not a sequence number"))?;
                mode = Mode::Item(sequence);
            }
            "--direction" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--direction requires 'in' or 'out'".to_owned())?;
                direction = Some(match value.to_ascii_lowercase().as_str() {
                    "in" => PacketDirection::In,
                    "out" => PacketDirection::Out,
                    _ => return Err(format!("'{value}' is not a direction")),
                });
            }
            "--filter" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--filter requires a search text".to_owned())?;
                filter = Some(value);
            }
            other => {
                if let Some(value) = other.strip_prefix("--item=") {
                    let sequence = value
                        .parse::<u32>()
                        .map_err(|_| format!("'{value}' is not a sequence number"))?;
                    mode = Mode::Item(sequence);
                } else if let Some(value) = other.strip_prefix("--direction=") {
                    direction = Some(match value.to_ascii_lowercase().as_str() {
                        "in" => PacketDirection::In,
                        "out" => PacketDirection::Out,
                        _ => return Err(format!("'{value}' is not a direction")),
                    });
                } else if let Some(value) = other.strip_prefix("--filter=") {
                    filter = Some(value.to_owned());
                } else if other.starts_with('-') {
                    return Err(format!("unknown argument '{other}'"));
                } else if directory.is_none() {
                    directory = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unexpected argument '{other}'"));
                }
            }
        }
    }

    let directory = directory.ok_or_else(|| "a dump directory is required".to_owned())?;

    Ok(Some(Options {
        directory,
        mode,
        direction,
        filter,
        json,
        show_unset_fields,
    }))
}

/// Whether a packet passes the direction and the search filter.
fn matches(item: &NetHookItem, options: &Options, inner_message_name: Option<&str>) -> bool {
    if let Some(direction) = options.direction {
        if item.direction != direction {
            return false;
        }
    }

    let filter = match options.filter.as_ref() {
        Some(filter) => filter,
        None => return true,
    };

    let message_name = item.emsg_name();
    if message_name.to_lowercase().contains(&filter.to_lowercase()) {
        return true;
    }

    match inner_message_name {
        Some(inner) => inner.to_lowercase().contains(&filter.to_lowercase()),
        None => false,
    }
}

fn run(options: &Options) -> Result<(), String> {
    let mut dump = NetHookDump::new();
    dump.load_from_directory(&options.directory)
        .map_err(|error| error.to_string())?;

    let specializations = default_specializations();

    let mut selected: Vec<(&NetHookItem, Option<String>)> = Vec::new();
    for item in dump.items() {
        let inner_message_name = item.inner_message_name();
        if !matches(item, options, inner_message_name.as_deref()) {
            continue;
        }
        selected.push((item, inner_message_name));
    }

    match options.mode {
        Mode::List => {
            if options.json {
                let rows: Vec<serde_json::Value> = selected
                    .iter()
                    .map(|(item, inner)| {
                        serde_json::json!({
                            "sequence": item.sequence,
                            "timestamp": format_timestamp(item.timestamp),
                            "direction": item.direction.as_str(),
                            "message": item.emsg_name(),
                            "inner_message": inner,
                            "file": item.path.display().to_string(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::Value::Array(rows))
                        .map_err(|error| error.to_string())?
                );
            } else {
                print_list(&selected);
            }
        }
        Mode::Item(sequence) => {
            let found = selected
                .iter()
                .find(|(item, _)| item.sequence == sequence);

            let item = match found {
                Some((item, _)) => *item,
                None => return Err(format!("no packet with the sequence number {sequence}")),
            };

            let tree = build_tree(
                item,
                &dump,
                &specializations,
                options.show_unset_fields,
            );

            if options.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&tree).map_err(|error| error.to_string())?
                );
            } else {
                let mut out = String::new();
                write_tree(&mut out, &tree, 0);
                print!("{out}");
            }
        }
        Mode::All => {
            let trees: Vec<TreeNode> = selected
                .iter()
                .map(|(item, _)| {
                    build_tree(item, &dump, &specializations, options.show_unset_fields)
                })
                .collect();

            if options.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&trees).map_err(|error| error.to_string())?
                );
            } else {
                let mut out = String::new();
                for tree in &trees {
                    write_tree(&mut out, tree, 0);
                    out.push('\n');
                }
                print!("{out}");
            }
        }
    }

    Ok(())
}

/// Prints the list of the captured packets.
fn print_list(items: &[(&NetHookItem, Option<String>)]) {
    println!(
        "{:>6}  {:<19}  {:<3}  {:<32}  {}",
        "#", "Timestamp", "Dir", "Message", "Inner Message"
    );

    for (item, inner) in items {
        println!(
            "{:>6}  {:<19}  {:<3}  {:<32}  {}",
            item.sequence,
            format_timestamp(item.timestamp),
            item.direction.as_str(),
            item.emsg_name(),
            inner.as_deref().unwrap_or("")
        );
    }
}

/// Formats a timestamp as `YYYY-MM-DD hh:mm:ss` (UTC).
fn format_timestamp(time: SystemTime) -> String {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(_) => return String::new(),
    };

    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// The civil date of a number of days since the Unix epoch.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    (year, month as u32, day as u32)
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
