//! The decoded bodies of captured Steam messages.

use std::fmt;

/// A protobuf message body.
///
/// Rust has no runtime reflection, so the decoded message is kept behind this
/// trait. Code that needs a concrete type decodes the bytes again.
pub trait ProtobufBody: fmt::Debug + Send + Sync {}

impl<T> ProtobufBody for T where T: fmt::Debug + Send + Sync {}

/// A message body of the SteamKit wire format that is not protobuf backed.
pub trait SerializableBody: fmt::Debug + Send + Sync {}

impl<T> SerializableBody for T where T: fmt::Debug + Send + Sync {}

/// The wire type of a protobuf field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireType {
    /// A variable length integer.
    Varint,
    /// A fixed 64 bit value.
    Fixed64,
    /// A length delimited value.
    LengthDelimited,
    /// The start of a group; Steam does not use groups.
    StartGroup,
    /// The end of a group; Steam does not use groups.
    EndGroup,
    /// A fixed 32 bit value.
    Fixed32,
}

impl WireType {
    /// The display name of the wire type. Length-delimited shows as `String`.
    pub fn as_str(self) -> &'static str {
        match self {
            WireType::Varint => "Varint",
            WireType::Fixed64 => "Fixed64",
            WireType::LengthDelimited => "String",
            WireType::StartGroup => "StartGroup",
            WireType::EndGroup => "EndGroup",
            WireType::Fixed32 => "Fixed32",
        }
    }
}

/// A single value of a protobuf field.
#[derive(Clone, Debug, PartialEq)]
pub enum ProtobufValue {
    /// A variable length integer.
    Varint(u64),
    /// A fixed 64 bit value.
    Fixed64(u64),
    /// A fixed 32 bit value.
    Fixed32(u32),
    /// A length delimited value.
    Bytes(Vec<u8>),
    /// A group. Groups aren't decoded.
    Group,
}

impl fmt::Display for ProtobufValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtobufValue::Varint(value) => write!(f, "{value}"),
            ProtobufValue::Fixed64(value) => write!(f, "{value}"),
            ProtobufValue::Fixed32(value) => write!(f, "{value}"),
            ProtobufValue::Bytes(bytes) => {
                write!(f, "\"{}\"", String::from_utf8_lossy(bytes))
            }
            ProtobufValue::Group => write!(f, "StartGroup is not implemented"),
        }
    }
}

/// A protobuf field of a message whose type could not be determined.
///
/// Each field number maps to every value seen for it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProtobufField {
    /// The number of the field.
    pub number: u32,
    /// The wire type of the field.
    pub wire_type: WireType,
    /// The values of the field; a field may occur more than once.
    pub values: Vec<ProtobufValue>,
}

/// The decoded body of a captured message.
#[derive(Debug)]
pub enum Body {
    /// A protobuf message whose type the analyzer knows.
    Protobuf {
        /// The name of the protobuf type.
        type_name: String,
        /// The bytes of the message.
        bytes: Vec<u8>,
        /// The decoded message.
        message: Box<dyn ProtobufBody>,
    },

    /// A protobuf message whose type the analyzer does not know.
    Fields(Vec<ProtobufField>),

    /// A message of the SteamKit wire format that is not protobuf backed.
    Serializable {
        /// The name of the message type.
        type_name: String,
        /// The decoded message.
        message: Box<dyn SerializableBody>,
    },

    /// The body could not be decoded.
    Unknown(String),
}

impl Body {
    /// The name of the protobuf type of the body.
    pub fn protobuf_type_name(&self) -> Option<&str> {
        match self {
            Body::Protobuf { type_name, .. } => Some(type_name),
            _ => None,
        }
    }

    /// The bytes of the protobuf body.
    pub fn protobuf_bytes(&self) -> Option<&[u8]> {
        match self {
            Body::Protobuf { bytes, .. } => Some(bytes),
            _ => None,
        }
    }

    /// Decodes the protobuf body as a concrete message type.
    ///
    /// Returns `None` when the body is not a protobuf message or when the
    /// bytes do not decode into `T`.
    pub fn decode_as<T: prost::Message + Default>(&self) -> Option<T> {
        let bytes = self.protobuf_bytes()?;
        T::decode(bytes).ok()
    }

    /// The name of the type of the body.
    pub fn type_name(&self) -> Option<&str> {
        match self {
            Body::Protobuf { type_name, .. } => Some(type_name),
            Body::Serializable { type_name, .. } => Some(type_name),
            Body::Fields(_) => Some("Protobuf"),
            Body::Unknown(_) => None,
        }
    }
}
