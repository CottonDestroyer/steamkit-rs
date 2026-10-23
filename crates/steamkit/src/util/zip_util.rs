//! Decompressing PKZIP archives with a single file, as older depot chunks use.
//! Reads the zip structures directly and inflates with `flate2`.

use std::io::Read;

use crate::error::SteamError;
use crate::util::debug_log::DebugLog;

/// Signature of the end of central directory record.
const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];

/// Signature of a central directory file header.
const CENTRAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4B, 0x01, 0x02];

/// Signature of a local file header.
const LOCAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];

/// Compression method: stored (not compressed).
const COMPRESSION_STORED: u16 = 0;

/// Compression method: deflate.
const COMPRESSION_DEFLATE: u16 = 8;

/// The PKZIP format.
#[derive(Debug)]
pub(crate) struct ZipUtil;

impl ZipUtil {
    /// Decompresses the single file of a zip archive into `destination` and
    /// returns how many bytes were written.
    pub(crate) fn decompress<R: Read + ?Sized>(
        stream: &mut R,
        destination: &mut [u8],
        verify_checksum: bool,
    ) -> Result<i32, SteamError> {
        let mut data = Vec::new();
        stream.read_to_end(&mut data)?;

        let entry = read_first_entry(&data)?;

        DebugLog::assert(
            entry.total_entries == 1,
            "ZipUtil",
            "Expected the zip to contain only one file",
        );

        if destination.len() < entry.uncompressed_size as usize {
            return Err(SteamError::InvalidArgument(
                "The destination buffer is smaller than the decompressed data size.".to_string(),
            ));
        }

        let decompressed = decompress_entry(&data, &entry)?;

        if decompressed.len() != entry.uncompressed_size as usize {
            return Err(SteamError::Decompression(format!(
                "The zip entry is {} bytes long, but the archive declares {} bytes.",
                decompressed.len(),
                entry.uncompressed_size
            )));
        }

        destination[..decompressed.len()].copy_from_slice(&decompressed);

        if verify_checksum && crc32fast::hash(&destination[..decompressed.len()]) != entry.crc32 {
            return Err(SteamError::Decompression(
                "Checksum validation failed for decompressed file".to_string(),
            ));
        }

        Ok(entry.uncompressed_size as i32)
    }
}

/// The central directory entry of the file.
#[derive(Debug)]
struct ZipEntry {
    /// How many entries the archive has.
    total_entries: usize,
    /// The CRC of the uncompressed file.
    crc32: u32,
    /// The size of the compressed file.
    compressed_size: usize,
    /// The size of the uncompressed file.
    uncompressed_size: usize,
    /// The compression method of the file.
    compression_method: u16,
    /// The offset of the local file header of the file.
    local_header_offset: usize,
}

/// Reads the central directory entry of the first file.
fn read_first_entry(data: &[u8]) -> Result<ZipEntry, SteamError> {
    let eocd = find_end_of_central_directory(data).ok_or_else(|| {
        SteamError::Decompression("The zip archive has no end of central directory record.".to_string())
    })?;

    let total_entries =
        u16::from_le_bytes([data[eocd + 10], data[eocd + 11]]) as usize;
    let central_offset = u32::from_le_bytes([
        data[eocd + 16],
        data[eocd + 17],
        data[eocd + 18],
        data[eocd + 19],
    ]) as usize;

    let header = data
        .get(central_offset..central_offset + 46)
        .ok_or_else(|| SteamError::Decompression("The zip central directory is truncated.".to_string()))?;

    if header[..4] != CENTRAL_HEADER_SIGNATURE {
        return Err(SteamError::Decompression(
            "Expecting a central directory file header in the zip archive.".to_string(),
        ));
    }

    let compression_method = u16::from_le_bytes([header[10], header[11]]);
    let crc32 = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
    let compressed_size = u32::from_le_bytes([header[20], header[21], header[22], header[23]]) as usize;
    let uncompressed_size =
        u32::from_le_bytes([header[24], header[25], header[26], header[27]]) as usize;
    let local_header_offset =
        u32::from_le_bytes([header[42], header[43], header[44], header[45]]) as usize;

    if compressed_size == u32::MAX as usize || uncompressed_size == u32::MAX as usize {
        return Err(SteamError::Decompression(
            "Zip64 archives are not supported.".to_string(),
        ));
    }

    Ok(ZipEntry {
        total_entries,
        crc32,
        compressed_size,
        uncompressed_size,
        compression_method,
        local_header_offset,
    })
}

/// Finds the end of central directory record.
fn find_end_of_central_directory(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }

    // The record is at the end, after an optional comment of up to 65535 bytes.
    let maximum = std::cmp::min(data.len(), 0xFFFF + 22);
    let start = data.len() - maximum;

    for offset in (start..=data.len() - 22).rev() {
        if data[offset..offset + 4] == EOCD_SIGNATURE {
            return Some(offset);
        }
    }

    None
}

/// Decompresses the file of `entry`.
fn decompress_entry(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, SteamError> {
    let header = data.get(entry.local_header_offset..entry.local_header_offset + 30).ok_or_else(
        || SteamError::Decompression("The zip local file header is truncated.".to_string()),
    )?;

    if header[..4] != LOCAL_HEADER_SIGNATURE {
        return Err(SteamError::Decompression(
            "Expecting a local file header in the zip archive.".to_string(),
        ));
    }

    let name_length = u16::from_le_bytes([header[26], header[27]]) as usize;
    let extra_length = u16::from_le_bytes([header[28], header[29]]) as usize;

    let start = entry.local_header_offset + 30 + name_length + extra_length;
    let end = start + entry.compressed_size;

    let compressed = data
        .get(start..end)
        .ok_or_else(|| SteamError::Decompression("The zip entry data is truncated.".to_string()))?;

    match entry.compression_method {
        COMPRESSION_STORED => Ok(compressed.to_vec()),
        COMPRESSION_DEFLATE => {
            let mut decoder = flate2::read::DeflateDecoder::new(compressed);
            let mut decompressed = Vec::with_capacity(entry.uncompressed_size);

            decoder.read_to_end(&mut decompressed)?;

            Ok(decompressed)
        }
        method => Err(SteamError::Decompression(format!(
            "The zip entry uses the unsupported compression method {method}."
        ))),
    }
}
