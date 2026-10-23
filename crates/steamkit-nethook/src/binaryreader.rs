//! A small forward-only byte reader.

/// A forward-only reader over a byte buffer.
#[derive(Debug)]
pub struct BinaryReader<'a> {
    data: &'a [u8],
    position: u32,
}

impl<'a> BinaryReader<'a> {
    /// Creates a reader positioned at the start of `data`.
    pub fn new(data: &'a [u8]) -> BinaryReader<'a> {
        BinaryReader { data, position: 0 }
    }

    /// Returns the current read position.
    pub fn position(&self) -> u32 {
        self.position
    }

    /// Moves the read position to `position`.
    pub fn set_position(&mut self, position: u32) {
        self.position = position;
    }

    /// Moves the read position forward by `offset` bytes.
    pub fn seek_relative(&mut self, offset: u32) {
        self.position = self.position.saturating_add(offset);
    }

    /// Returns the number of bytes left to read.
    pub fn size_left(&self) -> u32 {
        let len = self.data.len() as u32;
        len.saturating_sub(self.position)
    }

    /// Reads a little-endian `u32`.
    ///
    /// Reads past the end are clamped instead of going out of bounds.
    pub fn read_u32(&mut self) -> u32 {
        let start = self.position as usize;
        let end = start.saturating_add(4);
        let value = if end <= self.data.len() {
            u32::from_le_bytes([
                self.data[start],
                self.data[start + 1],
                self.data[start + 2],
                self.data[start + 3],
            ])
        } else {
            0
        };

        self.position = self.position.saturating_add(4);
        value
    }

    /// Reads up to `len` bytes and returns them as a slice of the underlying buffer.
    ///
    /// The position always advances by `len`, but the returned slice is
    /// clamped to what's actually available.
    pub fn read_bytes(&mut self, len: u32) -> &'a [u8] {
        let start = (self.position as usize).min(self.data.len());
        let end = start.saturating_add(len as usize).min(self.data.len());

        self.position = self.position.saturating_add(len);
        &self.data[start..end]
    }
}
