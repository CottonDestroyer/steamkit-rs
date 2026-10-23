//! Decoding blobs that start with the "LZMA" magic header.

use std::io::{Read, Write};

use crate::error::SteamError;

use super::lzma_decoder::LzmaDecoder;

/// The "LZMA" magic that marks a blob with the LZMA header.
const LZMA_MAGIC: u32 = 0x4C5A_4D41;

/// Decodes blobs that start with the "LZMA" magic header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LzmaUtil;

impl LzmaUtil {
    /// Whether `data` starts with the LZMA magic.
    pub fn has_lzma_header(data: &[u8]) -> bool {
        if data.len() < std::mem::size_of::<u32>() {
            return false;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        magic == LZMA_MAGIC
    }

    /// Decompresses an LZMA stream.
    ///
    /// * `input` - the LZMA stream.
    /// * `output` - where to write the decompressed data.
    /// * `compressed_length` - the expected compressed length.
    /// * `uncompressed_length` - the expected decompressed length.
    pub fn decompress(
        input: &mut dyn Read,
        output: &mut dyn Write,
        compressed_length: usize,
        uncompressed_length: usize,
    ) -> Result<(), SteamError> {
        let mut properties = [0u8; LzmaDecoder::PROPERTIES_SIZE];
        input.read_exact(&mut properties)?;

        let mut compressed = vec![0u8; compressed_length];
        input.read_exact(&mut compressed)?;

        let mut decompressed = vec![0u8; uncompressed_length];
        LzmaDecoder::decode(&properties, &compressed, &mut decompressed, 0)?;

        output.write_all(&decompressed)?;
        Ok(())
    }

    /// Decompresses a blob with the LZMA header into a buffer from
    /// `output_factory`, which gets the expected decompressed length. The
    /// buffer is cleared first, so it ends up holding exactly the decompressed
    /// data.
    ///
    /// Returns `None` if the data doesn't start with the LZMA header.
    pub fn try_decompress(
        input: &mut dyn Read,
        output_factory: &mut dyn FnMut(usize) -> Vec<u8>,
    ) -> Result<Option<Vec<u8>>, SteamError> {
        let mut buffer = [0u8; std::mem::size_of::<u32>()];

        if !try_read_exactly(input, &mut buffer)? {
            return Ok(None);
        }

        let magic = u32::from_be_bytes(buffer);
        if magic != LZMA_MAGIC {
            return Ok(None);
        }

        if !try_read_exactly(input, &mut buffer)? {
            return Ok(None);
        }

        let uncompressed_length = i32::from_le_bytes(buffer);

        if !try_read_exactly(input, &mut buffer)? {
            return Ok(None);
        }

        let compressed_length = i32::from_le_bytes(buffer);

        if compressed_length < 0 || uncompressed_length < 0 {
            return Err(SteamError::Decompression(format!(
                "LZMA lengths must not be negative (compressed {compressed_length}, uncompressed {uncompressed_length})."
            )));
        }

        let mut output = output_factory(uncompressed_length as usize);

        // Start from an empty buffer so it holds only the decompressed data.
        output.clear();

        Self::decompress(
            input,
            &mut output,
            compressed_length as usize,
            uncompressed_length as usize,
        )?;

        Ok(Some(output))
    }
}

fn try_read_exactly(stream: &mut dyn Read, buffer: &mut [u8]) -> Result<bool, SteamError> {
    let mut read = 0;
    while read < buffer.len() {
        let count = stream.read(&mut buffer[read..])?;
        if count == 0 {
            return Ok(false);
        }

        read += count;
    }

    Ok(true)
}
