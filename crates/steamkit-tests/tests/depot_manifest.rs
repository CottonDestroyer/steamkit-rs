//! `DepotManifest` tests.
//!
//! File names in a manifest use the platform's path separator, so the
//! expected names are built with [`MAIN_SEPARATOR_STR`].

mod common;

use std::path::MAIN_SEPARATOR_STR;
use std::time::{Duration, UNIX_EPOCH};

use steamkit::types::depot_manifest::DepotManifest;

/// The decryption key of depot 440.
const DEPOT440_DECRYPTION_KEY: [u8; 32] = [
    0x44, 0xCE, 0x5C, 0x52, 0x97, 0xA4, 0x15, 0xA1,
    0xA6, 0xF6, 0x9C, 0x85, 0x60, 0x37, 0xA5, 0xA2,
    0xFD, 0xD8, 0x2C, 0xD4, 0x74, 0xFA, 0x65, 0x9E,
    0xDF, 0xB4, 0xD5, 0x9B, 0x2A, 0xBC, 0x55, 0xFC,
];

/// The creation time of the depot 440 manifest: 2013-04-17T20:39:24Z.
const CREATION_TIME_UNIX_SECONDS: u64 = 1_366_231_164;

/// The magic that starts the signature section of a manifest (`0x1B81B817`,
/// little endian).
const SIGNATURE_MAGIC: [u8; 4] = [0x17, 0xB8, 0x81, 0x1B];

/// Reads one of the manifest test assets.
fn read_manifest(name: &str) -> Vec<u8> {
    common::read_data_file(name)
}

/// Finds the first occurrence of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Checks the decrypted depot 440 manifest.
fn assert_decrypted_manifest(manifest: &DepotManifest) {
    assert!(!manifest.filenames_encrypted());
    assert_eq!(440u32, manifest.depot_id());
    assert_eq!(1118032470228587934u64, manifest.manifest_gid());
    assert_eq!(825745u64, manifest.total_uncompressed_size());
    assert_eq!(43168u64, manifest.total_compressed_size());

    let files = manifest
        .files()
        .expect("the decrypted manifest holds its files");

    assert_eq!(7usize, files.len());

    assert_eq!(
        UNIX_EPOCH + Duration::from_secs(CREATION_TIME_UNIX_SECONDS),
        manifest.creation_time()
    );

    let expected_names = [
        format!("bin{MAIN_SEPARATOR_STR}dxsupport.cfg"),
        format!("bin{MAIN_SEPARATOR_STR}dxsupport.csv"),
        format!("bin{MAIN_SEPARATOR_STR}dxsupport_episodic.cfg"),
        format!("bin{MAIN_SEPARATOR_STR}dxsupport_sp.cfg"),
        format!("bin{MAIN_SEPARATOR_STR}vidcfg.bin"),
        format!("hl2{MAIN_SEPARATOR_STR}media{MAIN_SEPARATOR_STR}startupvids.txt"),
        format!("tf{MAIN_SEPARATOR_STR}media{MAIN_SEPARATOR_STR}startupvids.txt"),
    ];

    for (index, expected_name) in expected_names.iter().enumerate() {
        assert_eq!(expected_name, &files[index].file_name);
    }

    assert_eq!(0u32, files[0].flags);
    assert_eq!(398709u64, files[0].total_size);
    assert_eq!(
        hex::decode("BAC8E2657470B2EB70D6DDCD6C07004BE8738697")
            .expect("the file hash is valid hex"),
        files[2].file_hash
    );

    for file in files {
        // The name hash is the SHA-1 of the name with backslashes.
        let name_hash = common::sha1(file.file_name.replace('/', "\\").as_bytes());
        assert_eq!(name_hash.to_vec(), file.file_name_hash);

        assert!(file.link_target.is_some());
        assert_eq!(1usize, file.chunks.len());
    }

    let chunk = &files[6].chunks[0];
    assert_eq!(963249608u32, chunk.checksum);
    assert_eq!(144u32, chunk.compressed_length);
    assert_eq!(17u32, chunk.uncompressed_length);
    assert_eq!(0u64, chunk.offset);

    let expected_chunk_id =
        hex::decode("94020BDE145A521EDEC9A9424E7A90FD042481E9").expect("the chunk id is valid hex");

    assert_eq!(
        expected_chunk_id.as_slice(),
        chunk.chunk_id.as_deref().unwrap_or(&[])
    );
}

#[test]
fn parses_and_decrypts_manifest_version4() {
    let manifest_data = read_manifest("depot_440_1118032470228587934_v4.manifest");

    let mut manifest = match DepotManifest::deserialize(&manifest_data) {
        Ok(manifest) => manifest,
        Err(error) => panic!("the v4 manifest must be parsed, got: {error}"),
    };

    assert!(manifest.filenames_encrypted());
    assert_eq!(1195249848u32, manifest.encrypted_crc());

    assert!(manifest.decrypt_filenames(&DEPOT440_DECRYPTION_KEY));

    assert_decrypted_manifest(&manifest);
}

#[test]
fn parses_and_decrypts_manifest() {
    let manifest_data = read_manifest("depot_440_1118032470228587934.manifest");

    let mut manifest = match DepotManifest::deserialize(&manifest_data) {
        Ok(manifest) => manifest,
        Err(error) => panic!("the manifest must be parsed, got: {error}"),
    };

    assert!(manifest.filenames_encrypted());
    assert_eq!(1606273976u32, manifest.encrypted_crc());

    assert!(manifest.decrypt_filenames(&DEPOT440_DECRYPTION_KEY));

    assert_decrypted_manifest(&manifest);
}

#[test]
fn parses_decrypted_manifest() {
    let manifest_data = read_manifest("depot_440_1118032470228587934_decrypted.manifest");

    let manifest = match DepotManifest::deserialize(&manifest_data) {
        Ok(manifest) => manifest,
        Err(error) => panic!("the decrypted manifest must be parsed, got: {error}"),
    };

    assert_decrypted_manifest(&manifest);
}

#[test]
fn roundtrip_serializes_manifest_encrypted_manifest() {
    let manifest_data = read_manifest("depot_440_1118032470228587934.manifest");

    let manifest = match DepotManifest::deserialize(&manifest_data) {
        Ok(manifest) => manifest,
        Err(error) => panic!("the manifest must be parsed, got: {error}"),
    };

    let mut actual = Vec::new();
    if let Err(error) = manifest.serialize(&mut actual) {
        panic!("the manifest must be serialized, got: {error}");
    }

    // We are unable to write signatures, so validate everything except for the signature
    let expected = manifest_data;

    let actual_offset = find_subslice(&actual, &SIGNATURE_MAGIC)
        .unwrap_or_else(|| panic!("the serialized manifest must hold a signature section"));
    let expected_offset = find_subslice(&expected, &SIGNATURE_MAGIC)
        .unwrap_or_else(|| panic!("the manifest must hold a signature section"));

    assert!(actual_offset > 0);
    assert!(expected_offset > 0);
    assert_eq!(&expected[..expected_offset], &actual[..actual_offset]);

    let expected_signature_length = i32::from_le_bytes(
        expected[expected_offset + 4..expected_offset + 8]
            .try_into()
            .expect("the signature length is readable"),
    );
    let actual_signature_length = i32::from_le_bytes(
        actual[actual_offset + 4..actual_offset + 8]
            .try_into()
            .expect("the signature length is readable"),
    );

    assert_eq!(131, expected_signature_length);
    assert_eq!(0, actual_signature_length);

    let expected_tail = expected_offset + expected_signature_length as usize + 8;
    let actual_tail = actual_offset + 8;

    assert_eq!(&expected[expected_tail..], &actual[actual_tail..]);
}

#[test]
fn roundtrip_serializes_manifest_byte_identical() {
    let manifest_data = read_manifest("depot_440_1118032470228587934_decrypted.manifest");

    let manifest = match DepotManifest::deserialize(&manifest_data) {
        Ok(manifest) => manifest,
        Err(error) => panic!("the decrypted manifest must be parsed, got: {error}"),
    };

    let mut actual = Vec::new();
    if let Err(error) = manifest.serialize(&mut actual) {
        panic!("the manifest must be serialized, got: {error}");
    }

    assert_eq!(manifest_data, actual);
}
