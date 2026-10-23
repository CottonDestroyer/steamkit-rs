//! Collects the protobuf descriptors found in a binary.

use prost::Message;
use prost_types::FileDescriptorProto;

use crate::proto_dumper::raw_options::RawDescriptor;
use crate::proto_dumper::wire;

/// The outcome of parsing a descriptor candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateResult {
    /// The candidate is a valid `FileDescriptorProto`.
    Ok,
    /// The candidate could not be parsed.
    Invalid,
}

/// Collects the descriptors that were found in a binary.
#[derive(Clone, Debug, Default)]
pub struct ProtobufCollector {
    /// Every descriptor that was parsed successfully.
    pub candidates: Vec<RawDescriptor>,
}

impl ProtobufCollector {
    /// Creates an empty collector.
    pub fn new() -> Self {
        ProtobufCollector {
            candidates: Vec::new(),
        }
    }

    /// Tries to parse the descriptor that starts at the beginning of `data`.
    ///
    /// Returns the outcome, the error message of an invalid candidate and the
    /// number of bytes the candidate occupies.
    pub fn try_parse_candidate(&mut self, data: &[u8]) -> (CandidateResult, Option<String>, usize) {
        let bytes_consumed = wire::consume_one_message(data);

        if bytes_consumed == 0 {
            return (
                CandidateResult::Invalid,
                Some("No data was consumed".to_owned()),
                0,
            );
        }

        match RawDescriptor::from_bytes(data[..bytes_consumed].to_vec()) {
            Ok(descriptor) => {
                self.candidates.push(descriptor);
                (CandidateResult::Ok, None, bytes_consumed)
            }
            Err(error) => (
                CandidateResult::Invalid,
                Some(error.to_string()),
                bytes_consumed,
            ),
        }
    }
}

/// Decodes a descriptor from its wire representation.
pub fn decode_descriptor(data: &[u8]) -> Result<FileDescriptorProto, prost::DecodeError> {
    FileDescriptorProto::decode(data)
}
