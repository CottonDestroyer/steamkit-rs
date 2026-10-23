//! The trait for storing the CM server list between runs.

use std::time::SystemTime;

use async_trait::async_trait;

use crate::error::SteamError;

use super::server_record::ServerRecord;

/// Stores the server list between runs.
#[async_trait]
pub trait IServerListProvider: Send + Sync {
    /// When the server list was last refreshed, used to decide whether to fetch
    /// a new one from the Steam Directory. Providers that never stored a list
    /// return the Unix epoch.
    async fn last_server_list_refresh(&self) -> Result<SystemTime, SteamError>;

    /// Returns the servers this provider has.
    async fn fetch_server_list(&self) -> Result<Vec<ServerRecord>, SteamError>;

    /// Replaces the stored list of servers.
    async fn update_server_list(&self, endpoints: Vec<ServerRecord>) -> Result<(), SteamError>;

    /// Alias of [`IServerListProvider::fetch_server_list`].
    async fn load_servers(&self) -> Result<Vec<ServerRecord>, SteamError> {
        self.fetch_server_list().await
    }

    /// Alias of [`IServerListProvider::update_server_list`].
    async fn save_servers(&self, endpoints: Vec<ServerRecord>) -> Result<(), SteamError> {
        self.update_server_list(endpoints).await
    }

    /// Alias of [`IServerListProvider::fetch_server_list`].
    async fn resolve_servers(&self) -> Result<Vec<ServerRecord>, SteamError> {
        self.fetch_server_list().await
    }
}
