//! Dumping network messages to disk for debugging.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use crate::base::generated::steam_language::EMsg;
use crate::error::SteamError;
use crate::util::date_utils::DateUtils;
use crate::util::i_log_context::{DebugLogContext, ILogContext};

/// The debug log category of the NetHook listener.
const CATEGORY_NAME: &str = "NetHook";

/// Gets to see the network messages sent to and received from the CM server.
/// For debugging only; don't build logic on it.
pub trait IDebugNetworkListener: Send + Sync {
    /// Called when a packet is received from the Steam server.
    fn on_incoming_network_message(&self, msg_type: EMsg, data: &[u8]);

    /// Called when a packet is about to be sent to the Steam server.
    fn on_outgoing_network_message(&self, msg_type: EMsg, data: &[u8]);
}

/// Dumps every network message sent to and received from the CM server to a
/// file, for NetHookAnalyzer, a hex editor or your own tools.
///
/// Careful: sensitive data (like your password) can end up on disk.
pub struct NetHookNetworkListener {
    /// Where progress is logged.
    log: Arc<dyn ILogContext>,
    /// The directory messages are written to.
    log_directory: PathBuf,
    /// The number of the next message.
    message_number: AtomicU64,
}

impl fmt::Debug for NetHookNetworkListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NetHookNetworkListener")
            .field("log_directory", &self.log_directory)
            .field("message_number", &self.message_number)
            .finish()
    }
}

impl NetHookNetworkListener {
    /// Creates a listener that dumps into `nethook/<unix time>/` next to the
    /// executable.
    pub fn new(log: Option<Arc<dyn ILogContext>>) -> Result<NetHookNetworkListener, SteamError> {
        let log = log.unwrap_or_else(DebugLogContext::instance);

        let timestamp = DateUtils::date_time_to_unix_time(SystemTime::now())
            .unwrap_or_default()
            .to_string();

        let log_directory = base_directory()
            .join("nethook")
            .join(timestamp);

        std::fs::create_dir_all(&log_directory)?;

        log.log_debug(
            CATEGORY_NAME,
            &format!("Created nethook directory: {}", log_directory.display()),
        );

        Ok(NetHookNetworkListener {
            log,
            log_directory,
            message_number: AtomicU64::new(0),
        })
    }

    /// Creates a listener that dumps into the given folder.
    pub fn with_path(
        path: impl AsRef<Path>,
        log: Option<Arc<dyn ILogContext>>,
    ) -> Result<NetHookNetworkListener, SteamError> {
        let log = log.unwrap_or_else(DebugLogContext::instance);
        let path = path.as_ref();

        if !path.is_dir() {
            return Err(SteamError::NotFound(format!(
                "{} does not exist.",
                path.display()
            )));
        }

        Ok(NetHookNetworkListener {
            log,
            log_directory: path.to_path_buf(),
            message_number: AtomicU64::new(0),
        })
    }

    /// Called when a packet arrives from the server.
    pub fn on_incoming_network_message(&self, msg_type: EMsg, data: &[u8]) {
        self.log.log_debug(
            CATEGORY_NAME,
            &format!(" <- Recv'd EMsg: {:?} ({})", msg_type, msg_type.value()),
        );

        self.log_net_message("in", msg_type, data);
    }

    /// Called when a packet is about to be sent to the server.
    pub fn on_outgoing_network_message(&self, msg_type: EMsg, data: &[u8]) {
        self.log.log_debug(
            CATEGORY_NAME,
            &format!("Sent -> EMsg: {:?} ({})", msg_type, msg_type.value()),
        );

        self.log_net_message("out", msg_type, data);
    }

    /// Writes the raw packet to the dump directory.
    fn log_net_message(&self, direction: &str, msg_type: EMsg, data: &[u8]) {
        let path = self.log_directory.join(self.file_name(direction, msg_type));

        if let Err(error) = std::fs::write(&path, data) {
            self.log.log_debug(
                CATEGORY_NAME,
                &format!("Failed to write {}: {}", path.display(), error),
            );
        }
    }

    /// The file name for a message.
    fn file_name(&self, direction: &str, msg_type: EMsg) -> String {
        let number = self.message_number.fetch_add(1, Ordering::SeqCst) + 1;

        format!(
            "{number:03}_{direction}_{}_k_EMsg{msg_type:?}.bin",
            msg_type.value()
        )
    }
}

impl IDebugNetworkListener for NetHookNetworkListener {
    fn on_incoming_network_message(&self, msg_type: EMsg, data: &[u8]) {
        NetHookNetworkListener::on_incoming_network_message(self, msg_type, data);
    }

    fn on_outgoing_network_message(&self, msg_type: EMsg, data: &[u8]) {
        NetHookNetworkListener::on_outgoing_network_message(self, msg_type, data);
    }
}

/// The directory of the running executable.
fn base_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}
