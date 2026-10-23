//! Depot manifests: the list of files (and their chunks) in a depot.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, MAIN_SEPARATOR};
use std::time::UNIX_EPOCH;

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::BlockDecryptMut;
use aes::cipher::KeyInit;
use aes::Aes256;
use base64::Engine;
use cbc::cipher::KeyIvInit;
use prost::Message;
use sha1::{Digest, Sha1};
use steamkit_protos::steam::content_manifest_payload::file_mapping::ChunkData as ProtoChunkData;
use steamkit_protos::steam::content_manifest_payload::FileMapping as ProtoFileMapping;
use steamkit_protos::steam::{ContentManifestMetadata, ContentManifestPayload, ContentManifestSignature};

use crate::error::SteamError;
use crate::types::manifest::{
    date_time_from_unix_time, date_time_to_unix_time, ByteReader, Steam3Manifest,
};

/// The directory separator of the current platform.
const DIRECTORY_SEPARATOR: char = MAIN_SEPARATOR;

/// The directory separator of the current platform, as a string.
const DIRECTORY_SEPARATOR_STR: &str = if MAIN_SEPARATOR == '\\' { "\\" } else { "/" };

/// The separator Valve uses in depot manifests: the opposite of the platform one.
const ALT_DIR_CHAR: char = if MAIN_SEPARATOR == '\\' { '/' } else { '\\' };

/// The separator Valve uses in depot manifests, as a string.
const ALT_DIR_CHAR_STR: &str = if MAIN_SEPARATOR == '\\' { "/" } else { "\\" };

/// The magic value that introduces a protobuf payload section.
const PROTOBUF_PAYLOAD_MAGIC: u32 = 0x71F6_17D0;
/// The magic value that introduces a protobuf metadata section.
const PROTOBUF_METADATA_MAGIC: u32 = 0x1F48_12BE;
/// The magic value that introduces a protobuf signature section.
const PROTOBUF_SIGNATURE_MAGIC: u32 = 0x1B81_B817;
/// The magic value that marks the end of the manifest.
const PROTOBUF_ENDOFMANIFEST_MAGIC: u32 = 0x32C4_15AB;

/// A single chunk of a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkData {
    /// The SHA-1 chunk id.
    pub chunk_id: Option<Vec<u8>>,
    /// The expected Adler32 checksum.
    pub checksum: u32,
    /// The offset of the chunk in the file.
    pub offset: u64,
    /// The compressed length.
    pub compressed_length: u32,
    /// The uncompressed length.
    pub uncompressed_length: u32,
}

impl ChunkData {
    /// Creates an empty chunk.
    pub fn new() -> ChunkData {
        ChunkData::default()
    }

    /// Creates a chunk with the given values.
    pub fn with_values(
        id: Vec<u8>,
        checksum: u32,
        offset: u64,
        comp_length: u32,
        uncomp_length: u32,
    ) -> ChunkData {
        ChunkData {
            chunk_id: Some(id),
            checksum,
            offset,
            compressed_length: comp_length,
            uncompressed_length: uncomp_length,
        }
    }
}

/// A single file in a manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileData {
    /// The file name.
    pub file_name: String,
    /// SHA-1 hash of the file name.
    pub file_name_hash: Vec<u8>,
    /// The chunks the file is made of.
    pub chunks: Vec<ChunkData>,
    /// The `EDepotFileFlag` flags, as raw bits since a file can have several.
    pub flags: u32,
    /// The total size of the file.
    pub total_size: u64,
    /// SHA-1 hash of the file.
    pub file_hash: Vec<u8>,
    /// The symlink target, if it's a symlink.
    pub link_target: Option<String>,
}

impl FileData {
    /// Creates an empty file entry.
    pub fn new() -> FileData {
        FileData::default()
    }

    /// Creates a file entry with the given values.
    pub fn with_values(
        filename: &str,
        filename_hash: Vec<u8>,
        flags: u32,
        size: u64,
        hash: Vec<u8>,
        link_target: &str,
        encrypted: bool,
        num_chunks: usize,
    ) -> FileData {
        let file_name = if encrypted {
            filename.to_string()
        } else {
            filename.replace(ALT_DIR_CHAR_STR, DIRECTORY_SEPARATOR_STR)
        };

        FileData {
            file_name,
            file_name_hash: filename_hash,
            flags,
            total_size: size,
            file_hash: hash,
            chunks: Vec::with_capacity(num_chunks),
            link_target: Some(link_target.to_string()),
        }
    }
}

/// A Steam3 depot manifest, from the CDN or a `Steam/depotcache/` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepotManifest {
    files: Option<Vec<FileData>>,
    filenames_encrypted: bool,
    depot_id: u32,
    manifest_gid: u64,
    creation_time: std::time::SystemTime,
    total_uncompressed_size: u64,
    total_compressed_size: u64,
    encrypted_crc: u32,
}

impl Default for DepotManifest {
    fn default() -> DepotManifest {
        DepotManifest {
            files: None,
            filenames_encrypted: false,
            depot_id: 0,
            manifest_gid: 0,
            creation_time: UNIX_EPOCH,
            total_uncompressed_size: 0,
            total_compressed_size: 0,
            encrypted_crc: 0,
        }
    }
}

impl DepotManifest {
    /// Creates an empty manifest.
    pub fn new() -> DepotManifest {
        DepotManifest::default()
    }

    /// The files in this manifest.
    pub fn files(&self) -> Option<&[FileData]> {
        self.files.as_deref()
    }

    /// Sets the files in this manifest.
    pub fn set_files(&mut self, files: Option<Vec<FileData>>) {
        self.files = files;
    }

    /// Whether the file names are encrypted.
    pub fn filenames_encrypted(&self) -> bool {
        self.filenames_encrypted
    }

    /// Sets whether the file names are encrypted.
    pub fn set_filenames_encrypted(&mut self, filenames_encrypted: bool) {
        self.filenames_encrypted = filenames_encrypted;
    }

    /// The depot id.
    pub fn depot_id(&self) -> u32 {
        self.depot_id
    }

    /// Sets the depot id.
    pub fn set_depot_id(&mut self, depot_id: u32) {
        self.depot_id = depot_id;
    }

    /// The manifest id.
    pub fn manifest_gid(&self) -> u64 {
        self.manifest_gid
    }

    /// Sets the manifest id.
    pub fn set_manifest_gid(&mut self, manifest_gid: u64) {
        self.manifest_gid = manifest_gid;
    }

    /// When the manifest was created.
    pub fn creation_time(&self) -> std::time::SystemTime {
        self.creation_time
    }

    /// Sets when the manifest was created.
    pub fn set_creation_time(&mut self, creation_time: std::time::SystemTime) {
        self.creation_time = creation_time;
    }

    /// The total uncompressed size of all files.
    pub fn total_uncompressed_size(&self) -> u64 {
        self.total_uncompressed_size
    }

    /// Sets the total uncompressed size of all files.
    pub fn set_total_uncompressed_size(&mut self, total_uncompressed_size: u64) {
        self.total_uncompressed_size = total_uncompressed_size;
    }

    /// The total compressed size of all files.
    pub fn total_compressed_size(&self) -> u64 {
        self.total_compressed_size
    }

    /// Sets the total compressed size of all files.
    pub fn set_total_compressed_size(&mut self, total_compressed_size: u64) {
        self.total_compressed_size = total_compressed_size;
    }

    /// The CRC-32 of the encrypted manifest payload.
    pub fn encrypted_crc(&self) -> u32 {
        self.encrypted_crc
    }

    /// Sets the CRC-32 of the encrypted manifest payload.
    pub fn set_encrypted_crc(&mut self, encrypted_crc: u32) {
        self.encrypted_crc = encrypted_crc;
    }

    /// Parses a manifest from bytes.
    pub fn deserialize(data: &[u8]) -> Result<DepotManifest, SteamError> {
        let mut reader = ByteReader::new(data);
        DepotManifest::internal_deserialize(&mut reader)
    }

    /// Parses a manifest from a stream.
    pub fn deserialize_from_stream(stream: &mut dyn Read) -> Result<DepotManifest, SteamError> {
        let mut data = Vec::new();
        stream.read_to_end(&mut data)?;
        DepotManifest::deserialize(&data)
    }

    /// Loads a binary manifest from a file.
    pub async fn load_from_file(
        filename: impl AsRef<Path>,
    ) -> Result<Option<DepotManifest>, SteamError> {
        match tokio::fs::read(filename).await {
            Ok(data) => Ok(Some(DepotManifest::deserialize(&data)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Serializes the manifest to a file.
    pub async fn save_to_file(&self, filename: impl AsRef<Path>) -> Result<(), SteamError> {
        let mut buffer = Vec::new();
        self.serialize(&mut buffer)?;
        tokio::fs::write(filename, buffer).await?;
        Ok(())
    }

    /// Serializes the manifest into `output`.
    pub fn serialize(&self, output: &mut dyn Write) -> Result<(), SteamError> {
        let mut payload = ContentManifestPayload::default();
        let mut unique_chunks: HashSet<Vec<u8>> = HashSet::new();

        for file in self.files.iter().flatten() {
            let mut protofile = ProtoFileMapping::default();
            protofile.size = Some(file.total_size);
            protofile.flags = Some(file.flags);

            if self.filenames_encrypted {
                // Assume the name is unmodified.
                protofile.filename = Some(file.file_name.clone());
                protofile.sha_filename = Some(file.file_name_hash.clone());
            } else {
                let filename = file.file_name.replace('/', "\\");
                protofile.filename = Some(filename.clone());
                protofile.sha_filename = Some(
                    Sha1::digest(filename.to_lowercase().as_bytes())
                        .as_slice()
                        .to_vec(),
                );
            }

            protofile.sha_content = Some(file.file_hash.clone());

            if let Some(link_target) = file.link_target.as_ref() {
                if !link_target.trim().is_empty() {
                    protofile.linktarget = Some(link_target.clone());
                }
            }

            for chunk in &file.chunks {
                let mut protochunk = ProtoChunkData::default();
                protochunk.sha = chunk.chunk_id.clone();
                protochunk.crc = Some(chunk.checksum);
                protochunk.offset = Some(chunk.offset);
                protochunk.cb_original = Some(chunk.uncompressed_length);
                protochunk.cb_compressed = Some(chunk.compressed_length);

                protofile.chunks.push(protochunk);

                if let Some(chunk_id) = chunk.chunk_id.as_ref() {
                    unique_chunks.insert(chunk_id.clone());
                }
            }

            payload.mappings.push(protofile);
        }

        let mut metadata = ContentManifestMetadata::default();
        metadata.depot_id = Some(self.depot_id);
        metadata.gid_manifest = Some(self.manifest_gid);
        metadata.creation_time = Some(date_time_to_unix_time(self.creation_time) as u32);
        metadata.filenames_encrypted = Some(self.filenames_encrypted);
        metadata.cb_disk_original = Some(self.total_uncompressed_size);
        metadata.cb_disk_compressed = Some(self.total_compressed_size);
        metadata.unique_chunks = Some(unique_chunks.len() as u32);

        let payload_bytes = payload.encode_to_vec();

        // The payload CRC covers the length-prefixed payload.
        let mut crc_data = Vec::with_capacity(4 + payload_bytes.len());
        crc_data.extend_from_slice(&(payload_bytes.len() as i32).to_le_bytes());
        crc_data.extend_from_slice(&payload_bytes);
        let crc32 = crc32fast::hash(&crc_data);

        if self.filenames_encrypted {
            metadata.crc_encrypted = Some(crc32);
            metadata.crc_clear = Some(0);
        } else {
            metadata.crc_encrypted = Some(self.encrypted_crc);
            metadata.crc_clear = Some(crc32);
        }

        let metadata_bytes = metadata.encode_to_vec();

        // Protobuf payload
        output.write_all(&PROTOBUF_PAYLOAD_MAGIC.to_le_bytes())?;
        output.write_all(&(payload_bytes.len() as i32).to_le_bytes())?;
        output.write_all(&payload_bytes)?;

        // Protobuf metadata
        output.write_all(&PROTOBUF_METADATA_MAGIC.to_le_bytes())?;
        output.write_all(&(metadata_bytes.len() as i32).to_le_bytes())?;
        output.write_all(&metadata_bytes)?;

        // Empty signature section
        output.write_all(&PROTOBUF_SIGNATURE_MAGIC.to_le_bytes())?;
        output.write_all(&0i32.to_le_bytes())?;

        // End marker
        output.write_all(&PROTOBUF_ENDOFMANIFEST_MAGIC.to_le_bytes())?;

        Ok(())
    }

    /// Tries to decrypt the file names with the given key. Returns `false` if
    /// that fails.
    pub fn decrypt_filenames(&mut self, encryption_key: &[u8]) -> bool {
        if !self.filenames_encrypted {
            return true;
        }

        if encryption_key.len() != 32 {
            tracing::error!("Decrypt filenames used with non 32 byte key!");
            return false;
        }

        {
            let Some(files) = self.files.as_mut() else {
                tracing::error!("Files was null when attempting to decrypt filenames.");
                return false;
            };

            for file in files.iter_mut() {
                let Some(name) = decrypt_name(&file.file_name, encryption_key) else {
                    tracing::error!("Failed to decrypt the filename.");
                    return false;
                };

                file.file_name = name;

                if let Some(link_target) = file.link_target.as_ref() {
                    if !link_target.is_empty() {
                        let Some(link_name) = decrypt_name(link_target, encryption_key) else {
                            tracing::error!("Failed to decrypt the filename.");
                            return false;
                        };

                        file.link_target = Some(link_name);
                    }
                }
            }

            // Sort the files alphabetically, like Steam does.
            files.sort_by(|left, right| {
                cmp_ordinal_ignore_case(&left.file_name, &right.file_name)
            });
        }

        self.filenames_encrypted = false;
        true
    }

    /// Parses a manifest in either format.
    fn internal_deserialize(reader: &mut ByteReader<'_>) -> Result<DepotManifest, SteamError> {
        let mut payload: Option<ContentManifestPayload> = None;
        let mut metadata: Option<ContentManifestMetadata> = None;
        let mut signature: Option<ContentManifestSignature> = None;

        loop {
            let magic = reader.read_u32()?;

            if magic == PROTOBUF_ENDOFMANIFEST_MAGIC {
                break;
            }

            match magic {
                Steam3Manifest::MAGIC => {
                    let mut binary_manifest = Steam3Manifest::default();
                    binary_manifest.deserialize(reader)?;

                    let mut manifest = DepotManifest::new();
                    manifest.parse_binary_manifest(&binary_manifest);

                    let marker = reader.read_u32()?;
                    if marker != magic {
                        return Err(SteamError::Malformed(
                            "Unable to find end of message marker for depot manifest".to_string(),
                        ));
                    }

                    // Return early on purpose: a v4 manifest has no separate
                    // sections, and `parse_binary_manifest` handles it.
                    return Ok(manifest);
                }
                PROTOBUF_PAYLOAD_MAGIC => {
                    let length = reader.read_u32()? as usize;
                    let bytes = reader.read_bytes(length);
                    payload = Some(ContentManifestPayload::decode(bytes.as_slice())?);
                }
                PROTOBUF_METADATA_MAGIC => {
                    let length = reader.read_u32()? as usize;
                    let bytes = reader.read_bytes(length);
                    metadata = Some(ContentManifestMetadata::decode(bytes.as_slice())?);
                }
                PROTOBUF_SIGNATURE_MAGIC => {
                    let length = reader.read_u32()? as usize;
                    let bytes = reader.read_bytes(length);
                    signature = Some(ContentManifestSignature::decode(bytes.as_slice())?);
                }
                other => {
                    return Err(SteamError::Malformed(format!(
                        "Unrecognized magic value {other:X} in depot manifest."
                    )))
                }
            }
        }

        match (payload, metadata, signature) {
            (Some(payload), Some(metadata), Some(_)) => {
                let mut manifest = DepotManifest::new();
                manifest.parse_protobuf_manifest_metadata(&metadata);
                manifest.parse_protobuf_manifest_payload(&payload);
                Ok(manifest)
            }
            _ => Err(SteamError::Malformed(
                "Missing ContentManifest sections required for parsing depot manifest".to_string(),
            )),
        }
    }

    /// Fills this in from a v4 (binary) manifest.
    fn parse_binary_manifest(&mut self, manifest: &Steam3Manifest) {
        let filenames_encrypted = manifest.are_file_names_encrypted;
        let mut files = Vec::with_capacity(manifest.mapping.len());

        for file_mapping in &manifest.mapping {
            let mut file_data = FileData::with_values(
                file_mapping.file_name.as_deref().unwrap_or(""),
                file_mapping.hash_file_name.clone().unwrap_or_default(),
                file_mapping.flags,
                file_mapping.total_size,
                file_mapping.hash_content.clone().unwrap_or_default(),
                "",
                filenames_encrypted,
                file_mapping.chunks.len(),
            );

            for chunk in &file_mapping.chunks {
                file_data.chunks.push(ChunkData::with_values(
                    chunk.chunk_gid.clone().unwrap_or_default(),
                    chunk.checksum,
                    chunk.offset,
                    chunk.compressed_size,
                    chunk.decompressed_size,
                ));
            }

            files.push(file_data);
        }

        self.files = Some(files);
        self.filenames_encrypted = filenames_encrypted;
        self.depot_id = manifest.depot_id;
        self.manifest_gid = manifest.manifest_gid;
        self.creation_time = manifest.creation_time;
        self.total_uncompressed_size = manifest.total_uncompressed_size;
        self.total_compressed_size = manifest.total_compressed_size;
        self.encrypted_crc = manifest.encrypted_crc;
    }

    /// Fills this in from a protobuf manifest payload.
    fn parse_protobuf_manifest_payload(&mut self, payload: &ContentManifestPayload) {
        let mut files = Vec::with_capacity(payload.mappings.len());

        for file_mapping in &payload.mappings {
            let mut file_data = FileData::with_values(
                file_mapping.filename.as_deref().unwrap_or(""),
                file_mapping.sha_filename.clone().unwrap_or_default(),
                file_mapping.flags.unwrap_or(0),
                file_mapping.size.unwrap_or(0),
                file_mapping.sha_content.clone().unwrap_or_default(),
                file_mapping.linktarget.as_deref().unwrap_or(""),
                self.filenames_encrypted,
                file_mapping.chunks.len(),
            );

            for chunk in &file_mapping.chunks {
                file_data.chunks.push(ChunkData::with_values(
                    chunk.sha.clone().unwrap_or_default(),
                    chunk.crc.unwrap_or(0),
                    chunk.offset.unwrap_or(0),
                    chunk.cb_compressed.unwrap_or(0),
                    chunk.cb_original.unwrap_or(0),
                ));
            }

            files.push(file_data);
        }

        self.files = Some(files);
    }

    /// Fills this in from protobuf manifest metadata.
    fn parse_protobuf_manifest_metadata(&mut self, metadata: &ContentManifestMetadata) {
        self.filenames_encrypted = metadata.filenames_encrypted.unwrap_or(false);
        self.depot_id = metadata.depot_id.unwrap_or(0);
        self.manifest_gid = metadata.gid_manifest.unwrap_or(0);
        self.creation_time = date_time_from_unix_time(metadata.creation_time.unwrap_or(0) as u64);
        self.total_uncompressed_size = metadata.cb_disk_original.unwrap_or(0);
        self.total_compressed_size = metadata.cb_disk_compressed.unwrap_or(0);
        self.encrypted_crc = metadata.crc_encrypted.unwrap_or(0);
    }
}

/// Decrypts a single base64-encoded, encrypted file name.
fn decrypt_name(name: &str, encryption_key: &[u8]) -> Option<String> {
    use base64::prelude::BASE64_STANDARD;

    // The encrypted names have a trailing newline, which strict base64 decoding
    // rejects, so drop whitespace first.
    let encoded: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let decoded = BASE64_STANDARD.decode(encoded.as_bytes()).ok()?;

    // The first block is the encrypted IV, the rest is the encrypted name.
    if decoded.len() < 16 {
        return None;
    }

    let cipher = Aes256::new_from_slice(encryption_key).ok()?;

    let mut iv = aes::cipher::Block::<Aes256>::clone_from_slice(&decoded[..16]);
    <Aes256 as aes::cipher::BlockDecrypt>::decrypt_block(&cipher, &mut iv);

    let decryptor = cbc::Decryptor::<Aes256>::new_from_slices(encryption_key, &iv).ok()?;

    let mut buffer = decoded[16..].to_vec();
    let plaintext_len = decryptor.decrypt_padded_mut::<Pkcs7>(&mut buffer).ok()?.len();

    // Trim the trailing null byte of the plaintext.
    let mut filename_length = plaintext_len;
    if filename_length > 0 && buffer[filename_length - 1] == 0 {
        filename_length -= 1;
    }

    // ASCII is a subset of UTF-8, so replacing the raw bytes is safe.
    for byte in buffer.iter_mut() {
        if *byte == ALT_DIR_CHAR as u8 {
            *byte = DIRECTORY_SEPARATOR as u8;
        }
    }

    Some(String::from_utf8_lossy(&buffer[..filename_length]).into_owned())
}

/// Ordinal, case-insensitive string comparison.
fn cmp_ordinal_ignore_case(left: &str, right: &str) -> Ordering {
    let mut left_chars = left
        .chars()
        .map(|ch| ch.to_uppercase().next().unwrap_or(ch));
    let mut right_chars = right
        .chars()
        .map(|ch| ch.to_uppercase().next().unwrap_or(ch));

    loop {
        match (left_chars.next(), right_chars.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_char), Some(right_char)) => match left_char.cmp(&right_char) {
                Ordering::Equal => continue,
                other => return other,
            },
        }
    }
}
