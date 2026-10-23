//! CRC-32 (reflected, polynomial `0xEDB88320`) as used by the LZMA SDK.

/// The CRC lookup table, built at compile time.
pub const TABLE: [u32; 256] = build_table();

const K_POLY: u32 = 0xEDB8_8320;

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut r = i;
        let mut j = 0;
        while j < 8 {
            if (r & 1) != 0 {
                r = (r >> 1) ^ K_POLY;
            } else {
                r >>= 1;
            }
            j += 1;
        }
        table[i as usize] = r;
        i += 1;
    }
    table
}

/// A running CRC-32 checksum, as used by the LZMA SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crc {
    value: u32,
}

impl Default for Crc {
    /// Starts at `0xFFFFFFFF`.
    fn default() -> Self {
        Self { value: 0xFFFF_FFFF }
    }
}

impl Crc {
    /// Creates a new checksum.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the checksum to its initial state.
    pub fn init(&mut self) {
        self.value = 0xFFFF_FFFF;
    }

    /// Folds a single byte into the checksum.
    pub fn update_byte(&mut self, b: u8) {
        self.value = TABLE[((self.value as u8) ^ b) as usize] ^ (self.value >> 8);
    }

    /// Folds `size` bytes starting at `offset` into the checksum.
    ///
    /// Bytes past the end of `data` are ignored instead of raising an
    /// out-of-range error.
    pub fn update(&mut self, data: &[u8], offset: u32, size: u32) {
        let start = (offset as usize).min(data.len());
        let end = (offset as usize)
            .saturating_add(size as usize)
            .min(data.len());
        for &b in &data[start..end] {
            self.value = TABLE[((self.value as u8) ^ b) as usize] ^ (self.value >> 8);
        }
    }

    /// The current digest.
    pub fn get_digest(&self) -> u32 {
        self.value ^ 0xFFFF_FFFF
    }

    /// Calculates the digest of a range of bytes.
    pub fn calculate_digest(data: &[u8], offset: u32, size: u32) -> u32 {
        let mut crc = Crc::new();
        crc.update(data, offset, size);
        crc.get_digest()
    }

    /// Verifies that a range of bytes hashes to `digest`.
    pub fn verify_digest(digest: u32, data: &[u8], offset: u32, size: u32) -> bool {
        Self::calculate_digest(data, offset, size) == digest
    }
}
