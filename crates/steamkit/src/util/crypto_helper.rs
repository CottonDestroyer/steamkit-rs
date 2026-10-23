//! The symmetric crypto used in Steam's protocols.

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyInit, KeyIvInit};
use aes::cipher::generic_array::GenericArray;
use cbc::Decryptor as CbcDecryptor;
use ecb::Decryptor as EcbDecryptor;

use crate::error::SteamError;

/// The size of the AES block, in bytes.
const BLOCK_SIZE: usize = 16;

/// The size of the AES-256 key, in bytes.
const KEY_SIZE: usize = 32;

/// Decrypts AES-256-CBC (PKCS7) data whose IV is prepended, ECB-encrypted.
/// Same as [`CryptoHelper::symmetric_decrypt`].
pub fn symmetric_decrypt(input: &[u8], key: &[u8]) -> Result<Vec<u8>, SteamError> {
    CryptoHelper::symmetric_decrypt(input, key)
}

/// The crypto functions used in Steam's protocols.
#[derive(Debug)]
pub struct CryptoHelper;

impl CryptoHelper {
    /// Decrypts AES-256-CBC (PKCS7) data whose IV is prepended, ECB-encrypted.
    ///
    /// The first 16 bytes of `input` are the IV, encrypted with AES-256-ECB
    /// (no padding); the rest is decrypted with AES-256-CBC using that IV. The
    /// key has to be 32 bytes.
    pub fn symmetric_decrypt(input: &[u8], key: &[u8]) -> Result<Vec<u8>, SteamError> {
        if key.len() != KEY_SIZE {
            crate::util::debug_log::DebugLog::assert(
                false,
                "CryptoHelper",
                "SymmetricDecrypt used with non 32 byte key!",
            );

            return Err(SteamError::InvalidArgument(format!(
                "SymmetricDecrypt used with non {KEY_SIZE} byte key (was {} bytes)!",
                key.len()
            )));
        }

        if input.len() < BLOCK_SIZE {
            return Err(SteamError::Crypto(format!(
                "SymmetricDecrypt input is too short to contain the IV (was {} bytes).",
                input.len()
            )));
        }

        // The first 16 bytes are the ECB-encrypted IV.
        let mut iv = [0u8; BLOCK_SIZE];
        {
            let mut decryptor = EcbDecryptor::<aes::Aes256>::new_from_slice(key)
                .map_err(|_| SteamError::Crypto("Invalid AES-256 key.".to_string()))?;

            let mut block = GenericArray::clone_from_slice(&input[..BLOCK_SIZE]);
            decryptor.decrypt_block_mut(&mut block);
            iv.copy_from_slice(block.as_slice());
        }

        // The rest is CBC-encrypted with that IV, PKCS7-padded.
        let ciphertext = &input[BLOCK_SIZE..];

        if ciphertext.is_empty() || ciphertext.len() % BLOCK_SIZE != 0 {
            return Err(SteamError::Crypto(format!(
                "SymmetricDecrypt input has an invalid length (was {} bytes).",
                input.len()
            )));
        }

        let mut buffer = vec![0u8; ciphertext.len()];
        buffer.copy_from_slice(ciphertext);

        let decryptor = CbcDecryptor::<aes::Aes256>::new_from_slices(key, &iv)
            .map_err(|_| SteamError::Crypto("Invalid AES-256 key or IV.".to_string()))?;

        let plaintext = decryptor
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
            .map_err(|_| SteamError::Crypto("Failed to decrypt the payload.".to_string()))?;

        Ok(plaintext.to_vec())
    }
}

