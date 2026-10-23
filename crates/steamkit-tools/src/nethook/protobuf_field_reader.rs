//! Generic protobuf reader, used when the message type isn't known.
//!
//! Reads the field headers and collects every field's values. Steam doesn't
//! length-prefix the message, so it reads until the end of the buffer.

use crate::nethook::body::{ProtobufField, ProtobufValue, WireType};
use crate::nethook::error::{NetHookError, NetHookResult};

/// Reads a variable length integer.
fn read_varint(bytes: &[u8], offset: &mut usize) -> NetHookResult<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;

    loop {
        if *offset >= bytes.len() {
            return Err(NetHookError::malformed("truncated protobuf varint"));
        }
        if shift >= 64 {
            return Err(NetHookError::malformed("protobuf varint is too long"));
        }

        let byte = bytes[*offset];
        *offset += 1;
        value |= u64::from(byte & 0x7f) << shift;

        if byte & 0x80 == 0 {
            return Ok(value);
        }

        shift += 7;
    }
}

/// Advances the offset by `count` bytes.
fn take(bytes: &[u8], offset: usize, count: usize) -> NetHookResult<usize> {
    let end = offset
        .checked_add(count)
        .ok_or_else(|| NetHookError::malformed("protobuf length overflow"))?;

    if end > bytes.len() {
        return Err(NetHookError::malformed("truncated protobuf message"));
    }

    Ok(end)
}

/// Skips a protobuf group.
fn skip_group(bytes: &[u8], offset: &mut usize) -> NetHookResult<()> {
    let mut depth = 1usize;

    while depth > 0 {
        if *offset >= bytes.len() {
            return Err(NetHookError::malformed("truncated protobuf group"));
        }

        let tag = read_varint(bytes, offset)?;
        match tag & 0x7 {
            0 => {
                read_varint(bytes, offset)?;
            }
            1 => {
                *offset = take(bytes, *offset, 8)?;
            }
            2 => {
                let length = read_varint(bytes, offset)? as usize;
                *offset = take(bytes, *offset, length)?;
            }
            3 => {
                depth += 1;
            }
            4 => {
                depth -= 1;
            }
            5 => {
                *offset = take(bytes, *offset, 4)?;
            }
            other => {
                return Err(NetHookError::malformed(format!(
                    "unknown protobuf wire type {other}"
                )));
            }
        }
    }

    Ok(())
}

/// Reads the fields of a protobuf message.
///
/// Values are collected per field in the order they appear.
pub fn read_protobuf(bytes: &[u8]) -> NetHookResult<Vec<ProtobufField>> {
    let mut fields: Vec<ProtobufField> = Vec::new();
    let mut offset = 0usize;

    while offset < bytes.len() {
        let tag = read_varint(bytes, &mut offset)?;
        let field_number = (tag >> 3) as u32;
        let wire_type = match tag & 0x7 {
            0 => WireType::Varint,
            1 => WireType::Fixed64,
            2 => WireType::LengthDelimited,
            3 => WireType::StartGroup,
            4 => WireType::EndGroup,
            5 => WireType::Fixed32,
            other => {
                return Err(NetHookError::malformed(format!(
                    "unknown protobuf wire type {other}"
                )));
            }
        };

        let value = match wire_type {
            WireType::Varint => ProtobufValue::Varint(read_varint(bytes, &mut offset)?),
            WireType::Fixed64 => {
                let end = take(bytes, offset, 8)?;
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&bytes[offset..end]);
                offset = end;
                ProtobufValue::Fixed64(u64::from_le_bytes(raw))
            }
            WireType::Fixed32 => {
                let end = take(bytes, offset, 4)?;
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&bytes[offset..end]);
                offset = end;
                ProtobufValue::Fixed32(u32::from_le_bytes(raw))
            }
            WireType::LengthDelimited => {
                let length = read_varint(bytes, &mut offset)? as usize;
                let end = take(bytes, offset, length)?;
                let value = ProtobufValue::Bytes(bytes[offset..end].to_vec());
                offset = end;
                value
            }
            WireType::StartGroup => {
                skip_group(bytes, &mut offset)?;
                ProtobufValue::Group
            }
            WireType::EndGroup => {
                return Err(NetHookError::malformed("unexpected protobuf end group"));
            }
        };

        match fields.iter_mut().find(|field| field.number == field_number) {
            Some(field) => field.values.push(value),
            None => fields.push(ProtobufField {
                number: field_number,
                wire_type,
                values: vec![value],
            }),
        }
    }

    Ok(fields)
}
