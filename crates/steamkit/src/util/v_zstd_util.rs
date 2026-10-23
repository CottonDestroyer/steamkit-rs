//! Valve's "VZstd" container, used for zstd-compressed depot chunks.

use crate::error::SteamError;

/// The magic that starts a VZstd stream.
const VZSTD_HEADER: u32 = 0x615A_5356;

/// Magic, CRC.
const HEADER_LENGTH: usize = 8;

/// CRC, decompressed size, unknown, magic.
const FOOTER_LENGTH: usize = 15;

/// The VZstd container format.
#[derive(Debug)]
pub struct VZstdUtil;

impl VZstdUtil {
    /// Decompresses a VZstd buffer into `destination` and returns how many
    /// bytes were written.
    pub fn decompress(
        buffer: &[u8],
        destination: &mut [u8],
        verify_checksum: bool,
    ) -> Result<i32, SteamError> {
        if buffer.len() < HEADER_LENGTH + FOOTER_LENGTH {
            return Err(SteamError::Decompression(format!(
                "VZstd data is too short (was {} bytes).",
                buffer.len()
            )));
        }

        // Native byte order, like the reference implementation.
        if u32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) != VZSTD_HEADER {
            return Err(SteamError::Decompression(
                "Expecting VZstdHeader at start of stream".to_string(),
            ));
        }

        let crc32 = i32::from_ne_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let footer = buffer.len() - FOOTER_LENGTH;
        let crc32_footer =
            i32::from_ne_bytes([buffer[footer], buffer[footer + 1], buffer[footer + 2], buffer[footer + 3]]);
        let size_decompressed = i32::from_ne_bytes([
            buffer[footer + 4],
            buffer[footer + 5],
            buffer[footer + 6],
            buffer[footer + 7],
        ]);

        // The CRC is stored twice, in the header and in the footer.
        debug_assert_eq!(crc32, crc32_footer);

        if buffer[buffer.len() - 3] != b'z'
            || buffer[buffer.len() - 2] != b's'
            || buffer[buffer.len() - 1] != b'v'
        {
            return Err(SteamError::Decompression(
                "Expecting VZstdFooter at end of stream".to_string(),
            ));
        }

        if destination.len() < size_decompressed as usize {
            return Err(SteamError::InvalidArgument(
                "The destination buffer is smaller than the decompressed data size.".to_string(),
            ));
        }

        let input = &buffer[HEADER_LENGTH..footer];

        let decompressed = zstd::stream::decode_all(input)
            .map_err(|_| SteamError::Decompression("Failed to decompress Zstd.".to_string()))?;

        if decompressed.len() != size_decompressed as usize {
            return Err(SteamError::Decompression(format!(
                "Failed to decompress Zstd (expected {size_decompressed} bytes, got {}).",
                decompressed.len()
            )));
        }

        destination[..decompressed.len()].copy_from_slice(&decompressed);

        if verify_checksum
            && crc32fast::hash(&destination[..decompressed.len()]) != crc32_footer as u32
        {
            return Err(SteamError::Decompression(
                "CRC does not match decompressed data. VZstd data may be corrupted.".to_string(),
            ));
        }

        Ok(size_decompressed)
    }
}
