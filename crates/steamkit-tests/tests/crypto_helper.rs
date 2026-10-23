//! `CryptoHelper::symmetric_decrypt`: a random AES-256-CBC ciphertext with an
//! ECB encrypted IV prefix (the format Steam uses) must decrypt back to the
//! plaintext.

mod common;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use rand::RngCore;
use steamkit::util::crypto_helper::CryptoHelper;

/// The plaintext.
const DECRYPTED_EXPECTED: &str = "this is a 24 byte string";

/// The password the key is derived from.
const ENCRYPTION_KEY: &str = "encryption key";

#[test]
fn test_symmetric_encryption() {
    let key = common::sha256(ENCRYPTION_KEY.as_bytes());

    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);

    let aes = common::Aes256::new(&key);

    // The output is the 16 byte ECB encrypted IV followed by the CBC
    // encrypted plaintext.
    let crypted_iv = aes.encrypt_ecb(&iv);
    let cipher_text = aes.encrypt_cbc_pkcs7(DECRYPTED_EXPECTED.as_bytes(), &iv);

    let mut encrypted_data = Vec::with_capacity(crypted_iv.len() + cipher_text.len());
    encrypted_data.extend_from_slice(&crypted_iv);
    encrypted_data.extend_from_slice(&cipher_text);

    // Round trip the ciphertext through base64.
    let encrypted_string = BASE64.encode(&encrypted_data);
    let decrypted_data = BASE64
        .decode(encrypted_string.as_bytes())
        .expect("the base64 round trip must produce the original bytes");

    let decrypted = CryptoHelper::symmetric_decrypt(&decrypted_data, &key)
        .expect("the ciphertext must decrypt");

    assert_eq!(DECRYPTED_EXPECTED.as_bytes(), decrypted.as_slice());
}

