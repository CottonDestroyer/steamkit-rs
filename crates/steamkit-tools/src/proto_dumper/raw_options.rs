//! A raw view of a protobuf descriptor.
//!
//! `prost` drops unknown fields when decoding, and options declared as
//! extensions live exactly there. These types read the descriptor schema at
//! the wire level so those options can still be printed.

use crate::proto_dumper::wire::{parse_message, WireField, WireValue};
use crate::proto_error::{ProtoError, ProtoResult};
use prost::Message;
use prost_types::FileDescriptorProto;

/// A protobuf descriptor together with the bytes it was decoded from.
///
/// The bytes are needed because `prost` throws the fields it does not know
/// away, and those fields are exactly where the options that are declared as
/// extensions live.
#[derive(Clone, Debug)]
pub struct RawDescriptor {
    /// The decoded descriptor.
    pub proto: FileDescriptorProto,
    /// The bytes the descriptor was decoded from.
    pub bytes: Vec<u8>,
}

impl RawDescriptor {
    /// Decodes a descriptor from its wire representation.
    pub fn from_bytes(bytes: Vec<u8>) -> ProtoResult<RawDescriptor> {
        let proto = FileDescriptorProto::decode(bytes.as_slice()).map_err(|error| {
            ProtoError::Decode {
                message: error.to_string(),
            }
        })?;

        Ok(RawDescriptor { proto, bytes })
    }

    /// The raw view of the descriptor.
    pub fn raw(&self) -> RawFileDescriptor {
        RawFileDescriptor::new(RawMessage::parse(&self.bytes))
    }
}


/// The field numbers of `FileDescriptorProto`.
mod file_fields {
    /// `repeated DescriptorProto message_type`
    pub const MESSAGE_TYPE: u32 = 4;
    /// `repeated EnumDescriptorProto enum_type`
    pub const ENUM_TYPE: u32 = 5;
    /// `repeated ServiceDescriptorProto service`
    pub const SERVICE: u32 = 6;
    /// `repeated FieldDescriptorProto extension`
    pub const EXTENSION: u32 = 7;
    /// `FileOptions options`
    pub const OPTIONS: u32 = 8;
}

/// The field numbers of `DescriptorProto`.
mod message_fields {
    /// `repeated FieldDescriptorProto field`
    pub const FIELD: u32 = 2;
    /// `repeated DescriptorProto nested_type`
    pub const NESTED_TYPE: u32 = 3;
    /// `repeated EnumDescriptorProto enum_type`
    pub const ENUM_TYPE: u32 = 4;
    /// `repeated DescriptorProto.ExtensionRange extension_range`
    pub const EXTENSION_RANGE: u32 = 5;
    /// `repeated FieldDescriptorProto extension`
    pub const EXTENSION: u32 = 6;
    /// `MessageOptions options`
    pub const OPTIONS: u32 = 7;
    /// `repeated OneofDescriptorProto oneof_decl`
    pub const ONEOF_DECL: u32 = 8;
}

/// The field numbers of `FieldDescriptorProto`.
mod field_fields {
    /// `FieldOptions options`
    pub const OPTIONS: u32 = 8;
}

/// The field numbers of `EnumDescriptorProto`.
mod enum_fields {
    /// `repeated EnumValueDescriptorProto value`
    pub const VALUE: u32 = 2;
    /// `EnumOptions options`
    pub const OPTIONS: u32 = 3;
}

/// The field numbers of `EnumValueDescriptorProto`.
mod enum_value_fields {
    /// `EnumValueOptions options`
    pub const OPTIONS: u32 = 3;
}

/// The field numbers of `ServiceDescriptorProto`.
mod service_fields {
    /// `repeated MethodDescriptorProto method`
    pub const METHOD: u32 = 2;
    /// `ServiceOptions options`
    pub const OPTIONS: u32 = 3;
}

/// The field numbers of `MethodDescriptorProto`.
mod method_fields {
    /// `MethodOptions options`
    pub const OPTIONS: u32 = 4;
}

/// The field numbers of `OneofDescriptorProto`.
mod oneof_fields {
    /// `OneofOptions options`
    pub const OPTIONS: u32 = 2;
}

/// The field numbers of `DescriptorProto.ExtensionRange`.
mod extension_range_fields {
    /// `ExtensionRangeOptions options`
    pub const OPTIONS: u32 = 3;
}

/// The raw fields of a protobuf message.
#[derive(Clone, Debug, Default)]
pub struct RawMessage {
    fields: Vec<WireField>,
}

impl RawMessage {
    /// Parses a message; data that cannot be parsed yields an empty message.
    pub fn parse(data: &[u8]) -> RawMessage {
        RawMessage {
            fields: parse_message(data).unwrap_or_default(),
        }
    }

    /// The first value of the given field.
    pub fn get(&self, number: u32) -> Option<&WireValue> {
        self.fields
            .iter()
            .find(|field| field.number == number)
            .map(|field| &field.value)
    }

    /// Every value of the given field, in the order they appeared.
    pub fn values(&self, number: u32) -> Vec<&WireValue> {
        self.fields
            .iter()
            .filter(|field| field.number == number)
            .map(|field| &field.value)
            .collect()
    }

    /// The first length delimited value of the given field.
    pub fn bytes(&self, number: u32) -> Option<&[u8]> {
        self.get(number).and_then(WireValue::as_bytes)
    }

    /// The first length delimited value of the given field, parsed as a message.
    pub fn message(&self, number: u32) -> Option<RawMessage> {
        self.bytes(number).map(RawMessage::parse)
    }

    /// Every length delimited value of the given field, parsed as messages.
    pub fn messages(&self, number: u32) -> Vec<RawMessage> {
        self.values(number)
            .into_iter()
            .filter_map(WireValue::as_bytes)
            .map(RawMessage::parse)
            .collect()
    }
}

/// The raw view of a `FileDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawFileDescriptor {
    inner: RawMessage,
}

impl RawFileDescriptor {
    /// Creates the raw view of a file descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawFileDescriptor { inner }
    }

    /// The options of the file.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(file_fields::OPTIONS)
    }

    /// The messages of the file.
    pub fn messages(&self) -> Vec<RawMessageDescriptor> {
        self.inner
            .messages(file_fields::MESSAGE_TYPE)
            .into_iter()
            .map(RawMessageDescriptor::new)
            .collect()
    }

    /// The enums of the file.
    pub fn enums(&self) -> Vec<RawEnumDescriptor> {
        self.inner
            .messages(file_fields::ENUM_TYPE)
            .into_iter()
            .map(RawEnumDescriptor::new)
            .collect()
    }

    /// The services of the file.
    pub fn services(&self) -> Vec<RawServiceDescriptor> {
        self.inner
            .messages(file_fields::SERVICE)
            .into_iter()
            .map(RawServiceDescriptor::new)
            .collect()
    }

    /// The top level extensions of the file.
    pub fn extensions(&self) -> Vec<RawFieldDescriptor> {
        self.inner
            .messages(file_fields::EXTENSION)
            .into_iter()
            .map(RawFieldDescriptor::new)
            .collect()
    }
}

/// The raw view of a `DescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawMessageDescriptor {
    inner: RawMessage,
}

impl RawMessageDescriptor {
    /// Creates the raw view of a message descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawMessageDescriptor { inner }
    }

    /// The options of the message.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(message_fields::OPTIONS)
    }

    /// The fields of the message.
    pub fn fields(&self) -> Vec<RawFieldDescriptor> {
        self.inner
            .messages(message_fields::FIELD)
            .into_iter()
            .map(RawFieldDescriptor::new)
            .collect()
    }

    /// The nested messages of the message.
    pub fn nested_types(&self) -> Vec<RawMessageDescriptor> {
        self.inner
            .messages(message_fields::NESTED_TYPE)
            .into_iter()
            .map(RawMessageDescriptor::new)
            .collect()
    }

    /// The nested enums of the message.
    pub fn enums(&self) -> Vec<RawEnumDescriptor> {
        self.inner
            .messages(message_fields::ENUM_TYPE)
            .into_iter()
            .map(RawEnumDescriptor::new)
            .collect()
    }

    /// The extensions declared inside the message.
    pub fn extensions(&self) -> Vec<RawFieldDescriptor> {
        self.inner
            .messages(message_fields::EXTENSION)
            .into_iter()
            .map(RawFieldDescriptor::new)
            .collect()
    }

    /// The extension ranges of the message.
    pub fn extension_ranges(&self) -> Vec<RawExtensionRange> {
        self.inner
            .messages(message_fields::EXTENSION_RANGE)
            .into_iter()
            .map(RawExtensionRange::new)
            .collect()
    }

    /// The oneofs of the message.
    pub fn oneofs(&self) -> Vec<RawOneofDescriptor> {
        self.inner
            .messages(message_fields::ONEOF_DECL)
            .into_iter()
            .map(RawOneofDescriptor::new)
            .collect()
    }
}

/// The raw view of a `FieldDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawFieldDescriptor {
    inner: RawMessage,
}

impl RawFieldDescriptor {
    /// Creates the raw view of a field descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawFieldDescriptor { inner }
    }

    /// The options of the field.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(field_fields::OPTIONS)
    }
}

/// The raw view of an `EnumDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawEnumDescriptor {
    inner: RawMessage,
}

impl RawEnumDescriptor {
    /// Creates the raw view of an enum descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawEnumDescriptor { inner }
    }

    /// The options of the enum.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(enum_fields::OPTIONS)
    }

    /// The values of the enum.
    pub fn values(&self) -> Vec<RawEnumValueDescriptor> {
        self.inner
            .messages(enum_fields::VALUE)
            .into_iter()
            .map(RawEnumValueDescriptor::new)
            .collect()
    }
}

/// The raw view of an `EnumValueDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawEnumValueDescriptor {
    inner: RawMessage,
}

impl RawEnumValueDescriptor {
    /// Creates the raw view of an enum value descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawEnumValueDescriptor { inner }
    }

    /// The options of the enum value.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(enum_value_fields::OPTIONS)
    }
}

/// The raw view of a `ServiceDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawServiceDescriptor {
    inner: RawMessage,
}

impl RawServiceDescriptor {
    /// Creates the raw view of a service descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawServiceDescriptor { inner }
    }

    /// The options of the service.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(service_fields::OPTIONS)
    }

    /// The methods of the service.
    pub fn methods(&self) -> Vec<RawMethodDescriptor> {
        self.inner
            .messages(service_fields::METHOD)
            .into_iter()
            .map(RawMethodDescriptor::new)
            .collect()
    }
}

/// The raw view of a `MethodDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawMethodDescriptor {
    inner: RawMessage,
}

impl RawMethodDescriptor {
    /// Creates the raw view of a method descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawMethodDescriptor { inner }
    }

    /// The options of the method.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(method_fields::OPTIONS)
    }
}

/// The raw view of a `OneofDescriptorProto`.
#[derive(Clone, Debug, Default)]
pub struct RawOneofDescriptor {
    inner: RawMessage,
}

impl RawOneofDescriptor {
    /// Creates the raw view of a oneof descriptor.
    pub fn new(inner: RawMessage) -> Self {
        RawOneofDescriptor { inner }
    }

    /// The options of the oneof.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(oneof_fields::OPTIONS)
    }
}

/// The raw view of a `DescriptorProto.ExtensionRange`.
#[derive(Clone, Debug, Default)]
pub struct RawExtensionRange {
    inner: RawMessage,
}

impl RawExtensionRange {
    /// Creates the raw view of an extension range.
    pub fn new(inner: RawMessage) -> Self {
        RawExtensionRange { inner }
    }

    /// The options of the extension range.
    pub fn options(&self) -> Option<RawMessage> {
        self.inner.message(extension_range_fields::OPTIONS)
    }
}
