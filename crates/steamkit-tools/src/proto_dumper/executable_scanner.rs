//! Finds the protobuf descriptors embedded in a binary.
//!
//! Steam's binaries contain their protobuf definitions as serialized
//! `FileDescriptorProto` messages. Each one starts with a length prefixed
//! file name, which is what the scanner looks for.

use std::path::Path;

use crate::proto_error::{ProtoError, ProtoResult};

/// The byte that marks the start of a length delimited protobuf field.
const MARKER_START: u8 = 0x0A;

/// Scans a file for embedded protobuf descriptors.
///
/// `process_candidate` is called with the name of the descriptor and the bytes
/// that follow it. It returns the number of bytes the descriptor occupies, or
/// `None` when the candidate is not a descriptor and the scan should continue.
pub fn scan_file<F>(data: &[u8], mut process_candidate: F) -> ProtoResult<()>
where
    F: FnMut(&str, &[u8]) -> Option<usize>,
{
    let mut index = 0usize;

    while data.len() >= 2 && index < data.len() - 1 {
        let found = match data[index..].iter().position(|byte| *byte == MARKER_START) {
            Some(offset) => index + offset,
            None => break,
        };

        index = found;
        if index >= data.len() - 1 {
            break;
        }

        let mut expected_length = u32::from(data[index + 1] & 0x7F);
        let mut marker_length = 2usize;

        if data[index + 1] & 0x80 != 0 {
            if index + 2 >= data.len() || data[index + 2] & 0x80 != 0 {
                index += 1;
                continue;
            }

            expected_length |= u32::from(data[index + 2]) << 7;
            marker_length = 3;
        }

        let expected_length = expected_length as usize;
        if index + marker_length + expected_length > data.len() {
            index += 1;
            continue;
        }

        let proto_name = decode_ascii(&data[index + marker_length..index + marker_length + expected_length]);

        if !proto_name.to_ascii_lowercase().ends_with(".proto") {
            index += 1;
            continue;
        }

        if !is_valid_proto_name(&proto_name) {
            println!("Skipping potentially valid '{proto_name}'");
            index += 1;
            continue;
        }

        match process_candidate(&proto_name, &data[index..]) {
            Some(bytes_consumed) => index += bytes_consumed.max(1),
            None => index += 1,
        }
    }

    Ok(())
}

/// Scans a file on disk for embedded protobuf descriptors.
pub fn scan_path<F>(file_name: &Path, process_candidate: F) -> ProtoResult<()>
where
    F: FnMut(&str, &[u8]) -> Option<usize>,
{
    let data = std::fs::read(file_name)
        .map_err(|error| ProtoError::io(file_name.display().to_string(), error))?;

    scan_file(&data, process_candidate)
}

/// Decodes ASCII, turning every byte above `0x7F` into `?`.
fn decode_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if *byte < 0x80 {
                *byte as char
            } else {
                '?'
            }
        })
        .collect()
}

/// True when the name matches the `^[a-zA-Z_0-9\\/.-]+\.proto$` pattern.
fn is_valid_proto_name(name: &str) -> bool {
    let Some(prefix) = name.strip_suffix(".proto") else {
        return false;
    };

    if prefix.is_empty() {
        return false;
    }

    prefix.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || byte == b'_'
            || byte == b'/'
            || byte == b'\\'
            || byte == b'.'
            || byte == b'-'
    })
}
