//! Gzip decompression for multi messages.
//!
//! Only gzip is accepted; a zlib stream is rejected.

use std::io::Read;

use flate2::read::GzDecoder;

/// Decompresses a gzip stream into a buffer of exactly `decompressed_len` bytes.
///
/// Returns `None` when the stream is truncated, corrupt, or doesn't
/// decompress to exactly `decompressed_len` bytes.
pub fn inflate(compressed: &[u8], decompressed_len: u32) -> Option<Vec<u8>> {
    let mut decoder = GzDecoder::new(compressed);
    let mut out = vec![0u8; decompressed_len as usize];

    let mut filled = 0usize;
    while filled < out.len() {
        match decoder.read(&mut out[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(_) => return None,
        }
    }

        // The stream has to end exactly at `decompressed_len` bytes. One more
        // read tells us whether it did.
    let mut probe = [0u8; 1];
    match decoder.read(&mut probe) {
        Ok(0) => Some(out),
        _ => None,
    }
}
