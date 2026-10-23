//! A server list provider that caches the list in a per-user directory.
//!
//! The file is `<temp dir>/steamkit/serverlist.protobuf`, in the same format
//! SteamKit2 uses.

use std::path::PathBuf;
use std::time::SystemTime;

use async_trait::async_trait;

use crate::error::SteamError;

use super::basic_server_list_proto::BasicServerListProto;
use super::i_server_list_provider::IServerListProvider;
use super::server_record::ServerRecord;

/// The name of the cache file.
const FILE_NAME: &str = "serverlist.protobuf";

/// The name of the directory that holds the cache file.
const DIRECTORY_NAME: &str = "steamkit";

/// A server list provider that caches the server list in a per-user directory.
#[derive(Clone, Debug)]
pub struct IsolatedStorageServerListProvider {
    filename: PathBuf,
}

impl IsolatedStorageServerListProvider {
    /// Creates a provider that uses `<temp dir>/steamkit/serverlist.protobuf`.
    pub fn new() -> Self {
        IsolatedStorageServerListProvider {
            filename: default_filename(),
        }
    }

    /// The path of the cache file.
    pub fn filename(&self) -> &std::path::Path {
        &self.filename
    }

    /// Reads the cached server list. Returns an empty list if the file can't
    /// be read.
    pub async fn read_server_list(&self) -> Result<Vec<ServerRecord>, SteamError> {
        let data = match tokio::fs::read(&self.filename).await {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!(
                    "IsolatedStorageServerListProvider: failed to read file {}: {}",
                    self.filename.display(),
                    error
                );
                return Ok(Vec::new());
            }
        };

        let items = BasicServerListProto::decode_list(&data)?;
        let mut servers = Vec::with_capacity(items.len());

        for item in &items {
            match item.to_server_record().await {
                Ok(record) => servers.push(record),
                Err(error) => {
                    tracing::warn!(
                        "IsolatedStorageServerListProvider: failed to load server {}: {}",
                        item.address(),
                        error
                    );
                }
            }
        }

        Ok(servers)
    }

    /// Writes the server list to the file.
    pub async fn write_server_list(&self, endpoints: &[ServerRecord]) -> Result<(), SteamError> {
        let items: Vec<BasicServerListProto> = endpoints
            .iter()
            .map(BasicServerListProto::from_server_record)
            .collect();

        let data = BasicServerListProto::encode_list(&items);

        if let Some(parent) = self.filename.parent() {
            if let Err(error) = tokio::fs::create_dir_all(parent).await {
                tracing::warn!(
                    "IsolatedStorageServerListProvider: failed to create directory {}: {}",
                    parent.display(),
                    error
                );
                return Ok(());
            }
        }

        match tokio::fs::write(&self.filename, &data).await {
            Ok(()) => Ok(()),
            Err(error) => {
                // Not fatal: just log it.
                tracing::warn!(
                    "IsolatedStorageServerListProvider: failed to write file {}: {}",
                    self.filename.display(),
                    error
                );
                Ok(())
            }
        }
    }
}

impl Default for IsolatedStorageServerListProvider {
    fn default() -> Self {
        IsolatedStorageServerListProvider::new()
    }
}

/// The default path of the cache file.
fn default_filename() -> PathBuf {
    let mut directory = std::env::temp_dir();
    if directory.as_os_str().is_empty() {
        directory = PathBuf::from(".");
    }

    directory.join(DIRECTORY_NAME).join(FILE_NAME)
}

#[async_trait]
impl IServerListProvider for IsolatedStorageServerListProvider {
    /// When the file was last written.
    async fn last_server_list_refresh(&self) -> Result<SystemTime, SteamError> {
        match tokio::fs::metadata(&self.filename).await {
            Ok(metadata) => Ok(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)),
            Err(_) => Ok(SystemTime::UNIX_EPOCH),
        }
    }

    /// Reads the cached server list.
    async fn fetch_server_list(&self) -> Result<Vec<ServerRecord>, SteamError> {
        self.read_server_list().await
    }

    /// Writes the server list to the file.
    async fn update_server_list(&self, endpoints: Vec<ServerRecord>) -> Result<(), SteamError> {
        self.write_server_list(&endpoints).await
    }
}
