//! Downloading game content (manifests and chunks) from Steam's CDN.

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::Url;

use crate::error::SteamError;
use crate::steam::steam_client::configuration::steam_configuration::HttpClientPurpose;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::depot_manifest::{ChunkData, DepotManifest};

use super::depot_chunk::DepotChunk;
use super::server::{ConnectionProtocol, Server};

/// The default timeout to use when making requests, in milliseconds.
static REQUEST_TIMEOUT: AtomicU64 = AtomicU64::new(10_000);

/// The default timeout to use when reading the response body, in milliseconds.
static RESPONSE_BODY_TIMEOUT: AtomicU64 = AtomicU64::new(60_000);

/// The version of the depot manifest format that is requested.
const MANIFEST_VERSION: u32 = 5;

/// Downloads game content from Steam's content servers.
#[derive(Clone, Debug)]
pub struct Client {
    http_client: reqwest::Client,
}

impl Client {
    /// Creates a client that uses the given HTTP client.
    pub fn new(http_client: reqwest::Client) -> Self {
        Client { http_client }
    }

    /// Creates a client using the HTTP client the Steam client's configuration
    /// provides for CDN requests.
    pub fn from_steam_client(steam_client: &SteamClient) -> Self {
        let factory = steam_client.configuration().http_client_factory().clone();

        Client::new(factory(HttpClientPurpose::CDN))
    }

    /// Creates a client with a default HTTP client.
    pub fn with_default_client() -> Result<Self, SteamError> {
        let http_client = reqwest::Client::builder()
            .build()
            .map_err(|error| SteamError::Connection(error.to_string()))?;

        Ok(Client::new(http_client))
    }

    /// Default timeout to use when making requests.
    pub fn request_timeout() -> Duration {
        Duration::from_millis(REQUEST_TIMEOUT.load(Ordering::Relaxed))
    }

    /// Sets the default timeout to use when making requests.
    pub fn set_request_timeout(value: Duration) {
        REQUEST_TIMEOUT.store(value.as_millis() as u64, Ordering::Relaxed);
    }

    /// Default timeout to use when reading the response body.
    pub fn response_body_timeout() -> Duration {
        Duration::from_millis(RESPONSE_BODY_TIMEOUT.load(Ordering::Relaxed))
    }

    /// Sets the default timeout to use when reading the response body.
    pub fn set_response_body_timeout(value: Duration) {
        RESPONSE_BODY_TIMEOUT.store(value.as_millis() as u64, Ordering::Relaxed);
    }

    /// Downloads the depot manifest specified by the given manifest ID, and optionally
    /// decrypts the manifest's filenames if the depot decryption key has been provided.
    ///
    /// * `depot_id` - the id of the depot being accessed.
    /// * `manifest_id` - the unique identifier of the manifest to be downloaded.
    /// * `manifest_request_code` - the manifest request code for the manifest that is
    ///   being downloaded.
    /// * `server` - the content server to connect to.
    /// * `depot_key` - the depot decryption key for the depot that will be downloaded.
    ///   This is used for decrypting filenames (if needed) in depot manifests.
    /// * `proxy_server` - optional content server marked as `UseAsProxy` which
    ///   transforms the request.
    /// * `cdn_auth_token` - CDN auth token for CDN content server endpoints if
    ///   necessary.
    pub async fn download_manifest_async(
        &self,
        depot_id: u32,
        manifest_id: u64,
        manifest_request_code: u64,
        server: &Server,
        depot_key: Option<&[u8]>,
        proxy_server: Option<&Server>,
        cdn_auth_token: Option<&str>,
    ) -> Result<DepotManifest, SteamError> {
        let url = if manifest_request_code > 0 {
            format!(
                "depot/{depot_id}/manifest/{manifest_id}/{MANIFEST_VERSION}/{manifest_request_code}"
            )
        } else {
            format!("depot/{depot_id}/manifest/{manifest_id}/{MANIFEST_VERSION}")
        };

        let built_url = Self::build_command(server, &url, cdn_auth_token, proxy_server)?;

        let response = self
            .send_request(self.http_client.get(built_url))
            .await
            .map_err(|error| {
                tracing::debug!("Failed to download manifest: {error}");
                error
            })?;

        let body = Self::read_body(response).await?;

        // Decompress the zipped manifest data.
        let manifest_data = extract_first_zip_entry(&body)?;

        let mut depot_manifest = DepotManifest::deserialize(&manifest_data)?;

        if let Some(depot_key) = depot_key {
            // If we have the depot key, decrypt the manifest filenames.
            depot_manifest.decrypt_filenames(depot_key);
        }

        Ok(depot_manifest)
    }

    /// Downloads the specified depot chunk, and optionally processes the chunk and
    /// verifies the checksum if the depot decryption key has been provided.
    ///
    /// This function will also validate the length of the downloaded chunk with the
    /// value of `ChunkData::compressed_length`, if it has been assigned a value.
    ///
    /// * `depot_id` - the id of the depot being accessed.
    /// * `chunk` - the chunk to download. This value should come from a manifest
    ///   downloaded with [`Client::download_manifest_async`].
    /// * `server` - the content server to connect to.
    /// * `destination` - the buffer to receive the chunk data. If `depot_key` is
    ///   provided, this will be the decompressed buffer. Allocate a buffer that is equal
    ///   or longer than `ChunkData::uncompressed_length`.
    /// * `depot_key` - the depot decryption key for the depot that will be downloaded.
    ///   This is used to process the chunk data.
    /// * `proxy_server` - optional content server marked as `UseAsProxy` which
    ///   transforms the request.
    /// * `cdn_auth_token` - CDN auth token for CDN content server endpoints if
    ///   necessary.
    ///
    /// Returns the total number of bytes written to `destination`.
    pub async fn download_depot_chunk_async(
        &self,
        depot_id: u32,
        chunk: &ChunkData,
        server: &Server,
        destination: &mut [u8],
        depot_key: Option<&[u8]>,
        proxy_server: Option<&Server>,
        cdn_auth_token: Option<&str>,
    ) -> Result<usize, SteamError> {
        let chunk_id = chunk.chunk_id.as_ref().ok_or_else(|| {
            SteamError::InvalidArgument("Chunk must have a ChunkID.".to_string())
        })?;

        match depot_key {
            None => {
                if destination.len() < chunk.compressed_length as usize {
                    return Err(SteamError::InvalidArgument(format!(
                        "The destination buffer must be longer than the chunk CompressedLength ({}) (since no depot key was provided).",
                        chunk.compressed_length
                    )));
                }
            }
            Some(_) => {
                if destination.len() < chunk.uncompressed_length as usize {
                    return Err(SteamError::InvalidArgument(format!(
                        "The destination buffer must be longer than the chunk UncompressedLength ({}).",
                        chunk.uncompressed_length
                    )));
                }
            }
        }

        let chunk_id = hex::encode(chunk_id);
        let url = format!("depot/{depot_id}/chunk/{chunk_id}");

        let request = if Self::use_lancache_server() {
            Self::build_lancache_request(&self.http_client, server, &url, cdn_auth_token)?
        } else {
            let built_url = Self::build_command(server, &url, cdn_auth_token, proxy_server)?;
            self.http_client.get(built_url)
        };

        let response = self.send_request(request).await.map_err(|error| {
            tracing::debug!("Failed to download a depot chunk: {error}");
            error
        })?;

        let mut content_length = chunk.compressed_length as usize;

        if let Some(length) = response.content_length() {
            content_length = length as usize;

            // Only check the length if the chunk has one.
            if chunk.compressed_length > 0 && content_length != chunk.compressed_length as usize {
                return Err(SteamError::Malformed(format!(
                    "Content-Length mismatch for depot chunk! (was {content_length}, but should be {})",
                    chunk.compressed_length
                )));
            }
        } else if content_length > 0 {
            tracing::debug!(
                "Response does not have Content-Length, falling back to chunk.CompressedLength."
            );
        } else {
            return Err(SteamError::Other(
                "Response does not have Content-Length and chunk.CompressedLength is not set."
                    .to_string(),
            ));
        }

        let body = Self::read_body(response).await?;

        if body.len() != content_length {
            return Err(SteamError::Malformed(format!(
                "Length mismatch after downloading depot chunk! (was {}, but should be {content_length})",
                body.len()
            )));
        }

        match depot_key {
            // No depot key: use the data as-is.
            None => {
                destination[..content_length].copy_from_slice(&body[..content_length]);

                Ok(content_length)
            }
            // Decrypt and decompress the chunk.
            Some(depot_key) => DepotChunk::process(chunk, &body[..content_length], destination, depot_key),
        }
    }

    /// Builds the request URL for a CDN command.
    ///
    /// * `server` - the content server to connect to.
    /// * `command` - the command (path) to request.
    /// * `query` - the query string of the request, for example a CDN auth token.
    /// * `proxy_server` - optional content server marked as `UseAsProxy` which
    ///   transforms the request.
    pub fn build_command(
        server: &Server,
        command: &str,
        query: Option<&str>,
        proxy_server: Option<&Server>,
    ) -> Result<Url, SteamError> {
        let mut scheme = match server.protocol {
            ConnectionProtocol::Http => "http",
            ConnectionProtocol::Https => "https",
        };
        let mut host = server.v_host.as_deref().ok_or_else(|| {
            SteamError::InvalidArgument("The server does not have a virtual host.".to_string())
        })?;
        let mut port = server.port;
        let mut path = format!("/{}", command.trim_start_matches('/'));

        if let Some(proxy_server) = proxy_server {
            if proxy_server.use_as_proxy {
                if let Some(path_template) = proxy_server.proxy_request_path_template.as_deref() {
                    path = path_template.replace("%host%", host).replace("%path%", &path);
                    scheme = match proxy_server.protocol {
                        ConnectionProtocol::Http => "http",
                        ConnectionProtocol::Https => "https",
                    };
                    host = proxy_server.v_host.as_deref().ok_or_else(|| {
                        SteamError::InvalidArgument(
                            "The proxy server does not have a virtual host.".to_string(),
                        )
                    })?;
                    port = proxy_server.port;
                }
            }
        }

        if !path.starts_with('/') {
            path.insert(0, '/');
        }

        let mut url = Url::parse(&format!("{scheme}://{host}:{port}{path}"))
            .map_err(|error| SteamError::InvalidArgument(format!("Invalid CDN url: {error}")))?;

        if let Some(query) = query {
            if !query.is_empty() {
                url.set_query(Some(query));
            }
        }

        Ok(url)
    }

    /// Sends a request and waits for the response headers.
    async fn send_request(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SteamError> {
        let timeout = Self::request_timeout();

        match tokio::time::timeout(timeout, request.send()).await {
            Ok(result) => {
                let response = result.map_err(|error| map_reqwest_error(error, timeout))?;

                if !response.status().is_success() {
                    return Err(SteamError::Other(format!(
                        "Response status code does not indicate success: {}",
                        response.status()
                    )));
                }

                Ok(response)
            }
            Err(_) => Err(SteamError::Timeout(timeout)),
        }
    }

    /// Reads the whole response body.
    async fn read_body(response: reqwest::Response) -> Result<bytes::Bytes, SteamError> {
        let timeout = Self::response_body_timeout();

        match tokio::time::timeout(timeout, response.bytes()).await {
            Ok(result) => result.map_err(|error| map_reqwest_error(error, timeout)),
            Err(_) => Err(SteamError::Timeout(timeout)),
        }
    }
}

/// Maps a reqwest error to a [`SteamError`].
fn map_reqwest_error(error: reqwest::Error, timeout: Duration) -> SteamError {
    if error.is_timeout() {
        SteamError::Timeout(timeout)
    } else if error.is_connect() {
        SteamError::Connection(error.to_string())
    } else {
        SteamError::Other(error.to_string())
    }
}

/// Extracts the first entry of a ZIP archive. Reads the central directory
/// directly and inflates the entry with [`flate2`].
fn extract_first_zip_entry(data: &[u8]) -> Result<Vec<u8>, SteamError> {
    const CENTRAL_FILE_HEADER: u32 = 0x0201_4b50;
    const LOCAL_FILE_HEADER: u32 = 0x0403_4b50;

    let eocd = find_end_of_central_directory(data)?;

    let entry_count = read_u16(data, eocd + 10)? as usize;
    let central_offset = read_u32(data, eocd + 16)? as usize;

    if entry_count == 0 {
        return Err(SteamError::Malformed(
            "Expected the zip to contain only one file".to_string(),
        ));
    }

    if read_u32(data, central_offset)? != CENTRAL_FILE_HEADER {
        return Err(SteamError::Malformed(
            "Invalid zip central directory header".to_string(),
        ));
    }

    let method = read_u16(data, central_offset + 10)?;
    let compressed_size = read_u32(data, central_offset + 20)? as usize;
    let uncompressed_size = read_u32(data, central_offset + 24)? as usize;
    let local_offset = read_u32(data, central_offset + 42)? as usize;

    if read_u32(data, local_offset)? != LOCAL_FILE_HEADER {
        return Err(SteamError::Malformed(
            "Invalid zip local file header".to_string(),
        ));
    }

    let local_name_length = read_u16(data, local_offset + 26)? as usize;
    let local_extra_length = read_u16(data, local_offset + 28)? as usize;

    let data_start = local_offset + 30 + local_name_length + local_extra_length;
    let data_end = if compressed_size == 0 {
        data.len()
    } else {
        data_start + compressed_size
    };

    if data_end > data.len() {
        return Err(SteamError::Malformed(
            "Truncated zip archive".to_string(),
        ));
    }

    let compressed = &data[data_start..data_end];

    match method {
        0 => Ok(compressed.to_vec()),
        8 => {
            let mut output = Vec::with_capacity(uncompressed_size);
            let mut decoder = flate2::read::DeflateDecoder::new(compressed);

            decoder
                .read_to_end(&mut output)
                .map_err(|error| SteamError::Decompression(format!("Failed to inflate zip entry: {error}")))?;

            Ok(output)
        }
        other => Err(SteamError::Decompression(format!(
            "Unsupported zip compression method {other}"
        ))),
    }
}

/// Finds the end of central directory record of a ZIP archive.
fn find_end_of_central_directory(data: &[u8]) -> Result<usize, SteamError> {
    const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;

    if data.len() < 22 {
        return Err(SteamError::Malformed(
            "The zip archive is too small".to_string(),
        ));
    }

    let start = data.len().saturating_sub(22 + 0xFFFF);
    let mut index = data.len() - 22;

    loop {
        if read_u32(data, index)? == END_OF_CENTRAL_DIRECTORY {
            return Ok(index);
        }

        if index == start {
            break;
        }

        index -= 1;
    }

    Err(SteamError::Malformed(
        "Zip end of central directory record not found".to_string(),
    ))
}

/// Reads a little endian `u16` from the given buffer.
fn read_u16(data: &[u8], offset: usize) -> Result<u16, SteamError> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| SteamError::Malformed("Truncated zip archive".to_string()))?;

    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Reads a little endian `u32` from the given buffer.
fn read_u32(data: &[u8], offset: usize) -> Result<u32, SteamError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| SteamError::Malformed("Truncated zip archive".to_string()))?;

    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
