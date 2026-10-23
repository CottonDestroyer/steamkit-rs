//! The sliding input window from the LZMA SDK (`LzInWindow`).
//!
//! The window keeps the not-yet-consumed bytes in a `Vec<u8>` and slides it
//! forward when the end of the buffer is reached. The offset math is u32
//! arithmetic that is expected to wrap, hence the `wrapping_*` calls.

/// A read cursor over an in-memory buffer. This is what the window reads
/// its input from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ByteReader {
    data: Vec<u8>,
    position: usize,
}

impl ByteReader {
    /// Creates a reader over `data`.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, position: 0 }
    }

    /// The current read position.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The total number of bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the reader has reached the end of the buffer.
    pub fn is_empty(&self) -> bool {
        self.position >= self.data.len()
    }

    /// Reads up to `buffer.len()` bytes and returns how many were read.
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        let available = self.data.len().saturating_sub(self.position);
        let count = available.min(buffer.len());
        if count == 0 {
            return 0;
        }
        buffer[..count].copy_from_slice(&self.data[self.position..self.position + count]);
        self.position += count;
        count
    }
}

/// The sliding window that holds the data the match finder works on.
#[derive(Debug, Clone, Default)]
pub struct InWindow {
    /// The buffer that holds the data.
    pub buffer_base: Vec<u8>,

    stream: Option<ByteReader>,

    /// Offset (from the buffer) of the first byte when a new block must be read.
    pos_limit: u32,
    /// Whether the end of the stream has been reached.
    stream_end_was_reached: bool,

    pointer_to_last_safe_position: u32,

    /// Offset (from the buffer) of the current byte.
    pub buffer_offset: u32,

    /// Size of the allocated memory block.
    pub block_size: u32,
    /// Offset (from the buffer) of the current byte.
    pub pos: u32,

    keep_size_before: u32,
    keep_size_after: u32,

    /// Offset (from the buffer) of the first byte not read from the stream.
    pub stream_pos: u32,
}

impl InWindow {
    /// Creates an empty window.
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the head of the buffer forward, dropping the bytes that are no
    /// longer needed.
    pub fn move_block(&mut self) {
        let mut offset = self
            .buffer_offset
            .wrapping_add(self.pos)
            .wrapping_sub(self.keep_size_before);
        // We need one additional byte, since MovePos moves on 1 byte.
        if offset > 0 {
            offset -= 1;
        }

        let num_bytes = self
            .buffer_offset
            .wrapping_add(self.stream_pos)
            .wrapping_sub(offset);

        let source = offset as usize;
        let destination = source.wrapping_add(num_bytes as usize);
        if destination <= self.buffer_base.len() {
            self.buffer_base.copy_within(source..destination, 0);
        }
        self.buffer_offset = self.buffer_offset.wrapping_sub(offset);
    }

    /// Reads more data from the stream into the window.
    pub fn read_block(&mut self) {
        if self.stream_end_was_reached {
            return;
        }
        loop {
            let size = (0u32
                .wrapping_sub(self.buffer_offset)
                .wrapping_add(self.block_size)
                .wrapping_sub(self.stream_pos)) as i32;
            if size == 0 {
                return;
            }
            if size < 0 {
                return;
            }

            let read_pos = self.buffer_offset.wrapping_add(self.stream_pos) as usize;
            let end = read_pos.saturating_add(size as usize);

            let num_read_bytes = match self.stream.as_mut() {
                Some(reader) => {
                    if end <= self.buffer_base.len() {
                        reader.read(&mut self.buffer_base[read_pos..end])
                    } else {
                        0
                    }
                }
                None => 0,
            };

            if num_read_bytes == 0 {
                self.pos_limit = self.stream_pos;
                let pointer_to_position = self.buffer_offset.wrapping_add(self.pos_limit);
                if pointer_to_position > self.pointer_to_last_safe_position {
                    self.pos_limit = self
                        .pointer_to_last_safe_position
                        .wrapping_sub(self.buffer_offset);
                }
                self.stream_end_was_reached = true;
                return;
            }
            self.stream_pos = self.stream_pos.wrapping_add(num_read_bytes as u32);
            if self.stream_pos >= self.pos.wrapping_add(self.keep_size_after) {
                self.pos_limit = self.stream_pos.wrapping_sub(self.keep_size_after);
            }
        }
    }

    /// Sets the input stream.
    pub fn set_stream(&mut self, stream: Vec<u8>) {
        self.stream = Some(ByteReader::new(stream));
    }

    /// Releases the input stream.
    pub fn release_stream(&mut self) {
        self.stream = None;
    }

    /// Prepares the window for a new stream.
    pub fn init(&mut self) {
        self.buffer_offset = 0;
        self.pos = 0;
        self.stream_pos = 0;
        self.stream_end_was_reached = false;
        self.read_block();
    }

    /// Advances the current position by one byte.
    pub fn move_pos(&mut self) {
        self.pos = self.pos.wrapping_add(1);
        if self.pos > self.pos_limit {
            let pointer_to_position = self.buffer_offset.wrapping_add(self.pos);
            if pointer_to_position > self.pointer_to_last_safe_position {
                self.move_block();
            }
            self.read_block();
        }
    }

    /// The byte at `index` relative to the current position.
    ///
    /// A negative index refers to a byte before the current position. Bytes
    /// outside of the buffer read as zero instead of raising an error.
    pub fn get_index_byte(&self, index: i32) -> u8 {
        let base = self.buffer_offset.wrapping_add(self.pos);
        let position = base as i64 + index as i64;
        if position < 0 {
            return 0;
        }
        self.buffer_base
            .get(position as usize)
            .copied()
            .unwrap_or(0)
    }

    /// The length of the match at `index` that is `distance` bytes away,
    /// limited to `limit` bytes.
    ///
    /// `index` and `limit` must not exceed the look-ahead size.
    pub fn get_match_len(&self, index: i32, distance: u32, limit: u32) -> u32 {
        let mut limit = limit;
        let position = self.pos as i64 + index as i64;
        if self.stream_end_was_reached && position + limit as i64 > self.stream_pos as i64 {
            limit = self.stream_pos.wrapping_sub(position as u32);
        }
        let distance = distance.wrapping_add(1);
        let pby = self
            .buffer_offset
            .wrapping_add(self.pos)
            .wrapping_add(index as u32);

        let mut i = 0u32;
        while i < limit {
            let current = self.buffer_base.get(pby.wrapping_add(i) as usize);
            let matched = self
                .buffer_base
                .get(pby.wrapping_add(i).wrapping_sub(distance) as usize);
            match (current, matched) {
                (Some(a), Some(b)) if a == b => {}
                _ => break,
            }
            i += 1;
        }
        i
    }

    /// The number of bytes that have not been consumed yet.
    pub fn get_num_available_bytes(&self) -> u32 {
        self.stream_pos.wrapping_sub(self.pos)
    }

    /// Shifts every offset by `sub_value`.
    pub fn reduce_offsets(&mut self, sub_value: i32) {
        let sub_value = sub_value as u32;
        self.buffer_offset = self.buffer_offset.wrapping_add(sub_value);
        self.pos_limit = self.pos_limit.wrapping_sub(sub_value);
        self.pos = self.pos.wrapping_sub(sub_value);
        self.stream_pos = self.stream_pos.wrapping_sub(sub_value);
    }

    /// Creates the window with the given keep sizes.
    pub fn create(&mut self, keep_size_before: u32, keep_size_after: u32, keep_size_reserv: u32) {
        self.keep_size_before = keep_size_before;
        self.keep_size_after = keep_size_after;
        let block_size = keep_size_before
            .wrapping_add(keep_size_after)
            .wrapping_add(keep_size_reserv);
        if self.buffer_base.is_empty() || self.block_size != block_size {
            self.block_size = block_size;
            self.buffer_base = vec![0u8; block_size as usize];
        }
        self.pointer_to_last_safe_position = self.block_size.wrapping_sub(keep_size_after);
    }
}
