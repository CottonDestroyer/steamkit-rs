//! A single captured packet.
//!
//! A NetHook dump is a directory of capture files. Every file holds one
//! captured Steam packet and is named after it:
//!
//! ```text
//! <sequence>_<direction>_<message number>_<message name>.bin
//! ```
//!
//! The direction is `in` for a packet the Steam client received and `out` for
//! one it sent. Only the first three parts matter; they're matched like the
//! regex `(?<num>\d+)_(?<direction>in|out)_(?<emsg>\d+)` and the rest is
//! ignored.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use steamkit::base::generated::steam_language::EMsg;

use crate::nethook::error::{NetHookError, NetHookResult};
use crate::nethook::message_reading;

/// The direction a captured packet travelled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketDirection {
    /// The packet was received by the Steam client.
    In,
    /// The packet was sent by the Steam client.
    Out,
}

impl PacketDirection {
    /// `"in"` or `"out"`.
    pub fn as_str(self) -> &'static str {
        match self {
            PacketDirection::In => "in",
            PacketDirection::Out => "out",
        }
    }
}

/// The parts of the name of a capture file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureFileName {
    /// The sequence number of the packet within the dump.
    pub sequence: u32,
    /// The direction the packet travelled in.
    pub direction: PacketDirection,
    /// The number of the Steam message.
    pub emsg: u32,
}

/// Parses the name of a capture file.
///
/// Behaves like the case-insensitive regex
/// `(?<num>\d+)_(?<direction>in|out)_(?<emsg>\d+)`: the leftmost match
/// anywhere in the name wins, and the rest of the name is ignored.
pub fn parse_capture_file_name(name: &str) -> Option<CaptureFileName> {
    let bytes = name.as_bytes();

    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_digit() {
            continue;
        }

        let mut index = start;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let sequence_end = index;

        if index >= bytes.len() || bytes[index] != b'_' {
            continue;
        }
        index += 1;

        let direction = if name[index..].len() >= 2
            && name[index..].as_bytes()[..2].eq_ignore_ascii_case(b"in")
        {
            PacketDirection::In
        } else if name[index..].len() >= 3
            && name[index..].as_bytes()[..3].eq_ignore_ascii_case(b"out")
        {
            PacketDirection::Out
        } else {
            continue;
        };
        index += if direction == PacketDirection::In { 2 } else { 3 };

        if index >= bytes.len() || bytes[index] != b'_' {
            continue;
        }
        index += 1;

        let emsg_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }

        if index == emsg_start {
            continue;
        }

        let sequence = match name[start..sequence_end].parse::<u32>() {
            Ok(sequence) => sequence,
            Err(_) => continue,
        };

        let emsg = match name[emsg_start..index].parse::<u32>() {
            Ok(emsg) => emsg,
            Err(_) => continue,
        };

        return Some(CaptureFileName {
            sequence,
            direction,
            emsg,
        });
    }

    None
}

/// One captured packet of a NetHook dump.
#[derive(Clone, Debug)]
pub struct NetHookItem {
    /// The sequence number of the packet within the dump.
    pub sequence: u32,
    /// The time the capture file was last written.
    pub timestamp: SystemTime,
    /// The direction the packet travelled in.
    pub direction: PacketDirection,
    /// The message type of the packet.
    pub emsg: EMsg,
    /// The number of the message, as the capture file name spells it.
    pub raw_emsg: u32,
    /// The capture file of the packet.
    pub path: PathBuf,
}

impl NetHookItem {
    /// Loads a capture file.
    ///
    /// Returns `Ok(None)` when the file name doesn't follow the capture naming
    /// scheme.
    pub fn load_from_file(path: &Path) -> NetHookResult<Option<NetHookItem>> {
        let name = match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => return Ok(None),
        };

        let parsed = match parse_capture_file_name(&name) {
            Some(parsed) => parsed,
            None => return Ok(None),
        };

        let timestamp = std::fs::metadata(path)
            .map_err(|source| NetHookError::Io {
                path: path.display().to_string(),
                source,
            })?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(Some(NetHookItem {
            sequence: parsed.sequence,
            timestamp,
            direction: parsed.direction,
            emsg: EMsg::from_value(parsed.emsg as i32).unwrap_or(EMsg::Invalid),
            raw_emsg: parsed.emsg,
            path: path.to_path_buf(),
        }))
    }

    /// The message name, or its number if the `EMsg` enum doesn't know it.
    pub fn emsg_name(&self) -> String {
        match EMsg::from_value(self.raw_emsg as i32) {
            Some(emsg) => format!("{emsg:?}"),
            None => self.raw_emsg.to_string(),
        }
    }

    /// The bytes of the capture file.
    pub fn read_bytes(&self) -> NetHookResult<Vec<u8>> {
        std::fs::read(&self.path).map_err(|source| NetHookError::Io {
            path: self.path.display().to_string(),
            source,
        })
    }

    /// The name of the message inside the packet.
    ///
    /// This is the unified messaging method or the game coordinator message
    /// the packet carries, if any.
    pub fn inner_message_name(&self) -> Option<String> {
        let bytes = self.read_bytes().ok()?;
        message_reading::inner_message_name(self.emsg, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_capture_file_name, PacketDirection};

    #[test]
    fn parses_the_names_the_sniffer_writes() {
        let cases: [(&str, u32, PacketDirection, u32); 5] = [
            ("001_in_2_ProtoBuf.bin", 1, PacketDirection::In, 2),
            ("001_out_65535_ProtoBuf.bin", 1, PacketDirection::Out, 65535),
            ("042_in_0_Null.bin", 42, PacketDirection::In, 0),
            ("1000_out_1_Name.bin", 1000, PacketDirection::Out, 1),
            ("099_in_70000_Large.bin", 99, PacketDirection::In, 70000),
        ];

        for (name, sequence, direction, emsg) in cases {
            let parsed = parse_capture_file_name(name)
                .unwrap_or_else(|| panic!("{name} must be recognised as a capture file"));

            assert_eq!(sequence, parsed.sequence, "{name}");
            assert_eq!(direction, parsed.direction, "{name}");
            assert_eq!(emsg, parsed.emsg, "{name}");
        }
    }

    #[test]
    fn parses_the_direction_without_regard_to_case() {
        let parsed = parse_capture_file_name("001_IN_2_Name.bin").expect("the name must be parsed");

        assert_eq!(PacketDirection::In, parsed.direction);

        let parsed = parse_capture_file_name("001_Out_2_Name.bin").expect("the name must be parsed");

        assert_eq!(PacketDirection::Out, parsed.direction);
    }

    #[test]
    fn takes_the_leftmost_match_of_the_pattern() {
        let parsed = parse_capture_file_name("abc12_in_2_Name.bin").expect("the name must be parsed");

        assert_eq!(12, parsed.sequence);
        assert_eq!(2, parsed.emsg);
    }

    #[test]
    fn rejects_names_without_the_pattern() {
        assert!(parse_capture_file_name("not-a-capture.bin").is_none());
        assert!(parse_capture_file_name("001_xx_2_Name.bin").is_none());
        assert!(parse_capture_file_name("001_in_Name.bin").is_none());
    }
}
