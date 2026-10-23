//! Tests for `networking/steam3/net_filter_encryption_with_hmac.rs`, the
//! AES + HMAC filter that encrypts every message of a TCP / UDP CM session.
//!
//! The wire format is pinned down by an independent encoder built from the
//! test suite's own AES-256 and SHA-1 (`tests/common`), which share no code
//! with the library.

#[allow(dead_code)]
mod common;

use steamkit::networking::steam3::net_filter_encryption_with_hmac::NetFilterEncryptionWithHMAC;
use steamkit::SteamError;

const KEY: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
];

fn filter() -> NetFilterEncryptionWithHMAC {
    NetFilterEncryptionWithHMAC::new(&KEY).expect("a 32 byte key is valid")
}

fn plain_text(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index * 7 + 3) as u8).collect()
}

/// HMAC-SHA1 (RFC 2104) on top of the test suite's SHA-1.
fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);

    let mut inner = block.iter().map(|byte| byte ^ 0x36).collect::<Vec<u8>>();
    inner.extend_from_slice(message);
    let inner_hash = common::sha1(&inner);

    let mut outer = block.iter().map(|byte| byte ^ 0x5C).collect::<Vec<u8>>();
    outer.extend_from_slice(&inner_hash);
    common::sha1(&outer)
}

/// An independent implementation of the sending side of the wire format:
/// `AES-ECB(iv) || AES-CBC-PKCS7(iv, plain_text)`, where
/// `iv = HMAC-SHA1(key[..16], random || plain_text)[..13] || random`.
fn reference_encrypt(key: &[u8; 32], plain_text: &[u8], random: [u8; 3]) -> Vec<u8> {
    let mut hmac_input = random.to_vec();
    hmac_input.extend_from_slice(plain_text);
    let tag = hmac_sha1(&key[..16], &hmac_input);

    let mut iv = [0u8; 16];
    iv[..13].copy_from_slice(&tag[..13]);
    iv[13..].copy_from_slice(&random);

    let aes = common::Aes256::new(key);
    let mut output = aes.encrypt_block(&iv).to_vec();
    output.extend_from_slice(&aes.encrypt_cbc_pkcs7(plain_text, &iv));
    output
}

fn assert_crypto_error(result: Result<Vec<u8>, SteamError>, what: &str) {
    match result {
        Err(SteamError::Crypto(_)) => {}
        Err(other) => panic!("{what}: expected a crypto error, got {other:?}"),
        Ok(_) => panic!("{what}: the tampered packet was accepted"),
    }
}

#[test]
fn rejects_session_keys_that_are_not_32_bytes() {
    for length in [0usize, 16, 31, 33, 64] {
        let result = NetFilterEncryptionWithHMAC::new(&vec![0u8; length]);
        assert!(
            matches!(result, Err(SteamError::InvalidArgument(_))),
            "a {length} byte key must be rejected"
        );
    }
}

#[test]
fn round_trips_messages_of_every_length_around_the_block_size() {
    let filter = filter();
    for length in (0..=48).chain([255, 256, 1_000, 65_536]) {
        let message = plain_text(length);
        let encrypted = filter.process_outgoing(&message).unwrap();
        let decrypted = filter.process_incoming(&encrypted).unwrap();
        assert_eq!(message, decrypted, "length {length}");
    }
}

#[test]
fn encrypted_length_is_the_iv_plus_the_padded_message() {
    let filter = filter();
    for length in 0..=64 {
        let encrypted = filter.process_outgoing(&plain_text(length)).unwrap();
        let padded = (length / 16 + 1) * 16;
        assert_eq!(16 + padded, encrypted.len(), "length {length}");
        assert_eq!(
            encrypted.len(),
            filter.calculate_max_encrypted_data_length(length),
            "length {length}"
        );
    }
}

#[test]
fn process_outgoing_into_writes_into_a_caller_buffer() {
    let filter = filter();
    let message = plain_text(40);

    let mut buffer = vec![0xAAu8; filter.calculate_max_encrypted_data_length(message.len()) + 10];
    let written = filter.process_outgoing_into(&message, &mut buffer).unwrap();

    assert_eq!(filter.calculate_max_encrypted_data_length(message.len()), written);
    assert_eq!(message, filter.process_incoming(&buffer[..written]).unwrap());
}

#[test]
fn process_outgoing_into_rejects_a_buffer_that_is_too_small() {
    let filter = filter();
    let message = plain_text(40);
    let mut buffer = vec![0u8; filter.calculate_max_encrypted_data_length(message.len()) - 1];

    assert!(matches!(
        filter.process_outgoing_into(&message, &mut buffer),
        Err(SteamError::InvalidArgument(_))
    ));
}

#[test]
fn the_same_message_encrypts_differently_every_time() {
    let filter = filter();
    let message = plain_text(32);

    let first = filter.process_outgoing(&message).unwrap();
    let second = filter.process_outgoing(&message).unwrap();

    // Three random bytes in the IV: a collision is a 1 in 16 million event.
    assert_ne!(first, second);
}

#[test]
fn decrypts_packets_built_by_an_independent_implementation() {
    let filter = filter();
    for (length, random) in [(0usize, [0u8, 0, 0]), (1, [1, 2, 3]), (16, [0xFF, 0xFE, 0xFD]), (100, [9, 8, 7])] {
        let message = plain_text(length);
        let packet = reference_encrypt(&KEY, &message, random);
        assert_eq!(message, filter.process_incoming(&packet).unwrap(), "length {length}");
    }
}

#[test]
fn rejects_a_packet_whose_hmac_does_not_match() {
    let filter = filter();
    let message = plain_text(20);

    // Correctly encrypted, but the HMAC covers a different message.
    let mut hmac_input = vec![1u8, 2, 3];
    hmac_input.extend_from_slice(b"some other message");
    let tag = hmac_sha1(&KEY[..16], &hmac_input);
    let mut iv = [0u8; 16];
    iv[..13].copy_from_slice(&tag[..13]);
    iv[13..].copy_from_slice(&[1, 2, 3]);
    let aes = common::Aes256::new(&KEY);
    let mut packet = aes.encrypt_block(&iv).to_vec();
    packet.extend_from_slice(&aes.encrypt_cbc_pkcs7(&message, &iv));

    assert_crypto_error(filter.process_incoming(&packet), "forged hmac");
}

#[test]
fn rejects_a_packet_that_was_tampered_with_anywhere() {
    let filter = filter();
    let packet = filter.process_outgoing(&plain_text(40)).unwrap();

    for index in 0..packet.len() {
        let mut tampered = packet.clone();
        tampered[index] ^= 0x01;
        assert_crypto_error(filter.process_incoming(&tampered), &format!("byte {index} flipped"));
    }
}

#[test]
fn rejects_a_packet_encrypted_with_another_key() {
    let mut other_key = KEY;
    other_key[31] ^= 0x80;
    let other = NetFilterEncryptionWithHMAC::new(&other_key).unwrap();

    let packet = other.process_outgoing(&plain_text(40)).unwrap();
    assert_crypto_error(filter().process_incoming(&packet), "wrong key");
}

#[test]
fn rejects_packets_that_are_too_short_or_misaligned() {
    let filter = filter();
    let packet = filter.process_outgoing(&plain_text(40)).unwrap();

    for length in [0usize, 1, 15, 16, 17, 31, packet.len() - 1] {
        assert_crypto_error(filter.process_incoming(&packet[..length]), &format!("{length} bytes"));
    }

    let mut extended = packet.clone();
    extended.push(0);
    assert_crypto_error(filter.process_incoming(&extended), "one extra byte");
}

#[test]
fn rejects_a_packet_with_one_block_dropped_or_appended() {
    let filter = filter();
    let packet = filter.process_outgoing(&plain_text(64)).unwrap();

    let mut shortened = packet.clone();
    shortened.truncate(packet.len() - 16);
    assert_crypto_error(filter.process_incoming(&shortened), "last block dropped");

    let mut extended = packet.clone();
    extended.extend_from_slice(&packet[16..32]);
    assert_crypto_error(filter.process_incoming(&extended), "block appended");
}

#[test]
fn debug_output_does_not_reveal_the_key() {
    let rendered = format!("{:?}", filter());
    assert!(rendered.contains("NetFilterEncryptionWithHMAC"));
    assert!(!rendered.contains("session_key"));
    assert!(!rendered.contains("hmac_secret"));
}
