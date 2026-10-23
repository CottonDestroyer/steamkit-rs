//! A server list provider that stores nothing.

use std::time::SystemTime;

use async_trait::async_trait;

use crate::error::SteamError;

use super::i_server_list_provider::IServerListProvider;
use super::server_record::ServerRecord;

/// A server list provider that stores nothing, for when you fill in the server
/// list yourself.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullServerListProvider;

impl NullServerListProvider {
    /// Creates a new provider.
    pub fn new() -> Self {
        NullServerListProvider
    }
}

#[async_trait]
impl IServerListProvider for NullServerListProvider {
    /// Always the Unix epoch.
    async fn last_server_list_refresh(&self) -> Result<SystemTime, SteamError> {
        Ok(SystemTime::UNIX_EPOCH)
    }

    /// Always an empty list.
    async fn fetch_server_list(&self) -> Result<Vec<ServerRecord>, SteamError> {
        Ok(Vec::new())
    }

    /// Does nothing.
    async fn update_server_list(&self, _endpoints: Vec<ServerRecord>) -> Result<(), SteamError> {
        Ok(())
    }
}
