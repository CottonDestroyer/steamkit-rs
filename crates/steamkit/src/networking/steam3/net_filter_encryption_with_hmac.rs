//! The AES + HMAC filter used once channel encryption is set up.
//!
//! # Wire format
//!
//! ```text
//! offset  size  field
//! 0       16    AES-256-ECB(session_key, iv)
//! 16      ..    AES-256-CBC-PKCS7(session_key, iv, plaintext)
//! ```
//!
//! where the 16 byte `iv` is
//!
//! ```text
//! offset  size  field
//! 0       13    HMAC-SHA1(hmac_secret, iv[13..16] || plaintext)[0..13]
//! 13      3     random bytes
//! ```
//!
//! and `hmac_secret` is the first 16 bytes of the 32 byte session key.

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{
    BlockDecrypt, BlockDecryptMut, BlockEncrypt, BlockEncryptMut, KeyInit, KeyIvInit,
};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;

use crate::error::SteamError;

type HmacSha1 = Hmac<Sha1>;

const AES_BLOCK_SIZE: usize = 16;
const SESSION_KEY_LENGTH: usize = 32;
const HMAC_SECRET_LENGTH: usize = 16;
const HMAC_SHA1_OUTPUT_LENGTH: usize = 20;

/// The AES-CBC + HMAC-SHA1 filter of Steam's channel encryption.
#[derive(Clone)]
pub struct NetFilterEncryptionWithHMAC {
    session_key: [u8; SESSION_KEY_LENGTH],
    hmac_secret: [u8; HMAC_SECRET_LENGTH],
}

impl std::fmt::Debug for NetFilterEncryptionWithHMAC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetFilterEncryptionWithHMAC").finish_non_exhaustive()
    }
}

impl NetFilterEncryptionWithHMAC {
    /// The length of the initialization vector, in bytes.
    pub const INITIALIZATION_VECTOR_LENGTH: usize = 16;

    /// The number of random bytes at the end of the initialization vector.
    pub const INITIALIZATION_VECTOR_RANDOM_LENGTH: usize = 3;

    /// Creates a filter from the 32-byte session key.
    pub fn new(session_key: &[u8]) -> Result<Self, SteamError> {
        if session_key.len() != SESSION_KEY_LENGTH {
            return Err(SteamError::InvalidArgument(
                "AES session key was not 32 bytes!".to_string(),
            ));
        }

        let mut key = [0u8; SESSION_KEY_LENGTH];
        key.copy_from_slice(session_key);

        let mut secret = [0u8; HMAC_SECRET_LENGTH];
        secret.copy_from_slice(&session_key[..HMAC_SECRET_LENGTH]);

        Ok(Self {
            session_key: key,
            hmac_secret: secret,
        })
    }

    /// Decrypts and verifies an incoming packet.
    pub fn process_incoming(&self, data: &[u8]) -> Result<Vec<u8>, SteamError> {
        if data.len() < NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_LENGTH {
            return Err(SteamError::Crypto(
                "Unable to decrypt incoming packet: packet is too short".to_string(),
            ));
        }

        let mut iv = [0u8; AES_BLOCK_SIZE];
        iv.copy_from_slice(&data[..AES_BLOCK_SIZE]);

        let decrypted_iv = self.ecb_decrypt(&iv)?;
        let plain_text = self.cbc_decrypt(&data[AES_BLOCK_SIZE..], &decrypted_iv)?;

        self.validate_initialization_vector(&plain_text, &decrypted_iv)?;

        Ok(plain_text)
    }

    /// Encrypts an outgoing packet.
    pub fn process_outgoing(&self, data: &[u8]) -> Result<Vec<u8>, SteamError> {
        let mut output = vec![0u8; self.calculate_max_encrypted_data_length(data.len())];
        let length = self.process_outgoing_into(data, &mut output)?;
        output.truncate(length);
        Ok(output)
    }

    /// Encrypts an outgoing packet into the supplied buffer and returns the
    /// number of bytes written.
    ///
    /// `output` must be at least
    /// [`calculate_max_encrypted_data_length`](Self::calculate_max_encrypted_data_length)
    /// bytes long.
    pub fn process_outgoing_into(
        &self,
        data: &[u8],
        output: &mut [u8],
    ) -> Result<usize, SteamError> {
        let required = self.calculate_max_encrypted_data_length(data.len());
        if output.len() < required {
            return Err(SteamError::InvalidArgument(format!(
                "output buffer must be at least {required} bytes"
            )));
        }

        let mut iv = [0u8; AES_BLOCK_SIZE];
        self.generate_initialization_vector(data, &mut iv)?;

        let crypted_iv = self.ecb_encrypt(&iv)?;
        output[..AES_BLOCK_SIZE].copy_from_slice(&crypted_iv);

        let cipher_text_length = self.cbc_encrypt(data, &iv, &mut output[AES_BLOCK_SIZE..])?;

        Ok(AES_BLOCK_SIZE + cipher_text_length)
    }

    /// Upper bound on the encrypted size of a plaintext of the given length.
    pub fn calculate_max_encrypted_data_length(&self, plaintext_data_length: usize) -> usize {
        let block_size = AES_BLOCK_SIZE;
        let cipher_text_size = (plaintext_data_length + block_size) / block_size * block_size;
        NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_LENGTH + cipher_text_size
    }

    fn generate_initialization_vector(
        &self,
        plain_text: &[u8],
        iv: &mut [u8; AES_BLOCK_SIZE],
    ) -> Result<(), SteamError> {
        let hash_length =
            NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_LENGTH
                - NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_RANDOM_LENGTH;

        let mut rng = rand::thread_rng();
        rng.fill_bytes(
            &mut iv[hash_length..],
        );

        let hash_value = self.compute_hmac(iv, plain_text)?;
        iv[..hash_length].copy_from_slice(&hash_value[..hash_length]);

        Ok(())
    }

    fn validate_initialization_vector(
        &self,
        plain_text: &[u8],
        iv: &[u8; AES_BLOCK_SIZE],
    ) -> Result<(), SteamError> {
        let hash_length =
            NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_LENGTH
                - NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_RANDOM_LENGTH;

        // Constant-time comparison, so the timing doesn't leak how much of a
        // forged tag was right.
        self.hmac(iv, plain_text)?
            .verify_truncated_left(&iv[..hash_length])
            .map_err(|_| SteamError::Crypto("HMAC from server did not match computed HMAC.".to_string()))
    }

    /// The HMAC-SHA1 of `iv[13..16] || plain_text`, keyed with the HMAC secret.
    fn hmac(&self, iv: &[u8; AES_BLOCK_SIZE], plain_text: &[u8]) -> Result<HmacSha1, SteamError> {
        let mut mac = <HmacSha1 as Mac>::new_from_slice(&self.hmac_secret)
            .map_err(|_| SteamError::Crypto("invalid HMAC key length".to_string()))?;
        mac.update(
            &iv[NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_LENGTH
                - NetFilterEncryptionWithHMAC::INITIALIZATION_VECTOR_RANDOM_LENGTH..],
        );
        mac.update(plain_text);
        Ok(mac)
    }

    fn compute_hmac(
        &self,
        iv: &[u8; AES_BLOCK_SIZE],
        plain_text: &[u8],
    ) -> Result<[u8; HMAC_SHA1_OUTPUT_LENGTH], SteamError> {
        let tag = self.hmac(iv, plain_text)?.finalize().into_bytes();

        let mut out = [0u8; HMAC_SHA1_OUTPUT_LENGTH];
        out.copy_from_slice(tag.as_slice());
        Ok(out)
    }

    fn ecb_encrypt(&self, block: &[u8; AES_BLOCK_SIZE]) -> Result<[u8; AES_BLOCK_SIZE], SteamError> {
        let cipher = aes::Aes256::new_from_slice(&self.session_key)
            .map_err(|_| SteamError::Crypto("invalid AES session key length".to_string()))?;

        let mut buffer = aes::cipher::Block::<aes::Aes256>::default();
        buffer.copy_from_slice(block);
        cipher.encrypt_block(&mut buffer);

        let mut out = [0u8; AES_BLOCK_SIZE];
        out.copy_from_slice(buffer.as_slice());
        Ok(out)
    }

    fn ecb_decrypt(&self, block: &[u8; AES_BLOCK_SIZE]) -> Result<[u8; AES_BLOCK_SIZE], SteamError> {
        let cipher = aes::Aes256::new_from_slice(&self.session_key)
            .map_err(|_| SteamError::Crypto("invalid AES session key length".to_string()))?;

        let mut buffer = aes::cipher::Block::<aes::Aes256>::default();
        buffer.copy_from_slice(block);
        cipher.decrypt_block(&mut buffer);

        let mut out = [0u8; AES_BLOCK_SIZE];
        out.copy_from_slice(buffer.as_slice());
        Ok(out)
    }

    fn cbc_encrypt(
        &self,
        plain_text: &[u8],
        iv: &[u8; AES_BLOCK_SIZE],
        output: &mut [u8],
    ) -> Result<usize, SteamError> {
        let encryptor = cbc::Encryptor::<aes::Aes256>::new_from_slices(&self.session_key, iv)
            .map_err(|_| SteamError::Crypto("invalid AES key or IV length".to_string()))?;

        let cipher_text = encryptor
            .encrypt_padded_b2b_mut::<Pkcs7>(plain_text, output)
            .map_err(|_| SteamError::Crypto("AES-CBC encryption failed".to_string()))?;

        Ok(cipher_text.len())
    }

    fn cbc_decrypt(
        &self,
        cipher_text: &[u8],
        iv: &[u8; AES_BLOCK_SIZE],
    ) -> Result<Vec<u8>, SteamError> {
        if cipher_text.is_empty() || cipher_text.len() % AES_BLOCK_SIZE != 0 {
            return Err(SteamError::Crypto(
                "Unable to decrypt incoming packet: invalid ciphertext length".to_string(),
            ));
        }

        let decryptor = cbc::Decryptor::<aes::Aes256>::new_from_slices(&self.session_key, iv)
            .map_err(|_| SteamError::Crypto("invalid AES key or IV length".to_string()))?;

        let mut buffer = vec![0u8; cipher_text.len()];
        let plain_text = decryptor
            .decrypt_padded_b2b_mut::<Pkcs7>(cipher_text, &mut buffer)
            .map_err(|_| {
                SteamError::Crypto("Unable to decrypt incoming packet: bad padding".to_string())
            })?;

        Ok(plain_text.to_vec())
    }
}
