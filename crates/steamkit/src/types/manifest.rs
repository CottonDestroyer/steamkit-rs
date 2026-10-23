//! The old binary (v4) depot manifest format.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::SteamError;

/// Converts a Unix timestamp to a [`SystemTime`].
pub(crate) fn date_time_from_unix_time(unix_time: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(unix_time)
}

/// Converts a [`SystemTime`] to a Unix timestamp.
pub(crate) fn date_time_to_unix_time(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

/// A cursor over the raw bytes of a manifest, with little-endian reads.
pub(crate) struct ByteReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> ByteReader<'a> {
        ByteReader { data, position: 0 }
    }

    /// The current read position.
    pub(crate) fn position(&self) -> u64 {
        self.position as u64
    }

    /// Reads a little endian `uint32`.
    pub(crate) fn read_u32(&mut self) -> Result<u32, SteamError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a little endian `uint64`.
    pub(crate) fn read_u64(&mut self) -> Result<u64, SteamError> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads up to `count` bytes; fewer at the end of the data.
    pub(crate) fn read_bytes(&mut self, count: usize) -> Vec<u8> {
        let end = (self.position + count).min(self.data.len());
        let bytes = self.data[self.position..end].to_vec();
        self.position = end;
        bytes
    }

    /// Reads a null-terminated UTF-8 string.
    pub(crate) fn read_null_term_string(&mut self) -> Result<String, SteamError> {
        let start = self.position;
        let mut end = start;

        while end < self.data.len() && self.data[end] != 0 {
            end += 1;
        }

        let bytes = &self.data[start..end];
        self.position = if end < self.data.len() { end + 1 } else { end };

        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn read_exact(&mut self, count: usize) -> Result<Vec<u8>, SteamError> {
        let end = self.position + count;
        if end > self.data.len() {
            return Err(unexpected_eof());
        }

        let bytes = self.data[self.position..end].to_vec();
        self.position = end;
        Ok(bytes)
    }
}

/// Builds an "unexpected end of stream" error.
fn unexpected_eof() -> SteamError {
    SteamError::Malformed("unexpected end of stream".to_string())
}

/// A single chunk of a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Steam3ManifestChunk {
    /// The SHA-1 hash of the chunk.
    pub chunk_gid: Option<Vec<u8>>,
    pub checksum: u32,
    pub offset: u64,
    pub decompressed_size: u32,
    pub compressed_size: u32,
}

impl Steam3ManifestChunk {
    /// Reads a chunk.
    fn deserialize(reader: &mut ByteReader<'_>) -> Result<Steam3ManifestChunk, SteamError> {
        let chunk_gid = reader.read_bytes(20);

        Ok(Steam3ManifestChunk {
            chunk_gid: Some(chunk_gid),
            checksum: reader.read_u32()?,
            offset: reader.read_u64()?,
            decompressed_size: reader.read_u32()?,
            compressed_size: reader.read_u32()?,
        })
    }
}

/// A single file in a manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Steam3ManifestFileMapping {
    pub file_name: Option<String>,
    pub total_size: u64,
    /// The `EDepotFileFlag` flags, as raw bits since a file can have several.
    pub flags: u32,
    pub hash_content: Option<Vec<u8>>,
    pub hash_file_name: Option<Vec<u8>>,
    pub num_chunks: u32,
    pub chunks: Vec<Steam3ManifestChunk>,
}

impl Steam3ManifestFileMapping {
    /// Reads a file entry.
    fn deserialize(reader: &mut ByteReader<'_>) -> Result<Steam3ManifestFileMapping, SteamError> {
        let file_name = reader.read_null_term_string()?;
        let total_size = reader.read_u64()?;
        let flags = reader.read_u32()?;
        let hash_content = reader.read_bytes(20);
        let hash_file_name = reader.read_bytes(20);
        let num_chunks = reader.read_u32()?;

        let mut chunks = Vec::with_capacity(num_chunks as usize);
        for _ in 0..num_chunks {
            chunks.push(Steam3ManifestChunk::deserialize(reader)?);
        }

        Ok(Steam3ManifestFileMapping {
            file_name: Some(file_name),
            total_size,
            flags,
            hash_content: Some(hash_content),
            hash_file_name: Some(hash_file_name),
            num_chunks,
            chunks,
        })
    }
}

/// A binary (v4) Steam3 manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Steam3Manifest {
    pub magic: u32,
    pub version: u32,
    pub depot_id: u32,
    pub manifest_gid: u64,
    pub creation_time: SystemTime,
    pub are_file_names_encrypted: bool,
    pub total_uncompressed_size: u64,
    pub total_compressed_size: u64,
    pub chunk_count: u32,
    pub file_entry_count: u32,
    pub file_mapping_size: u32,
    pub encrypted_crc: u32,
    pub decrypted_crc: u32,
    pub flags: u32,
    pub mapping: Vec<Steam3ManifestFileMapping>,
}

impl Default for Steam3Manifest {
    fn default() -> Steam3Manifest {
        Steam3Manifest {
            magic: 0,
            version: 0,
            depot_id: 0,
            manifest_gid: 0,
            creation_time: UNIX_EPOCH,
            are_file_names_encrypted: false,
            total_uncompressed_size: 0,
            total_compressed_size: 0,
            chunk_count: 0,
            file_entry_count: 0,
            file_mapping_size: 0,
            encrypted_crc: 0,
            decrypted_crc: 0,
            flags: 0,
            mapping: Vec::new(),
        }
    }
}

impl Steam3Manifest {
    /// The magic number at the start of the manifest.
    pub const MAGIC: u32 = 0x1634_9781;

    /// The only version we understand.
    const CURRENT_VERSION: u32 = 4;

    /// Reads the manifest.
    pub(crate) fn deserialize(&mut self, reader: &mut ByteReader<'_>) -> Result<(), SteamError> {
        self.version = reader.read_u32()?;

        if self.version != Self::CURRENT_VERSION {
            return Err(SteamError::Malformed(format!(
                "Only version {} is supported.",
                Self::CURRENT_VERSION
            )));
        }

        self.depot_id = reader.read_u32()?;

        self.manifest_gid = reader.read_u64()?;
        self.creation_time = date_time_from_unix_time(reader.read_u32()? as u64);

        self.are_file_names_encrypted = reader.read_u32()? != 0;

        self.total_uncompressed_size = reader.read_u64()?;
        self.total_compressed_size = reader.read_u64()?;

        self.chunk_count = reader.read_u32()?;

        self.file_entry_count = reader.read_u32()?;
        self.file_mapping_size = reader.read_u32()?;

        self.encrypted_crc = reader.read_u32()?;
        self.decrypted_crc = reader.read_u32()?;

        self.flags = reader.read_u32()?;

        let mut mapping = Vec::new();
        let mut remaining = self.file_mapping_size;

        while remaining > 0 {
            let start = reader.position();

            let file_mapping = Steam3ManifestFileMapping::deserialize(reader)?;
            mapping.push(file_mapping);

            remaining = remaining.wrapping_sub((reader.position() - start) as u32);
        }

        self.mapping = mapping;

        Ok(())
    }
}
