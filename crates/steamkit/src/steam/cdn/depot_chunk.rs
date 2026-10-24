//! Decrypting and decompressing depot chunks.
//!
//! A chunk has no header of its own. On the wire it looks like this:
//!
//! ```text
//! chunk := iv[16] ciphertext[..]
//! iv    := AES-256-ECB(depotKey, iv)      // the IV is stored ECB encrypted
//! ciphertext := AES-256-CBC(depotKey, iv, plaintext) with PKCS#7 padding
//! plaintext := "VSZa" zstd-frame           // Valve zstd container
//!            | "VZa"  lzma-frame           // Valve LZMA container ("VZ" + version 'a')
//!            | "PK\x03\x04" zip           // a plain PKzip archive
//! ```
//!
//! What describes a chunk (SHA-1 id, Adler32 of the decompressed data, offset,
//! compressed and uncompressed size) is in the manifest, see
//! [`ChunkData`].
//! [`DepotChunk::process`] decrypts and decompresses a chunk and checks it
//! against that.

use std::io::Cursor;

use aes::cipher::{
    block_padding::Pkcs7, generic_array::GenericArray, BlockDecrypt, BlockDecryptMut, KeyInit,
    KeyIvInit,
};

use crate::error::SteamError;
use crate::types::depot_manifest::ChunkData;
use crate::util::v_zip_util::VZipUtil;
use crate::util::v_zstd_util::VZstdUtil;
use crate::util::zip_util::ZipUtil;

/// The size of an AES block, in bytes.
const AES_BLOCK_SIZE: usize = 16;

/// The required size of a depot decryption key, in bytes.
const DEPOT_KEY_SIZE: usize = 32;

/// Decrypts and decompresses depot chunks.
pub struct DepotChunk;

impl DepotChunk {
    /// Decrypts `data` with the depot key, decompresses it into `destination`
    /// and verifies it against `info`.
    ///
    /// Returns the number of bytes written to `destination`.
    pub fn process(
        info: &ChunkData,
        data: &[u8],
        destination: &mut [u8],
        depot_key: &[u8],
    ) -> Result<usize, SteamError> {
        if destination.len() < info.uncompressed_length as usize {
            return Err(SteamError::InvalidArgument(format!(
                "The destination buffer must be longer than the chunk UncompressedLength ({}).",
                info.uncompressed_length
            )));
        }

        if depot_key.len() != DEPOT_KEY_SIZE {
            return Err(SteamError::InvalidArgument(format!(
                "Tried to decrypt depot chunk with non {DEPOT_KEY_SIZE} byte key!"
            )));
        }

        if data.len() < AES_BLOCK_SIZE {
            return Err(SteamError::Malformed(format!(
                "Not enough data in the depot chunk (was {} bytes).",
                data.len()
            )));
        }

        let cipher = aes::Aes256::new(GenericArray::from_slice(depot_key));

        // The first 16 bytes are the ECB-encrypted IV.
        let mut iv = [0u8; AES_BLOCK_SIZE];
        {
            let mut block = GenericArray::clone_from_slice(&data[..AES_BLOCK_SIZE]);
            cipher.decrypt_block(&mut block);
            iv.copy_from_slice(block.as_slice());
        }

        // With CBC and padding, the plaintext is always smaller than the input.
        let ciphertext = &data[AES_BLOCK_SIZE..];
        if ciphertext.is_empty() || ciphertext.len() % AES_BLOCK_SIZE != 0 {
            return Err(SteamError::Malformed(format!(
                "The depot chunk has an invalid length (was {} bytes).",
                data.len()
            )));
        }

        let mut buffer = ciphertext.to_vec();
        let written = {
            let decryptor = cbc::Decryptor::<aes::Aes256>::new_from_slices(depot_key, &iv)
                .map_err(|_| SteamError::Crypto("Invalid depot key.".to_string()))?;

            let plaintext = decryptor
                .decrypt_padded_mut::<Pkcs7>(&mut buffer)
                .map_err(|_| SteamError::Crypto("Failed to decrypt the depot chunk.".to_string()))?;

            plaintext.len()
        };

        if written < 16 {
            return Err(SteamError::Decompression(format!(
                "Not enough data in the decrypted depot chunk (was {written} bytes)."
            )));
        }

        let plaintext = &buffer[..written];

        let written_decompressed: i32 = if plaintext.starts_with(b"VSZa") {
            // Zstd
            VZstdUtil::decompress(plaintext, destination, false)?
        } else if plaintext.starts_with(b"VZa") {
            // LZMA
            VZipUtil::decompress(plaintext, destination, false)?
        } else if plaintext.starts_with(b"PK\x03\x04") {
            // PKzip
            let mut stream = Cursor::new(plaintext.to_vec());
            ZipUtil::decompress(&mut stream, destination, false)?
        } else {
            return Err(SteamError::Decompression(format!(
                "Unexpected depot chunk compression (first four bytes are {}).",
                hex::encode_upper(&plaintext[..4])
            )));
        };

        if info.uncompressed_length as i32 != written_decompressed {
            return Err(SteamError::Decompression(format!(
                "Processed data checksum failed to decompressed to the expected chunk uncompressed length. (was {written_decompressed}, should be {})",
                info.uncompressed_length
            )));
        }

        let written_decompressed = usize::try_from(written_decompressed).map_err(|_| {
            SteamError::Decompression(
                "The decompressed depot chunk has a negative length.".to_string(),
            )
        })?;

        let data_crc = DepotChunk::adler_hash(&destination[..written_decompressed]);

        if data_crc != info.checksum {
            return Err(SteamError::Decompression(
                "Processed data checksum is incorrect! Downloaded depot chunk is corrupt or invalid/wrong depot key?".to_string(),
            ));
        }

        Ok(written_decompressed)
    }

    /// Adler32 of `input` with a seed of zero.
    ///
    /// Note: with a zero seed, `s1` starts at 0 rather than the 1 that RFC 1950
    /// uses. That's what Steam's chunk checksums expect.
    pub fn adler_hash(input: &[u8]) -> u32 {
        // See https://www.rfc-editor.org/rfc/rfc1950.html
        //
        // Largest prime below 65536.
        const BASE: u32 = 65521;

        let mut s1: u32 = 0;
        let mut s2: u32 = 0;

        for &byte in input {
            s1 = (s1 + u32::from(byte)) % BASE;
            s2 = (s2 + s1) % BASE;
        }

        (s2 << 16) | s1
    }
}
