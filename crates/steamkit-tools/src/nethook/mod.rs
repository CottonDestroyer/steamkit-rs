//! Analyzer for NetHook2 dumps.
//!
//! NetHook2 is a Windows library that gets injected into the Steam client and
//! dumps its traffic into a directory of capture files. This module reads
//! such a dump and decodes the messages (no GUI).
//!
//! The capture file format is described in [`item`] and the message layout
//! in [`message_reading`].

pub mod body;
pub mod dump;
pub mod emsg_extensions;
pub mod error;
pub mod item;
pub mod message_reading;
pub mod message_type_finder;
pub mod message_type_overrides;
pub mod proto_registry;
pub mod protobuf_field_reader;
pub mod render;
pub mod serializable_registry;
pub mod specializations;
pub mod tree;
pub mod well_known_app_ids;

pub use body::{Body, ProtobufBody, ProtobufField, ProtobufValue, SerializableBody, WireType};
pub use dump::NetHookDump;
pub use error::{NetHookError, NetHookResult};
pub use item::{CaptureFileName, NetHookItem, PacketDirection};
pub use message_reading::{read_message, Header, ReadMessage};
pub use render::TreeNode;
pub use specializations::{
    default_specializations, ExtraObject, GameCoordinatorSpecialization, Specialization,
    SpecializationContext,
};
pub use tree::build_tree;
