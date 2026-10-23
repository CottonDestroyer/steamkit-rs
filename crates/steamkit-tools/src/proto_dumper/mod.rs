//! Extracts protobuf definitions from Steam binaries, used by
//! `steamkit-proto-dump`.
//!
//! [`executable_scanner`] finds the embedded descriptors,
//! [`protobuf_collector`] parses them and [`protobuf_dumper`] writes them
//! back out as `.proto` source.

pub mod executable_scanner;
pub mod protobuf_collector;
pub mod protobuf_dumper;
pub mod raw_options;
pub mod util;
pub mod wire;

pub use executable_scanner::{scan_file, scan_path};
pub use protobuf_collector::{CandidateResult, ProtobufCollector};
pub use protobuf_dumper::ProtobufDumper;
pub use raw_options::{RawDescriptor, RawFileDescriptor, RawMessage};
pub use util::{to_literal, to_literal_bytes, to_literal_double, to_literal_float, OptionList};
