//! A NetHook dump: a directory of capture files, loaded into memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use steamkit::base::generated::steam_language::EMsg;
use steamkit_protos::steam::CCredentialsGetAccountAuthSecretResponse;

use crate::nethook::error::NetHookResult;
use crate::nethook::item::NetHookItem;

/// The name of the unified messaging method that hands out the account auth
/// secret.
const GET_ACCOUNT_AUTH_SECRET: &str = "Credentials.GetAccountAuthSecret#1";

/// The captured packets of a NetHook dump.
#[derive(Clone, Debug, Default)]
pub struct NetHookDump {
    items: Vec<NetHookItem>,
    account_auth_secrets: HashMap<i32, Vec<u8>>,
}

impl NetHookDump {
    /// Creates an empty dump.
    pub fn new() -> Self {
        NetHookDump {
            items: Vec::new(),
            account_auth_secrets: HashMap::new(),
        }
    }

    /// Loads every capture file of a directory.
    pub fn load_from_directory(&mut self, directory: &Path) -> NetHookResult<()> {
        self.items.clear();

        let entries = std::fs::read_dir(directory).map_err(|source| {
            crate::nethook::error::NetHookError::Io {
                path: directory.display().to_string(),
                source,
            }
        })?;

        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .map(|extension| extension.eq_ignore_ascii_case("bin"))
                    .unwrap_or(false)
            {
                paths.push(path);
            }
        }

        paths.sort();

        for path in paths {
            self.add_item_from_file(&path)?;
        }

        Ok(())
    }

    /// The captured packets of the dump.
    pub fn items(&self) -> &[NetHookItem] {
        &self.items
    }

    /// Adds a capture file to the dump.
    pub fn add_item_from_file(&mut self, path: &Path) -> NetHookResult<Option<NetHookItem>> {
        let item = match NetHookItem::load_from_file(path)? {
            Some(item) => item,
            None => return Ok(None),
        };

        self.items.push(item.clone());

        self.capture_account_auth_secret(&item);

        Ok(Some(item))
    }

    /// Removes the capture file with the given path from the dump.
    pub fn remove_item_with_path(&mut self, path: &Path) -> Option<NetHookItem> {
        let index = self
            .items
            .iter()
            .position(|item| item.path.as_path() == path)?;

        Some(self.items.remove(index))
    }

    /// The account auth secret with the given identifier.
    pub fn account_auth_secret(&self, secret_id: i32) -> Option<&[u8]> {
        self.account_auth_secrets.get(&secret_id).map(|secret| secret.as_slice())
    }

    /// Remembers the account auth secret of a captured response.
    fn capture_account_auth_secret(&mut self, item: &NetHookItem) {
        if item.emsg != EMsg::ServiceMethodResponse {
            return;
        }

        if item.inner_message_name().as_deref() != Some(GET_ACCOUNT_AUTH_SECRET) {
            return;
        }

        let bytes = match item.read_bytes() {
            Ok(bytes) => bytes,
            Err(_) => return,
        };

        let message = match crate::nethook::message_reading::read_message(&bytes) {
            Ok(message) => message,
            Err(_) => return,
        };

        let response: CCredentialsGetAccountAuthSecretResponse =
            match message.body.decode_as() {
                Some(response) => response,
                None => return,
            };

        if let (Some(secret_id), Some(secret)) = (response.secret_id, response.secret) {
            self.account_auth_secrets.insert(secret_id, secret);
        }
    }
}
