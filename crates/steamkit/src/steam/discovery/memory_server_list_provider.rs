//! A server list provider that keeps the list in memory.

use std::time::SystemTime;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::error::SteamError;

use super::i_server_list_provider::IServerListProvider;
use super::server_record::ServerRecord;

/// A server list provider that uses an in-memory list.
#[derive(Debug, Default)]
pub struct MemoryServerListProvider {
    servers: RwLock<Vec<ServerRecord>>,
    last_updated: RwLock<Option<SystemTime>>,
}

impl MemoryServerListProvider {
    /// Creates a new, empty provider.
    pub fn new() -> Self {
        MemoryServerListProvider::default()
    }
}

#[async_trait]
impl IServerListProvider for MemoryServerListProvider {
    /// When the list was last updated.
    async fn last_server_list_refresh(&self) -> Result<SystemTime, SteamError> {
        Ok(self
            .last_updated
            .read()
            .await
            .unwrap_or(SystemTime::UNIX_EPOCH))
    }

    /// Returns the stored server list.
    async fn fetch_server_list(&self) -> Result<Vec<ServerRecord>, SteamError> {
        Ok(self.servers.read().await.clone())
    }

    /// Stores the server list.
    async fn update_server_list(&self, endpoints: Vec<ServerRecord>) -> Result<(), SteamError> {
        *self.servers.write().await = endpoints;
        *self.last_updated.write().await = Some(SystemTime::now());

        Ok(())
    }
}
