//! Decompressing VZip deltas: VZip streams compressed against a previous chunk.

use crate::error::SteamError;
use crate::util::lzma_decoder::LzmaDecoder;
use crate::util::v_zip_util::VZipUtil;

/// The version byte of VZip delta streams.
const VERSION: u8 = b'd';

/// VZip delta decompression.
#[derive(Debug)]
pub struct VZipDeltaUtil;

impl VZipDeltaUtil {
    /// Decompresses a VZip delta against `source_chunk`. The source chunk is
    /// the preset dictionary of the LZMA stream.
    pub fn decompress(
        buffer: &[u8],
        source_chunk_data: &[u8],
    ) -> Result<Vec<u8>, SteamError> {
        // The CRC in the header is the chunk's CRC too.
        let frame = VZipUtil::read_frame(buffer, VERSION)?;

        let total_length = source_chunk_data.len() + frame.decompressed_size as usize;
        let mut window = vec![0u8; total_length];

        window[..source_chunk_data.len()].copy_from_slice(source_chunk_data);

        LzmaDecoder::decode(
            frame.properties,
            frame.compressed,
            &mut window,
            source_chunk_data.len(),
        )?;

        let out_data = &window[source_chunk_data.len()..];

        if crc32fast::hash(out_data) != frame.output_crc {
            return Err(SteamError::Decompression(
                "CRC does not match decompressed data. VZip data may be corrupted.".to_string(),
            ));
        }

        Ok(out_data.to_vec())
    }
}
