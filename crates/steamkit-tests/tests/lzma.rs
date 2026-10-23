//! LZMA, VZip and VZip delta tests.
//!
//! The CRC-32 is computed here so the test crate doesn't need another
//! dependency. The incompressible middle of the delta target comes from
//! `rand`; the exact bytes don't matter, only that the delta stays small and
//! round trips.

mod common;

use rand::{RngCore, SeedableRng};

use steamkit::error::SteamError;
use steamkit::steam::cdn::depot_chunk::DepotChunk;
use steamkit::types::depot_manifest::ChunkData;
use steamkit::util::lzma::compress::lzma::lzma_encoder::Encoder;
use steamkit::util::lzma::i_coder::{
    CoderPropID, CoderProperty, ICoder, ISetCoderProperties, IWriteCoderProperties,
};
use steamkit::util::lzma_util::LzmaUtil;
use steamkit::util::v_zip_delta_util::VZipDeltaUtil;

/// The depot key of depot 4.
const DEPOT4_KEY: [u8; 32] = [
    0x57, 0xEF, 0x98, 0xDE, 0x53, 0x88, 0xA8, 0x4C, 0xBD, 0x6B, 0xA9, 0x4D, 0xCD, 0xA3, 0x6F, 0x8B,
    0xB7, 0x49, 0x17, 0x7D, 0xAA, 0xBF, 0x35, 0x21, 0x2F, 0xEF, 0xCC, 0x09, 0x83, 0x33, 0xE4, 0xD5,
];

/// The default dictionary size of the test encoder.
const DEFAULT_DICTIONARY_SIZE: i32 = 1 << 20;

/// The CRC-32 (IEEE 802.3) checksum of the given data.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;

    for byte in data {
        crc ^= u32::from(*byte);

        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }

    !crc
}

/// The decrypted and decompressed test chunk of depot 4.
fn read_depot_chunk_contents() -> Vec<u8> {
    let chunk_data =
        common::read_data_file("depot_4_chunk_dc036a660987d8f705936d4443fedb5e1d2e5d9b.bin");

    // The test chunk of depot 4.
    let chunk = ChunkData::with_values(
        hex::decode("DC036A660987D8F705936D4443FEDB5E1D2E5D9B").expect("valid chunk id hex"),
        649235822,
        0,
        25072,
        68887,
    );

    let mut destination = vec![0u8; chunk.uncompressed_length as usize];

    let written = match DepotChunk::process(&chunk, &chunk_data, &mut destination, &DEPOT4_KEY) {
        Ok(written) => written,
        Err(error) => panic!("the depot chunk must be processed, got: {error}"),
    };

    destination.truncate(written);
    destination
}

/// An LZMA encoder with the test settings.
fn create_encoder(dictionary_size: i32) -> Encoder {
    let mut encoder = Encoder::new();

    encoder
        .set_coder_properties(
            &[
                CoderPropID::DictionarySize,
                CoderPropID::PosStateBits,
                CoderPropID::LitContextBits,
                CoderPropID::LitPosBits,
                CoderPropID::EndMarker,
            ],
            &[
                CoderProperty::Int32(dictionary_size),
                CoderProperty::Int32(2),
                CoderProperty::Int32(3),
                CoderProperty::Int32(0),
                CoderProperty::Boolean(false),
            ],
        )
        .expect("the coder properties are valid");

    encoder
}

/// A delta source: the chunk contents, cut back so the last byte is below 32.
///
/// The encoder uses zero as the byte before its first literal, while the delta
/// decoder uses the last byte of the source. Both select the same literal
/// coder when that byte is below 1 << (8 - lc).
fn create_delta_source(contents: &[u8]) -> Vec<u8> {
    let mut length = contents.len();

    while length > 0 && contents[length - 1] >= 32 {
        length -= 1;
    }

    contents[..length].to_vec()
}

/// A delta target: mostly the source, with 1000 random bytes in the middle and
/// the last byte flipped.
fn create_delta_target(source: &[u8]) -> Vec<u8> {
    let mut target = vec![0u8; source.len() + 1000];

    let head = source[..20000].to_vec();
    target[..20000].copy_from_slice(&head);

    let mut random = rand::rngs::StdRng::seed_from_u64(1950);
    random.fill_bytes(&mut target[20000..21000]);

    let tail = source[20000..].to_vec();
    target[21000..].copy_from_slice(&tail);

    let last = target.len() - 1;
    target[last] ^= 0xFF;

    target
}

/// Compresses `target` as a VZip delta against `source`.
fn create_v_zip_delta(source: &[u8], target: &[u8], dictionary_size: i32) -> Vec<u8> {
    let crc = crc32(target);

    let mut output = Vec::new();
    output.extend_from_slice(&0x5A56u16.to_le_bytes());
    output.push(b'd');
    output.extend_from_slice(&crc.to_le_bytes());

    let mut encoder = create_encoder(dictionary_size);
    encoder.set_train_size(source.len() as u32);
    encoder.write_coder_properties(&mut output);

    let mut input = Vec::with_capacity(source.len() + target.len());
    input.extend_from_slice(source);
    input.extend_from_slice(target);

    encoder
        .code(input, &mut output, -1, -1, None)
        .expect("the delta must be encoded");

    output.extend_from_slice(&crc.to_le_bytes());
    output.extend_from_slice(&(target.len() as u32).to_le_bytes());
    output.extend_from_slice(&0x767Au16.to_le_bytes());

    output
}

#[test]
fn v_zip_delta_decodes_against_source_chunk() {
    let source = create_delta_source(&read_depot_chunk_contents());
    let target = create_delta_target(&source);
    let delta = create_v_zip_delta(&source, &target, DEFAULT_DICTIONARY_SIZE);

    // Most of the target comes from the source, so the delta is much smaller than the target
    assert!(
        delta.len() < target.len() / 10,
        "Delta is {} bytes",
        delta.len()
    );

    let decompressed = match VZipDeltaUtil::decompress(&delta, &source) {
        Ok(decompressed) => decompressed,
        Err(error) => panic!("the delta must be decompressed, got: {error}"),
    };

    assert_eq!(target, decompressed);
}

#[test]
fn v_zip_delta_decodes_with_source_chunk_larger_than_dictionary() {
    const DICTIONARY_SIZE: i32 = 1 << 14;

    let source = create_delta_source(&read_depot_chunk_contents());
    assert!(source.len() > (DICTIONARY_SIZE as usize) * 2);

    // Repeats bytes from the end of the source that are within reach of the dictionary
    let mut target = vec![0u8; 10000];
    let tail = source[source.len() - target.len()..].to_vec();
    target.copy_from_slice(&tail);
    target[100] ^= 0xFF;

    let delta = create_v_zip_delta(&source, &target, DICTIONARY_SIZE);
    assert!(
        delta.len() < target.len() / 10,
        "Delta is {} bytes",
        delta.len()
    );

    let decompressed = match VZipDeltaUtil::decompress(&delta, &source) {
        Ok(decompressed) => decompressed,
        Err(error) => panic!("the delta must be decompressed, got: {error}"),
    };

    assert_eq!(target, decompressed);
}

#[test]
fn v_zip_delta_throws_with_wrong_source_chunk() {
    let source = create_delta_source(&read_depot_chunk_contents());
    let target = create_delta_target(&source);
    let delta = create_v_zip_delta(&source, &target, DEFAULT_DICTIONARY_SIZE);

    let mut wrong_source = source.clone();
    wrong_source[100] ^= 0xFF;

    // A corrupt payload is reported as a decompression failure.
    match VZipDeltaUtil::decompress(&delta, &wrong_source) {
        Ok(_) => panic!("the delta must not be decompressed against the wrong source"),
        Err(error) => assert!(
            matches!(error, SteamError::Decompression(_)),
            "expected a decompression error, got: {error}"
        ),
    }
}

#[test]
fn lzma_util_decompresses_blob() {
    let contents = read_depot_chunk_contents();

    let mut compressed = Vec::new();
    let mut encoder = create_encoder(DEFAULT_DICTIONARY_SIZE);
    encoder
        .code(contents.clone(), &mut compressed, -1, -1, None)
        .expect("the contents must be compressed");

    let mut blob = Vec::new();
    blob.extend_from_slice(&0x4C5A_4D41u32.to_be_bytes());
    blob.extend_from_slice(&(contents.len() as i32).to_le_bytes());
    blob.extend_from_slice(&(compressed.len() as i32).to_le_bytes());

    let mut encoder = create_encoder(DEFAULT_DICTIONARY_SIZE);
    encoder.write_coder_properties(&mut blob);

    blob.extend_from_slice(&compressed);

    assert!(LzmaUtil::has_lzma_header(&blob));

    let mut cursor = std::io::Cursor::new(blob);
    let mut factory = |capacity: usize| vec![0u8; capacity];

    let output = match LzmaUtil::try_decompress(&mut cursor, &mut factory) {
        Ok(output) => output,
        Err(error) => panic!("the blob must be decompressed, got: {error}"),
    };

    let output = output.expect("the blob carries the LZMA header");

    // The output is a fresh buffer, so there's no stream position to check.
    assert_eq!(contents, output);
}
