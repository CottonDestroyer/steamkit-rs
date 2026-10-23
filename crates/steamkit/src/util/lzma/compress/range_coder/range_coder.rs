//! The range encoder from the LZMA SDK (`RangeCoder`).
//!
//! Output goes into an owned `Vec<u8>`. `low` is a 64 bit accumulator whose
//! upper half holds the carry; `range` is a u32.

/// The binary range encoder.
#[derive(Debug, Clone, Default)]
pub struct Encoder {
    /// The output buffer.
    stream: Vec<u8>,

    /// The low bound of the current interval.
    pub low: u64,

    /// The size of the current interval.
    pub range: u32,

    cache_size: u32,
    cache: u8,

    start_position: i64,
}

impl Encoder {
    /// The value above which the range is renormalised.
    pub const TOP_VALUE: u32 = 1 << 24;

    /// Creates an uninitialised encoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the output buffer.
    pub fn set_stream(&mut self, stream: Vec<u8>) {
        self.stream = stream;
    }

    /// Takes the output buffer out of the encoder.
    pub fn release_stream(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.stream)
    }

    /// The bytes written so far.
    pub fn stream(&self) -> &[u8] {
        &self.stream
    }

    /// The bytes written so far, mutably.
    pub fn stream_mut(&mut self) -> &mut Vec<u8> {
        &mut self.stream
    }

    /// Initialises the encoder for a new stream.
    pub fn init(&mut self) {
        self.start_position = self.stream.len() as i64;

        self.low = 0;
        self.range = 0xFFFF_FFFF;
        self.cache_size = 1;
        self.cache = 0;
    }

    /// Flushes the pending low bytes.
    pub fn flush_data(&mut self) {
        for _ in 0..5 {
            self.shift_low();
        }
    }

    /// Flushes the output stream.
    ///
    /// Does nothing, since a `Vec<u8>` has nothing to flush.
    pub fn flush_stream(&mut self) {}

    /// Closes the output stream.
    ///
    /// Drops the output buffer.
    pub fn close_stream(&mut self) {
        self.stream = Vec::new();
    }

    /// Encodes a symbol that occupies `size` of `total` probability units.
    pub fn encode(&mut self, start: u32, size: u32, total: u32) {
        let range_div = self.range / total;
        self.range = range_div;
        // u32 * u32 wraps before being widened to u64, same as the SDK.
        self.low = self
            .low
            .wrapping_add(start.wrapping_mul(range_div) as u64);
        self.range = self.range.wrapping_mul(size);
        while self.range < Self::TOP_VALUE {
            self.range <<= 8;
            self.shift_low();
        }
    }

    /// Shifts the low accumulator by one byte, emitting the pending bytes.
    pub fn shift_low(&mut self) {
        let low32 = self.low as u32;
        if low32 < 0xFF00_0000 || (self.low >> 32) == 1 {
            let mut temp = self.cache;
            loop {
                self.stream
                    .push((temp as u32 + (self.low >> 32) as u32) as u8);
                temp = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = (low32 >> 24) as u8;
        }
        self.cache_size += 1;
        self.low = (low32 << 8) as u64;
    }

    /// Encodes `num_total_bits` bits of `v` without a probability model.
    pub fn encode_direct_bits(&mut self, v: u32, num_total_bits: i32) {
        let mut i = num_total_bits - 1;
        while i >= 0 {
            self.range >>= 1;
            if ((v >> i) & 1) == 1 {
                self.low = self.low.wrapping_add(self.range as u64);
            }
            if self.range < Self::TOP_VALUE {
                self.range <<= 8;
                self.shift_low();
            }
            i -= 1;
        }
    }

    /// Encodes a single bit using a probability model of `num_total_bits` bits.
    pub fn encode_bit(&mut self, size0: u32, num_total_bits: i32, symbol: u32) {
        let new_bound = (self.range >> num_total_bits) * size0;
        if symbol == 0 {
            self.range = new_bound;
        } else {
            self.low = self.low.wrapping_add(new_bound as u64);
            self.range -= new_bound;
        }
        while self.range < Self::TOP_VALUE {
            self.range <<= 8;
            self.shift_low();
        }
    }

    /// The number of bytes produced so far, plus the four bytes that the
    /// decoder needs to finish the stream.
    pub fn get_processed_size_add(&self) -> i64 {
        self.cache_size as i64 + (self.stream.len() as i64 - self.start_position) + 4
    }
}
