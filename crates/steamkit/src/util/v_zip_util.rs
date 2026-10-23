//! Valve's "VZip" container, used for Steam client updates and depot chunks.

use crate::error::SteamError;
use crate::util::lzma::compress::lzma::lzma_encoder::Encoder;
use crate::util::lzma::i_coder::{
    CoderPropID, CoderProperty, ICoder, ISetCoderProperties, IWriteCoderProperties,
};
use crate::util::lzma_decoder::LzmaDecoder;

/// The magic that starts a VZip stream.
const VZIP_HEADER: u16 = 0x5A56;

/// The magic that ends a VZip stream.
const VZIP_FOOTER: u16 = 0x767A;

/// Magic, version, timestamp/CRC.
const HEADER_LENGTH: usize = 7;

/// CRC, decompressed size, magic.
const FOOTER_LENGTH: usize = 10;

/// The version byte of the VZip files we write.
const VERSION: u8 = b'a';

/// The parts of a VZip buffer: the LZMA properties and stream, and the CRC and
/// size of the decompressed data from the footer.
#[derive(Debug)]
pub struct Frame<'a> {
    /// The LZMA properties of the stream.
    pub properties: &'a [u8],
    /// The compressed stream.
    pub compressed: &'a [u8],
    /// The CRC of the decompressed data.
    pub output_crc: u32,
    /// The size of the decompressed data.
    pub decompressed_size: i32,
}

/// The VZip container format.
#[derive(Debug)]
pub struct VZipUtil;

impl VZipUtil {
    /// Splits a VZip buffer into its parts. The four bytes after the header are
    /// a timestamp in some files (Steam client VZips) and a CRC32 in others
    /// (depot chunks), so they're not interpreted.
    pub fn read_frame(buffer: &[u8], version: u8) -> Result<Frame<'_>, SteamError> {
        if buffer.len() < HEADER_LENGTH + LzmaDecoder::PROPERTIES_SIZE + FOOTER_LENGTH {
            return Err(SteamError::Decompression(format!(
                "VZip data is too short (was {} bytes).",
                buffer.len()
            )));
        }

        let header = u16::from_le_bytes([buffer[0], buffer[1]]);

        if header != VZIP_HEADER {
            return Err(SteamError::Decompression(
                "Expecting VZipHeader at start of stream".to_string(),
            ));
        }

        if buffer[2] != version {
            return Err(SteamError::Decompression(format!(
                "Expecting VZip version '{}'",
                version as char
            )));
        }

        let footer = &buffer[buffer.len() - FOOTER_LENGTH..];
        let output_crc = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
        let size_decompressed = i32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]);

        let footer_magic = u16::from_le_bytes([footer[8], footer[9]]);

        if footer_magic != VZIP_FOOTER {
            return Err(SteamError::Decompression(
                "Expecting VZipFooter at end of stream".to_string(),
            ));
        }

        if size_decompressed < 0 {
            return Err(SteamError::Decompression(format!(
                "VZip decompressed size is negative (was {size_decompressed})."
            )));
        }

        let properties_start = HEADER_LENGTH;
        let compressed_start = HEADER_LENGTH + LzmaDecoder::PROPERTIES_SIZE;
        let compressed_end = buffer.len() - FOOTER_LENGTH;

        Ok(Frame {
            properties: &buffer[properties_start..compressed_start],
            compressed: &buffer[compressed_start..compressed_end],
            output_crc,
            decompressed_size: size_decompressed,
        })
    }

    /// Decompresses a VZip buffer into `destination` and returns how many bytes
    /// were written.
    pub fn decompress(
        buffer: &[u8],
        destination: &mut [u8],
        verify_checksum: bool,
    ) -> Result<i32, SteamError> {
        let frame = VZipUtil::read_frame(buffer, VERSION)?;

        if destination.len() < frame.decompressed_size as usize {
            return Err(SteamError::InvalidArgument(
                "The destination buffer is smaller than the decompressed data size.".to_string(),
            ));
        }

        let output = &mut destination[..frame.decompressed_size as usize];

        LzmaDecoder::decode(frame.properties, frame.compressed, output, 0)?;

        if verify_checksum && crc32fast::hash(output) != frame.output_crc {
            return Err(SteamError::Decompression(
                "CRC does not match decompressed data. VZip data may be corrupted.".to_string(),
            ));
        }

        Ok(frame.decompressed_size)
    }

    /// Compresses `buffer` into a VZip buffer.
    pub fn compress(buffer: &[u8]) -> Result<Vec<u8>, SteamError> {
        let crc = crc32fast::hash(buffer);
        let mut out: Vec<u8> = Vec::with_capacity(buffer.len() + 64);

        out.extend_from_slice(&VZIP_HEADER.to_le_bytes());
        out.push(VERSION);
        out.extend_from_slice(&crc.to_le_bytes());

        let mut encoder = Encoder::new();

        encoder
            .set_coder_properties(
                &[
                    CoderPropID::DictionarySize,
                    CoderPropID::PosStateBits,
                    CoderPropID::LitContextBits,
                    CoderPropID::LitPosBits,
                    CoderPropID::Algorithm,
                    CoderPropID::NumFastBytes,
                    CoderPropID::MatchFinder,
                    CoderPropID::EndMarker,
                ],
                &[
                    CoderProperty::Int32(1 << 23),
                    CoderProperty::Int32(2),
                    CoderProperty::Int32(3),
                    CoderProperty::Int32(0),
                    CoderProperty::Int32(2),
                    CoderProperty::Int32(128),
                    CoderProperty::String("bt4".to_string()),
                    CoderProperty::Boolean(false),
                ],
            )?;

        encoder.write_coder_properties(&mut out);
        encoder.code(buffer.to_vec(), &mut out, -1, -1, None)?;

        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(buffer.len() as u32).to_le_bytes());
        out.extend_from_slice(&VZIP_FOOTER.to_le_bytes());

        Ok(out)
    }
}
