//! A minimal protobuf wire format reader.
//!
//! `prost` drops the fields it doesn't know when decoding, but options
//! declared as extensions live in exactly those fields. This reads the wire
//! format directly so they can still be recovered.

use crate::proto_error::{ProtoError, ProtoResult};

/// The value of a single protobuf field.
#[derive(Clone, Debug, PartialEq)]
pub enum WireValue {
    /// A base 128 varint.
    Varint(u64),
    /// A little endian 32 bit value.
    Fixed32(u32),
    /// A little endian 64 bit value.
    Fixed64(u64),
    /// A length delimited value.
    Bytes(Vec<u8>),
}

impl WireValue {
    /// The varint payload, if the field holds one.
    pub fn as_varint(&self) -> Option<u64> {
        match self {
            WireValue::Varint(value) => Some(*value),
            _ => None,
        }
    }

    /// The 32 bit payload, if the field holds one.
    pub fn as_fixed32(&self) -> Option<u32> {
        match self {
            WireValue::Fixed32(value) => Some(*value),
            _ => None,
        }
    }

    /// The 64 bit payload, if the field holds one.
    pub fn as_fixed64(&self) -> Option<u64> {
        match self {
            WireValue::Fixed64(value) => Some(*value),
            _ => None,
        }
    }

    /// The length delimited payload, if the field holds one.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            WireValue::Bytes(value) => Some(value.as_slice()),
            _ => None,
        }
    }
}

/// A single field of a protobuf message.
#[derive(Clone, Debug, PartialEq)]
pub struct WireField {
    /// The field number.
    pub number: u32,
    /// The value of the field.
    pub value: WireValue,
}

/// Parses every field of a protobuf message.
pub fn parse_message(data: &[u8]) -> ProtoResult<Vec<WireField>> {
    let mut fields = Vec::new();
    let mut position = 0usize;

    while position < data.len() {
        let (key, next) = read_varint(data, position)?;
        position = next;

        let number = (key >> 3) as u32;
        let wire_type = (key & 0x07) as u8;

        if number == 0 {
            return Err(ProtoError::Decode {
                message: "field number 0 is not valid".to_owned(),
            });
        }

        let value = match wire_type {
            0 => {
                let (value, next) = read_varint(data, position)?;
                position = next;
                WireValue::Varint(value)
            }
            1 => {
                if position + 8 > data.len() {
                    return Err(ProtoError::Decode {
                        message: "truncated 64 bit value".to_owned(),
                    });
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[position..position + 8]);
                position += 8;
                WireValue::Fixed64(u64::from_le_bytes(bytes))
            }
            2 => {
                let (length, next) = read_varint(data, position)?;
                position = next;
                let length = length as usize;
                if position + length > data.len() {
                    return Err(ProtoError::Decode {
                        message: "truncated length delimited value".to_owned(),
                    });
                }
                let value = data[position..position + length].to_vec();
                position += length;
                WireValue::Bytes(value)
            }
            5 => {
                if position + 4 > data.len() {
                    return Err(ProtoError::Decode {
                        message: "truncated 32 bit value".to_owned(),
                    });
                }
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(&data[position..position + 4]);
                position += 4;
                WireValue::Fixed32(u32::from_le_bytes(bytes))
            }
            other => {
                return Err(ProtoError::Decode {
                    message: format!("unsupported wire type {other}"),
                })
            }
        };

        fields.push(WireField { number, value });
    }

    Ok(fields)
}

/// Returns the length of the first protobuf message in `data`.
///
/// Walks the fields and stops at the second `name` field (field 1), which is
/// where the next concatenated `FileDescriptorProto` starts. Also stops at
/// field 0 or anything that doesn't parse.
pub fn consume_one_message(data: &[u8]) -> usize {
    let mut position = 0usize;
    let mut consumed_field_one = false;

    loop {
        let before_field = position;

        let (key, next) = match read_varint(data, position) {
            Ok(value) => value,
            Err(_) => return before_field,
        };

        let number = (key >> 3) as u32;
        let wire_type = (key & 0x07) as u8;

        if number == 0 {
            return before_field;
        }

        if number == 1 {
            if consumed_field_one {
                return before_field;
            }
            consumed_field_one = true;
        }

        position = match skip_field(data, next, wire_type) {
            Ok(value) => value,
            Err(_) => return before_field,
        };
    }
}

/// Reads a base 128 varint.
fn read_varint(data: &[u8], position: usize) -> ProtoResult<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut position = position;

    loop {
        if position >= data.len() {
            return Err(ProtoError::Decode {
                message: "truncated varint".to_owned(),
            });
        }

        if shift >= 64 {
            return Err(ProtoError::Decode {
                message: "varint is too long".to_owned(),
            });
        }

        let byte = data[position];
        position += 1;

        value |= u64::from(byte & 0x7F) << shift;

        if byte & 0x80 == 0 {
            return Ok((value, position));
        }

        shift += 7;
    }
}

/// Skips a field and returns the position behind it.
fn skip_field(data: &[u8], position: usize, wire_type: u8) -> ProtoResult<usize> {
    match wire_type {
        0 => {
            let (_, next) = read_varint(data, position)?;
            Ok(next)
        }
        1 => {
            if position + 8 > data.len() {
                return Err(ProtoError::Decode {
                    message: "truncated 64 bit value".to_owned(),
                });
            }
            Ok(position + 8)
        }
        2 => {
            let (length, next) = read_varint(data, position)?;
            let length = length as usize;
            if next + length > data.len() {
                return Err(ProtoError::Decode {
                    message: "truncated length delimited value".to_owned(),
                });
            }
            Ok(next + length)
        }
        5 => {
            if position + 4 > data.len() {
                return Err(ProtoError::Decode {
                    message: "truncated 32 bit value".to_owned(),
                });
            }
            Ok(position + 4)
        }
        other => Err(ProtoError::Decode {
            message: format!("unsupported wire type {other}"),
        }),
    }
}
