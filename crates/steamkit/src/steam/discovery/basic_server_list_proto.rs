//! The on-disk format of a cached server list.
//!
//! It's a sequence of length-prefixed protobuf messages, the same as SteamKit2
//! writes, so the cache files are interchangeable:
//!
//! ```text
//! file  := item*
//! item  := 0x0A varint(len) payload      // field 1, length delimited
//! payload:= field1 address | field2 port | field3 protocols
//! field1 := 0x0A varint(len) utf8-bytes  // string
//! field2 := 0x10 varint(port)            // int32
//! field3 := 0x18 varint(protocols)       // enum
//! ```
//!
//! Fields holding their default value (empty address, port 0, protocols 0) are
//! left out.

use crate::error::SteamError;
use crate::networking::steam3::protocol_type::ProtocolTypes;

use super::server_record::{tcp_udp_protocol_types, ServerRecord};

/// One cached server list entry.
#[derive(Clone, Debug, Default)]
pub(crate) struct BasicServerListProto {
    /// The address of the server (an IP address or a DNS host name).
    pub(crate) address: String,
    /// The port of the server.
    pub(crate) port: u16,
    /// The protocol types the server supports. If missing, `Tcp | Udp` is assumed.
    pub(crate) protocols: Option<ProtocolTypes>,
}

impl BasicServerListProto {
    /// Creates a payload for the given address, port and protocol types.
    pub(crate) fn new(address: String, port: u16, protocols: ProtocolTypes) -> Self {
        BasicServerListProto {
            address,
            port,
            protocols: Some(protocols),
        }
    }

    /// The address of the server.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// The port of the server.
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// The protocol types the server supports; `Tcp | Udp` if the value was
    /// missing.
    pub(crate) fn protocols(&self) -> ProtocolTypes {
        self.protocols.unwrap_or_else(tcp_udp_protocol_types)
    }

    /// Creates a payload from a server record.
    pub(crate) fn from_server_record(record: &ServerRecord) -> Self {
        BasicServerListProto {
            address: record.get_host(),
            port: record.get_port(),
            protocols: Some(record.protocol_types()),
        }
    }

    /// Converts back to a server record, resolving a DNS address.
    pub(crate) async fn to_server_record(&self) -> Result<ServerRecord, SteamError> {
        ServerRecord::create_server(&self.address, self.port, self.protocols()).await
    }

    /// Encodes the entry as a protobuf message.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();

        if !self.address.is_empty() {
            write_tag(&mut out, 1, WireType::LengthDelimited);
            write_varint(&mut out, self.address.len() as u64);
            out.extend_from_slice(self.address.as_bytes());
        }

        if self.port != 0 {
            write_tag(&mut out, 2, WireType::Varint);
            write_varint(&mut out, u64::from(self.port));
        }

        if let Some(protocols) = self.protocols {
            if protocols.raw() != 0 {
                write_tag(&mut out, 3, WireType::Varint);
                write_varint(&mut out, u64::from(protocols.raw()));
            }
        }

        out
    }

    /// Decodes an entry from a protobuf message.
    pub(crate) fn decode(data: &[u8]) -> Result<Self, SteamError> {
        let mut result = BasicServerListProto::default();
        let mut cursor = Cursor::new(data);

        while !cursor.is_empty() {
            let tag = cursor.read_varint()?;
            let field = tag >> 3;
            let wire_type = WireType::from_tag(tag)?;

            match (field, wire_type) {
                (1, WireType::LengthDelimited) => {
                    let length = cursor.read_varint()? as usize;
                    let bytes = cursor.read_bytes(length)?;
                    result.address = String::from_utf8(bytes.to_vec()).map_err(|error| {
                        SteamError::Malformed(format!("Invalid UTF-8 in server address: {error}"))
                    })?;
                }
                (2, WireType::Varint) => {
                    result.port = cursor.read_varint()? as u16;
                }
                (3, WireType::Varint) => {
                    result.protocols = Some(ProtocolTypes(cursor.read_varint()? as u32));
                }
                _ => cursor.skip(wire_type)?,
            }
        }

        Ok(result)
    }

    /// Encodes a list of entries in the file format above.
    pub(crate) fn encode_list(items: &[BasicServerListProto]) -> Vec<u8> {
        let mut out = Vec::new();

        for item in items {
            let payload = item.encode();
            write_tag(&mut out, 1, WireType::LengthDelimited);
            write_varint(&mut out, payload.len() as u64);
            out.extend_from_slice(&payload);
        }

        out
    }

    /// Decodes a list of entries in the file format above.
    pub(crate) fn decode_list(data: &[u8]) -> Result<Vec<Self>, SteamError> {
        let mut result = Vec::new();
        let mut cursor = Cursor::new(data);

        while !cursor.is_empty() {
            let tag = cursor.read_varint()?;
            let field = tag >> 3;
            let wire_type = WireType::from_tag(tag)?;

            if field == 1 && wire_type == WireType::LengthDelimited {
                let length = cursor.read_varint()? as usize;
                let bytes = cursor.read_bytes(length)?;
                result.push(BasicServerListProto::decode(bytes)?);
            } else {
                cursor.skip(wire_type)?;
            }
        }

        Ok(result)
    }
}

/// The protobuf wire types used here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WireType {
    Varint,
    Fixed64,
    LengthDelimited,
    StartGroup,
    EndGroup,
    Fixed32,
}

impl WireType {
    fn from_tag(tag: u64) -> Result<Self, SteamError> {
        match tag & 0x07 {
            0 => Ok(WireType::Varint),
            1 => Ok(WireType::Fixed64),
            2 => Ok(WireType::LengthDelimited),
            3 => Ok(WireType::StartGroup),
            4 => Ok(WireType::EndGroup),
            5 => Ok(WireType::Fixed32),
            other => Err(SteamError::Malformed(format!(
                "Unknown protobuf wire type {other}"
            ))),
        }
    }

    fn skip_size(self) -> Option<usize> {
        match self {
            WireType::Fixed64 => Some(8),
            WireType::Fixed32 => Some(4),
            _ => None,
        }
    }
}

/// Writes a protobuf tag byte.
fn write_tag(out: &mut Vec<u8>, field: u64, wire_type: WireType) {
    let wire = match wire_type {
        WireType::Varint => 0u64,
        WireType::Fixed64 => 1,
        WireType::LengthDelimited => 2,
        WireType::StartGroup => 3,
        WireType::EndGroup => 4,
        WireType::Fixed32 => 5,
    };

    write_varint(out, (field << 3) | wire);
}

/// Writes a base-128 varint.
fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// A minimal protobuf reader.
struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position >= self.data.len()
    }

    fn read_varint(&mut self) -> Result<u64, SteamError> {
        let mut result: u64 = 0;
        let mut shift = 0;

        loop {
            if self.position >= self.data.len() {
                return Err(SteamError::Malformed(
                    "Unexpected end of protobuf data".to_string(),
                ));
            }

            let byte = self.data[self.position];
            self.position += 1;

            result |= u64::from(byte & 0x7F) << shift;

            if byte & 0x80 == 0 {
                break;
            }

            shift += 7;
            if shift > 63 {
                return Err(SteamError::Malformed(
                    "Protobuf varint is too long".to_string(),
                ));
            }
        }

        Ok(result)
    }

    fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], SteamError> {
        let end = self.position.checked_add(length).ok_or_else(|| {
            SteamError::Malformed("Protobuf length overflow".to_string())
        })?;

        if end > self.data.len() {
            return Err(SteamError::Malformed(
                "Unexpected end of protobuf data".to_string(),
            ));
        }

        let bytes = &self.data[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn skip(&mut self, wire_type: WireType) -> Result<(), SteamError> {
        match wire_type {
            WireType::Varint => {
                self.read_varint()?;
            }
            WireType::LengthDelimited => {
                let length = self.read_varint()? as usize;
                self.read_bytes(length)?;
            }
            other => {
                let size = other.skip_size().ok_or_else(|| {
                    SteamError::Malformed(format!(
                        "Unsupported protobuf wire type in server list: {other:?}"
                    ))
                })?;
                self.read_bytes(size)?;
            }
        }

        Ok(())
    }
}
