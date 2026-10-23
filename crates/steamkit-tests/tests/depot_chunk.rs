//! `DepotChunk` tests, including the VZip and VZstd containers.
//!
//! `DepotChunk::process` returns `Result<usize, SteamError>`, so the failure
//! cases match on the error variant.

mod common;

use steamkit::error::SteamError;
use steamkit::steam::cdn::depot_chunk::DepotChunk;
use steamkit::types::depot_manifest::ChunkData;
use steamkit::util::crypto_helper::symmetric_decrypt;
use steamkit::util::v_zip_util::VZipUtil;
use steamkit::util::v_zstd_util::VZstdUtil;

/// The depot key of depot 4.
const DEPOT4_KEY: [u8; 32] = [
    0x57, 0xEF, 0x98, 0xDE, 0x53, 0x88, 0xA8, 0x4C, 0xBD, 0x6B, 0xA9, 0x4D, 0xCD, 0xA3, 0x6F, 0x8B,
    0xB7, 0x49, 0x17, 0x7D, 0xAA, 0xBF, 0x35, 0x21, 0x2F, 0xEF, 0xCC, 0x09, 0x83, 0x33, 0xE4, 0xD5,
];

/// The depot key of depot 1004.
const DEPOT1004_KEY: [u8; 32] = [
    0xF2, 0x1F, 0xAA, 0x4F, 0x29, 0xEC, 0xEE, 0x9D, 0x19, 0x2F, 0xC1, 0x78, 0x4D, 0x17, 0x01, 0x37,
    0x6E, 0xB8, 0x7E, 0x13, 0x52, 0x18, 0x24, 0xCE, 0xAF, 0x01, 0x16, 0xB1, 0x0C, 0x55, 0x8A, 0x81,
];

/// The test chunk of depot 4.
fn depot4_chunk() -> ChunkData {
    ChunkData::with_values(
        hex::decode("DC036A660987D8F705936D4443FEDB5E1D2E5D9B").expect("valid chunk id hex"),
        649235822,
        0,
        25072,
        68887,
    )
}

/// The test chunk of depot 1004.
fn depot1004_chunk() -> ChunkData {
    ChunkData::with_values(
        hex::decode("B4E70AFDE4693284936DA1A67758C897A492E1FC").expect("valid chunk id hex"),
        1759764290,
        0,
        33536,
        113816,
    )
}

/// Reads a chunk test asset.
fn read_chunk_file(name: &str) -> Vec<u8> {
    common::read_data_file(name)
}

/// Uppercase hex.
fn hex_upper(data: &[u8]) -> String {
    hex::encode_upper(data)
}

/// Decrypts a depot chunk with the depot key. The tests only feed it valid
/// data, so a failure here means a broken fixture.
fn decrypt_depot_chunk(data: &[u8], depot_key: &[u8]) -> Vec<u8> {
    match symmetric_decrypt(data, depot_key) {
        Ok(plaintext) => plaintext,
        Err(error) => panic!("failed to decrypt the depot chunk: {error}"),
    }
}

/// VZip is a 7 byte header, 5 bytes of LZMA properties, the LZMA stream, and a
/// 10 byte footer of CRC, size and magic.
const VZIP_PROPERTIES_OFFSET: usize = 7;
const VZIP_STREAM_OFFSET: usize = 12;
const VZIP_FOOTER_LENGTH: usize = 10;

/// The decrypted (still VZip compressed) test chunk of depot 4.
fn read_decrypted_v_zip_chunk() -> Vec<u8> {
    decrypt_depot_chunk(
        &read_chunk_file("depot_4_chunk_dc036a660987d8f705936d4443fedb5e1d2e5d9b.bin"),
        &DEPOT4_KEY,
    )
}

/// A copy of a VZip payload with its LZMA stream replaced.
fn with_stream(payload: &[u8], stream: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; VZIP_STREAM_OFFSET + stream.len() + VZIP_FOOTER_LENGTH];

    let head = payload[..VZIP_STREAM_OFFSET].to_vec();
    result[..VZIP_STREAM_OFFSET].copy_from_slice(&head);
    result[VZIP_STREAM_OFFSET..VZIP_STREAM_OFFSET + stream.len()].copy_from_slice(stream);

    let footer_start = payload.len() - VZIP_FOOTER_LENGTH;
    let footer = payload[footer_start..].to_vec();
    let footer_slot = result.len() - VZIP_FOOTER_LENGTH;
    result[footer_slot..].copy_from_slice(&footer);

    result
}

/// A copy of a VZip payload with a different decompressed size in the footer.
fn with_decompressed_size(payload: &[u8], size: i32) -> Vec<u8> {
    let mut result = payload.to_vec();
    let offset = result.len() - 6;
    result[offset..offset + 4].copy_from_slice(&size.to_le_bytes());
    result
}

#[test]
fn decrypts_and_decompresses_depot_chunk_pkzip() {
    let chunk_data = read_chunk_file("depot_440_chunk_bac8e2657470b2eb70d6ddcd6c07004be8738697.bin");

    let chunk = ChunkData::with_values(
        Vec::new(), // id is not needed here
        2130218374,
        0,
        320,
        544,
    );

    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written_length = match DepotChunk::process(&chunk, &chunk_data, &mut destination, &[
        0x44, 0xCE, 0x5C, 0x52, 0x97, 0xA4, 0x15, 0xA1,
        0xA6, 0xF6, 0x9C, 0x85, 0x60, 0x37, 0xA5, 0xA2,
        0xFD, 0xD8, 0x2C, 0xD4, 0x74, 0xFA, 0x65, 0x9E,
        0xDF, 0xB4, 0xD5, 0x9B, 0x2A, 0xBC, 0x55, 0xFC,
    ]) {
        Ok(written) => written,
        Err(error) => panic!("expected the chunk to process, got: {error}"),
    };

    assert_eq!(chunk.compressed_length as usize, chunk_data.len());
    assert_eq!(chunk.uncompressed_length as usize, written_length);

    let hash = hex_upper(&common::sha1(&destination));
    assert_eq!("BAC8E2657470B2EB70D6DDCD6C07004BE8738697", hash);
}

#[test]
fn decrypts_and_decompresses_depot_chunk_vzip() {
    let chunk_data =
        read_chunk_file("depot_232250_chunk_7b8567d9b3c09295cdbf4978c32b348d8e76c750.bin");

    let chunk = ChunkData::with_values(
        Vec::new(), // id is not needed here
        2894626744,
        0,
        304,
        798,
    );

    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written_length = match DepotChunk::process(&chunk, &chunk_data, &mut destination, &[
        0xE5, 0xF6, 0xAE, 0xD5, 0x5E, 0x9E, 0xCE, 0x42,
        0x9E, 0x56, 0xB8, 0x13, 0xFB, 0xF6, 0xBF, 0xE9,
        0x24, 0xF3, 0xCF, 0x72, 0x97, 0x2F, 0xDB, 0xD0,
        0x57, 0x1F, 0xFC, 0xAD, 0x9F, 0x2F, 0x7D, 0xAA,
    ]) {
        Ok(written) => written,
        Err(error) => panic!("expected the chunk to process, got: {error}"),
    };

    assert_eq!(chunk.compressed_length as usize, chunk_data.len());
    assert_eq!(chunk.uncompressed_length as usize, written_length);

    let hash = hex_upper(&common::sha1(&destination));
    assert_eq!("7B8567D9B3C09295CDBF4978C32B348D8E76C750", hash);
}

#[test]
fn decrypts_and_decompresses_depot_chunk_zstd() {
    let chunk_data =
        read_chunk_file("depot_3441461_chunk_9e72678e305540630a665b93e1463bc3983eb55a.bin");

    let chunk = ChunkData::with_values(
        Vec::new(), // id is not needed here
        3753325726,
        0,
        176,
        156,
    );

    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written_length = match DepotChunk::process(&chunk, &chunk_data, &mut destination, &[
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
    ]) {
        Ok(written) => written,
        Err(error) => panic!("expected the chunk to process, got: {error}"),
    };

    assert_eq!(chunk.compressed_length as usize, chunk_data.len());
    assert_eq!(chunk.uncompressed_length as usize, written_length);

    let hash = hex_upper(&common::sha1(&destination));
    assert_eq!("9E72678E305540630A665B93E1463BC3983EB55A", hash);
}

#[test]
fn decrypts_and_decompresses_larger_depot_chunk_vzip() {
    let chunk_data =
        read_chunk_file("depot_4_chunk_dc036a660987d8f705936d4443fedb5e1d2e5d9b.bin");
    let chunk = depot4_chunk();

    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written_length = match DepotChunk::process(&chunk, &chunk_data, &mut destination, &DEPOT4_KEY) {
        Ok(written) => written,
        Err(error) => panic!("expected the chunk to process, got: {error}"),
    };

    assert_eq!(chunk.compressed_length as usize, chunk_data.len());
    assert_eq!(chunk.uncompressed_length as usize, written_length);

    let chunk_id = match chunk.chunk_id.as_deref() {
        Some(chunk_id) => chunk_id,
        None => panic!("the depot 4 chunk must carry a chunk id"),
    };
    assert_eq!(chunk_id, &common::sha1(&destination)[..]);
}

#[test]
fn decrypts_and_decompresses_larger_depot_chunk_zstd() {
    let chunk_data =
        read_chunk_file("depot_1004_chunk_b4e70afde4693284936da1a67758c897a492e1fc.bin");
    let chunk = depot1004_chunk();

    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written_length = match DepotChunk::process(&chunk, &chunk_data, &mut destination, &DEPOT1004_KEY) {
        Ok(written) => written,
        Err(error) => panic!("expected the chunk to process, got: {error}"),
    };

    assert_eq!(chunk.compressed_length as usize, chunk_data.len());
    assert_eq!(chunk.uncompressed_length as usize, written_length);

    let chunk_id = match chunk.chunk_id.as_deref() {
        Some(chunk_id) => chunk_id,
        None => panic!("the depot 1004 chunk must carry a chunk id"),
    };
    assert_eq!(chunk_id, &common::sha1(&destination)[..]);
}

#[test]
fn throws_on_corrupt_depot_chunk_vzip() {
    let mut chunk_data =
        read_chunk_file("depot_4_chunk_dc036a660987d8f705936d4443fedb5e1d2e5d9b.bin");
    let middle = chunk_data.len() / 2;
    chunk_data[middle] ^= 0x55;

    let chunk = depot4_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    // A corrupt payload is reported as a decompression failure.
    match DepotChunk::process(&chunk, &chunk_data, &mut destination, &DEPOT4_KEY) {
        Ok(_) => panic!("expected the corrupt depot chunk to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}

#[test]
fn throws_on_corrupt_depot_chunk_zstd() {
    let mut chunk_data =
        read_chunk_file("depot_1004_chunk_b4e70afde4693284936da1a67758c897a492e1fc.bin");
    let middle = chunk_data.len() / 2;
    chunk_data[middle] ^= 0x55;

    let chunk = depot1004_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    match DepotChunk::process(&chunk, &chunk_data, &mut destination, &DEPOT1004_KEY) {
        Ok(_) => panic!("expected the corrupt depot chunk to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}

#[test]
fn vzip_ignores_dictionary_size_smaller_than_output() {
    let mut payload = read_decrypted_v_zip_chunk();

    let dictionary_size_offset = VZIP_PROPERTIES_OFFSET + 1;
    let dictionary_size = u32::from_le_bytes([
        payload[dictionary_size_offset],
        payload[dictionary_size_offset + 1],
        payload[dictionary_size_offset + 2],
        payload[dictionary_size_offset + 3],
    ]);
    assert!(dictionary_size >= depot4_chunk().uncompressed_length);

    // Matches only reach back into the output, so the dictionary size in the
    // properties does not limit them
    let smaller = (1u32 << 12).to_le_bytes();
    payload[dictionary_size_offset..dictionary_size_offset + 4].copy_from_slice(&smaller);

    let chunk = depot4_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written = match VZipUtil::decompress(&payload, &mut destination, true) {
        Ok(written) => written,
        Err(error) => panic!("expected the VZip data to decompress, got: {error}"),
    };

    let written = usize::try_from(written).expect("a non negative written length");
    let chunk_id = match chunk.chunk_id.as_deref() {
        Some(chunk_id) => chunk_id,
        None => panic!("the depot 4 chunk must carry a chunk id"),
    };
    assert_eq!(chunk_id, &common::sha1(&destination[..written])[..]);
}

#[test]
fn vzip_cuts_matches_at_end_of_output() {
    let payload = read_decrypted_v_zip_chunk();

    let chunk = depot4_chunk();
    let uncompressed_length = chunk.uncompressed_length as usize;

    let mut expected = vec![0u8; uncompressed_length];
    match VZipUtil::decompress(&payload, &mut expected, true) {
        Ok(_) => {}
        Err(error) => panic!("expected the VZip data to decompress, got: {error}"),
    }

    // Some of these lengths end inside a match, which has to be cut short
    // rather than rejected
    for length in (uncompressed_length - 64)..uncompressed_length {
        let mut destination = vec![0u8; length];

        let written = match VZipUtil::decompress(
            &with_decompressed_size(&payload, length as i32),
            &mut destination,
            false,
        ) {
            Ok(written) => written,
            Err(error) => panic!("expected {length} bytes to decompress, got: {error}"),
        };

        assert_eq!(length as i32, written);
        assert_eq!(&expected[..length], &destination[..]);
    }
}

#[test]
fn vzip_throws_on_truncated_stream() {
    let payload = read_decrypted_v_zip_chunk();

    let chunk = depot4_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let stream_start = VZIP_STREAM_OFFSET;
    let stream_end = payload.len() - VZIP_FOOTER_LENGTH;
    let stream = &payload[stream_start..stream_end];

    let half_stream = with_stream(&payload, &stream[..stream.len() / 2]);
    match VZipUtil::decompress(&half_stream, &mut destination, false) {
        Ok(_) => panic!("expected the truncated stream to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }

    let too_short_for_range_decoder = with_stream(&payload, &stream[..4]);
    match VZipUtil::decompress(&too_short_for_range_decoder, &mut destination, false) {
        Ok(_) => panic!("expected the truncated stream to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }

    // With checksum verification, the usual default.
    match VZipUtil::decompress(&payload[..21], &mut destination, true) {
        Ok(_) => panic!("expected the short buffer to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}

#[test]
fn vzip_throws_when_output_is_larger_than_stream() {
    let chunk = depot4_chunk();
    let uncompressed_length = chunk.uncompressed_length as usize;

    let payload = with_decompressed_size(&read_decrypted_v_zip_chunk(), (uncompressed_length + 1000) as i32);
    let mut destination = vec![0u8; uncompressed_length + 1000];

    match VZipUtil::decompress(&payload, &mut destination, false) {
        Ok(_) => panic!("expected the oversized output to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}

#[test]
fn vzip_throws_on_invalid_header_footer_or_properties() {
    let payload = read_decrypted_v_zip_chunk();

    let chunk = depot4_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let expect_decompression_error = |name: &str, result: Result<i32, SteamError>| {
        match result {
            Ok(_) => panic!("expected {name} to fail"),
            Err(error) => assert!(
                matches!(error, SteamError::Decompression(_)),
                "expected a decompression error for {name}, got: {error}"
            ),
        }
    };

    let mut bad_header = payload.clone();
    bad_header[0] = b'X';
    expect_decompression_error("the header", VZipUtil::decompress(&bad_header, &mut destination, true));

    let mut bad_version = payload.clone();
    bad_version[2] = b'd';
    expect_decompression_error("the version", VZipUtil::decompress(&bad_version, &mut destination, true));

    let mut bad_footer = payload.clone();
    let last = bad_footer.len() - 1;
    bad_footer[last] = b'X';
    expect_decompression_error("the footer", VZipUtil::decompress(&bad_footer, &mut destination, true));

    let negative_size = with_decompressed_size(&payload, -1);
    expect_decompression_error("the negative size", VZipUtil::decompress(&negative_size, &mut destination, true));

    let mut bad_properties = payload.clone();
    bad_properties[VZIP_PROPERTIES_OFFSET] = 9 * 5 * 5;
    expect_decompression_error("the properties", VZipUtil::decompress(&bad_properties, &mut destination, true));

    let mut small_destination = vec![0u8; chunk.uncompressed_length as usize - 1];
    match VZipUtil::decompress(&payload, &mut small_destination, true) {
        Ok(_) => panic!("expected the small destination to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::InvalidArgument(_)),
            "expected an invalid argument error, got: {error}"
        ),
    }
}

#[test]
fn vzip_throws_on_match_before_any_data() {
    let payload = with_decompressed_size(&read_decrypted_v_zip_chunk(), 16);
    let mut destination = vec![0u8; 16];

    // The initial code decodes a match bit followed by a simple match with
    // distance 1
    let mut match_bytes = [0u8; 32];
    match_bytes[1] = 0x7F;
    match_bytes[2] = 0xFF;
    match_bytes[3] = 0xFD;

    match VZipUtil::decompress(&with_stream(&payload, &match_bytes), &mut destination, false) {
        Ok(_) => panic!("expected the match before any data to fail"),
        Err(SteamError::Decompression(message)) => assert!(
            message.contains("distance"),
            "expected the error to mention the distance, got: {message}"
        ),
        Err(error) => panic!("expected a decompression error, got: {error}"),
    }

    // The initial code decodes a match bit followed by a repeated match bit
    let mut repeated_match = [0u8; 32];
    repeated_match[1] = 0xFF;
    repeated_match[2] = 0xFF;
    repeated_match[3] = 0xFF;
    repeated_match[4] = 0xF0;

    match VZipUtil::decompress(&with_stream(&payload, &repeated_match), &mut destination, false) {
        Ok(_) => panic!("expected the repeated match to fail"),
        Err(SteamError::Decompression(message)) => assert!(
            message.contains("repeated"),
            "expected the error to mention the repeated match, got: {message}"
        ),
        Err(error) => panic!("expected a decompression error, got: {error}"),
    }
}

#[test]
fn zstd_throws_on_corrupt_frame() {
    let payload = decrypt_depot_chunk(
        &read_chunk_file("depot_1004_chunk_b4e70afde4693284936da1a67758c897a492e1fc.bin"),
        &DEPOT1004_KEY,
    );

    let chunk = depot1004_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    // Drop the middle third of the frame, keeping the VZstd header and footer intact
    let third = payload.len() / 3;
    let mut truncated = vec![0u8; payload.len() - third];
    truncated[..third].copy_from_slice(&payload[..third]);
    truncated[third..].copy_from_slice(&payload[third * 2..]);

    match VZstdUtil::decompress(&truncated, &mut destination, true) {
        Ok(_) => panic!("expected the truncated frame to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }

    let mut bad_frame_magic = payload.clone();
    bad_frame_magic[8] ^= 0xFF;

    match VZstdUtil::decompress(&bad_frame_magic, &mut destination, true) {
        Ok(_) => panic!("expected the corrupt frame to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}

#[test]
fn zstd_throws_on_short_input() {
    let payload = decrypt_depot_chunk(
        &read_chunk_file("depot_1004_chunk_b4e70afde4693284936da1a67758c897a492e1fc.bin"),
        &DEPOT1004_KEY,
    );

    let chunk = depot1004_chunk();
    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    match VZstdUtil::decompress(&payload[..22], &mut destination, true) {
        Ok(_) => panic!("expected the short input to fail"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}
