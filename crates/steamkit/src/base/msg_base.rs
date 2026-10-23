//! The payload buffer of a client message, plus a small slice reader.
//!
//! * [`MsgBase`] owns its bytes and has a read/write cursor.
//! * [`Reader`] is a borrowing cursor over a slice. The generated
//!   [`SteamSerializable`](crate::base::generated::steam_language_internal::SteamSerializable)
//!   types read through it.

use crate::error::SteamError;

/// The reference point for [`MsgBase::seek_from`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SeekOrigin {
    /// The beginning of the stream.
    Begin,
    /// The current position of the stream.
    Current,
    /// The end of the stream.
    End,
}

/// A forward-only reader over a byte slice. Integers are little endian.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader positioned at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// The number of bytes consumed so far.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The number of bytes left to read.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    /// Whether the reader has consumed the whole slice.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Reads exactly `count` bytes and advances the cursor.
    pub fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>, SteamError> {
        let end = self.position.checked_add(count).ok_or_else(|| {
            SteamError::Malformed(format!("read length overflow at offset {}", self.position))
        })?;
        if end > self.data.len() {
            return Err(SteamError::Malformed(format!(
                "attempted to read {count} bytes at offset {}, but only {} remain",
                self.position,
                self.remaining()
            )));
        }
        let slice = self.data[self.position..end].to_vec();
        self.position = end;
        Ok(slice)
    }

    /// Reads up to `count` bytes and advances the cursor. Returns fewer bytes
    /// if the data runs out instead of failing.
    pub fn read_up_to(&mut self, count: usize) -> Vec<u8> {
        let start = self.position.min(self.data.len());
        let end = start.saturating_add(count).min(self.data.len());
        self.position = end;
        self.data[start..end].to_vec()
    }

    /// Reads a single unsigned byte.
    pub fn read_u8(&mut self) -> Result<u8, SteamError> {
        Ok(self.read_bytes(1)?[0])
    }

    /// Reads a single signed byte.
    pub fn read_i8(&mut self) -> Result<i8, SteamError> {
        Ok(self.read_u8()? as i8)
    }

    /// Reads a single unsigned 16 bit short.
    pub fn read_u16(&mut self) -> Result<u16, SteamError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a single signed 16 bit short.
    pub fn read_i16(&mut self) -> Result<i16, SteamError> {
        Ok(self.read_u16()? as i16)
    }

    /// Reads a single unsigned 32 bit integer.
    pub fn read_u32(&mut self) -> Result<u32, SteamError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a single signed 32 bit integer.
    pub fn read_i32(&mut self) -> Result<i32, SteamError> {
        Ok(self.read_u32()? as i32)
    }

    /// Reads a single unsigned 64 bit integer.
    pub fn read_u64(&mut self) -> Result<u64, SteamError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a single signed 64 bit integer.
    pub fn read_i64(&mut self) -> Result<i64, SteamError> {
        Ok(self.read_u64()? as i64)
    }

    /// Reads a single 32 bit float.
    pub fn read_f32(&mut self) -> Result<f32, SteamError> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    /// Reads a single 64 bit double.
    pub fn read_f64(&mut self) -> Result<f64, SteamError> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    /// Reads a null-terminated UTF-8 string and consumes the terminator. If
    /// there's no terminator, reads to the end of the data.
    pub fn read_null_term_string(&mut self) -> Result<String, SteamError> {
        let start = self.position;
        while self.position < self.data.len() && self.data[self.position] != 0 {
            self.position += 1;
        }
        let bytes = &self.data[start..self.position];
        if self.position < self.data.len() {
            // consume the null terminator
            self.position += 1;
        }
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The payload of a client message: a byte buffer with a cursor.
#[derive(Clone, Debug, Default)]
pub struct MsgBase {
    /// The raw bytes of the message payload.
    pub payload: Vec<u8>,
    /// The read/write cursor into [`MsgBase::payload`].
    pub position: u64,
}

impl MsgBase {
    /// Creates an empty payload with the given initial capacity.
    pub fn new(payload_reserve: usize) -> Self {
        Self {
            payload: Vec::with_capacity(payload_reserve),
            position: 0,
        }
    }

    /// Creates a payload pre-filled with `data`, positioned at the start.
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            payload: data.to_vec(),
            position: 0,
        }
    }

    /// Seeks to `offset` from the beginning of the payload.
    ///
    /// Returns the new position.
    pub fn seek(&mut self, offset: u64) -> u64 {
        self.position = offset;
        self.position
    }

    /// Seeks to `offset` relative to `origin` and returns the new position.
    pub fn seek_from(&mut self, offset: u64, origin: SeekOrigin) -> u64 {
        let base = match origin {
            SeekOrigin::Begin => 0u64,
            SeekOrigin::Current => self.position,
            SeekOrigin::End => self.payload.len() as u64,
        };
        self.position = base.saturating_add(offset);
        self.position
    }

    /// The number of bytes held in the payload.
    pub fn len(&self) -> usize {
        self.payload.len()
    }

    /// Whether the payload holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    /// A copy of the whole payload, not just the part after the cursor.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.payload.clone()
    }

    /// A borrowing reader over the part of the payload after the cursor.
    pub fn reader(&self) -> Reader<'_> {
        let start = (self.position as usize).min(self.payload.len());
        Reader::new(&self.payload[start..])
    }

    /// Writes a single unsigned byte.
    pub fn write_u8(&mut self, v: u8) {
        self.write_bytes(&[v]);
    }

    /// Writes a single signed byte.
    pub fn write_i8(&mut self, v: i8) {
        self.write_bytes(&[v as u8]);
    }

    /// Writes a single unsigned 16 bit short.
    pub fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single signed 16 bit short.
    pub fn write_i16(&mut self, v: i16) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single unsigned 32 bit integer.
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single signed 32 bit integer.
    pub fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single unsigned 64 bit integer.
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single signed 64 bit integer.
    pub fn write_i64(&mut self, v: i64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single 32 bit float.
    pub fn write_f32(&mut self, v: f32) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes a single 64 bit double.
    pub fn write_f64(&mut self, v: f64) {
        self.write_bytes(&v.to_le_bytes());
    }

    /// Writes `v` at the cursor and advances it. Writing past the end
    /// zero-fills the gap.
    pub fn write_bytes(&mut self, v: &[u8]) {
        let start = self.position as usize;
        let end = start.saturating_add(v.len());
        if end > self.payload.len() {
            self.payload.resize(end, 0);
        }
        self.payload[start..end].copy_from_slice(v);
        self.position = end as u64;
    }

    /// Writes the UTF-8 bytes of `v`, without a terminator.
    pub fn write_string(&mut self, v: &str) {
        self.write_bytes(v.as_bytes());
    }

    /// Writes the UTF-8 bytes of `v` followed by a null terminator.
    pub fn write_null_term_string(&mut self, v: &str) {
        self.write_bytes(v.as_bytes());
        self.write_u8(0);
    }

    /// Reads exactly `count` bytes and advances the cursor.
    pub fn read_exact(&mut self, count: usize) -> Result<Vec<u8>, SteamError> {
        let start = self.position as usize;
        let end = start.checked_add(count).ok_or_else(|| {
            SteamError::Malformed(format!("read length overflow at offset {}", start))
        })?;
        if end > self.payload.len() {
            return Err(SteamError::Malformed(format!(
                "attempted to read {count} bytes at offset {start}, but only {} remain",
                self.payload.len().saturating_sub(start)
            )));
        }
        self.position = end as u64;
        Ok(self.payload[start..end].to_vec())
    }

    /// Reads up to `count` bytes and advances the cursor. Returns fewer bytes
    /// if the payload runs out instead of failing.
    pub fn read_bytes(&mut self, count: usize) -> Vec<u8> {
        let start = (self.position as usize).min(self.payload.len());
        let end = start.saturating_add(count).min(self.payload.len());
        self.position = end as u64;
        self.payload[start..end].to_vec()
    }

    /// Reads a single unsigned byte.
    pub fn read_u8(&mut self) -> Result<u8, SteamError> {
        Ok(self.read_exact(1)?[0])
    }

    /// Reads a single signed byte.
    pub fn read_i8(&mut self) -> Result<i8, SteamError> {
        Ok(self.read_u8()? as i8)
    }

    /// Reads a single unsigned 16 bit short.
    pub fn read_u16(&mut self) -> Result<u16, SteamError> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a single signed 16 bit short.
    pub fn read_i16(&mut self) -> Result<i16, SteamError> {
        Ok(self.read_u16()? as i16)
    }

    /// Reads a single unsigned 32 bit integer.
    pub fn read_u32(&mut self) -> Result<u32, SteamError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a single signed 32 bit integer.
    pub fn read_i32(&mut self) -> Result<i32, SteamError> {
        Ok(self.read_u32()? as i32)
    }

    /// Reads a single unsigned 64 bit integer.
    pub fn read_u64(&mut self) -> Result<u64, SteamError> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a single signed 64 bit integer.
    pub fn read_i64(&mut self) -> Result<i64, SteamError> {
        Ok(self.read_u64()? as i64)
    }

    /// Reads a single 32 bit float.
    pub fn read_f32(&mut self) -> Result<f32, SteamError> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    /// Reads a single 64 bit double.
    pub fn read_f64(&mut self) -> Result<f64, SteamError> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    /// Reads a null terminated UTF-8 string, consuming the terminator.
    pub fn read_null_term_string(&mut self) -> Result<String, SteamError> {
        let mut bytes = Vec::new();
        loop {
            let byte = self.read_u8()?;
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Same as [`read_i8`](Self::read_i8).
    pub fn read_int8(&mut self) -> Result<i8, SteamError> {
        self.read_i8()
    }

    /// Same as [`read_i8`](Self::read_i8).
    pub fn read_sbyte(&mut self) -> Result<i8, SteamError> {
        self.read_i8()
    }

    /// Same as [`read_u8`](Self::read_u8).
    pub fn read_uint8(&mut self) -> Result<u8, SteamError> {
        self.read_u8()
    }

    /// Same as [`read_u8`](Self::read_u8).
    pub fn read_byte(&mut self) -> Result<u8, SteamError> {
        self.read_u8()
    }

    /// Same as [`read_i16`](Self::read_i16).
    pub fn read_int16(&mut self) -> Result<i16, SteamError> {
        self.read_i16()
    }

    /// Same as [`read_i16`](Self::read_i16).
    pub fn read_short(&mut self) -> Result<i16, SteamError> {
        self.read_i16()
    }

    /// Same as [`read_u16`](Self::read_u16).
    pub fn read_uint16(&mut self) -> Result<u16, SteamError> {
        self.read_u16()
    }

    /// Same as [`read_u16`](Self::read_u16).
    pub fn read_ushort(&mut self) -> Result<u16, SteamError> {
        self.read_u16()
    }

    /// Same as [`read_i32`](Self::read_i32).
    pub fn read_int32(&mut self) -> Result<i32, SteamError> {
        self.read_i32()
    }

    /// Same as [`read_i32`](Self::read_i32).
    pub fn read_int(&mut self) -> Result<i32, SteamError> {
        self.read_i32()
    }

    /// Same as [`read_u32`](Self::read_u32).
    pub fn read_uint32(&mut self) -> Result<u32, SteamError> {
        self.read_u32()
    }

    /// Same as [`read_u32`](Self::read_u32).
    pub fn read_uint(&mut self) -> Result<u32, SteamError> {
        self.read_u32()
    }

    /// Same as [`read_i64`](Self::read_i64).
    pub fn read_int64(&mut self) -> Result<i64, SteamError> {
        self.read_i64()
    }

    /// Same as [`read_i64`](Self::read_i64).
    pub fn read_long(&mut self) -> Result<i64, SteamError> {
        self.read_i64()
    }

    /// Same as [`read_u64`](Self::read_u64).
    pub fn read_uint64(&mut self) -> Result<u64, SteamError> {
        self.read_u64()
    }

    /// Same as [`read_u64`](Self::read_u64).
    pub fn read_ulong(&mut self) -> Result<u64, SteamError> {
        self.read_u64()
    }

    /// Same as [`read_f32`](Self::read_f32).
    pub fn read_single(&mut self) -> Result<f32, SteamError> {
        self.read_f32()
    }

    /// Same as [`read_f32`](Self::read_f32).
    pub fn read_float(&mut self) -> Result<f32, SteamError> {
        self.read_f32()
    }

    /// Same as [`read_f64`](Self::read_f64).
    pub fn read_double(&mut self) -> Result<f64, SteamError> {
        self.read_f64()
    }
}
