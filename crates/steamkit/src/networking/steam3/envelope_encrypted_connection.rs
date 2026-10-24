//! Steam's channel encryption, layered on top of another connection.
//!
//! It answers the server's `ChannelEncryptRequest` with a fresh 32-byte session
//! key, RSA-OAEP-SHA1 encrypted with the universe's public key. After that every
//! message is encrypted with AES-256-CBC and authenticated with HMAC-SHA1 (see
//! [`NetFilterEncryptionWithHMAC`]).
//!
//! # Handshake wire format
//!
//! The request and the result use the plain `MsgHdr` header (20 bytes), the
//! response additionally carries a `MsgChannelEncryptResponse` body:
//!
//! ```text
//! offset  size  field
//! 0       4     EMsg (i32, little endian)
//! 4       8     target job id (u64, u64::MAX)
//! 12      8     source job id (u64, u64::MAX)
//! 20      4     protocol version (u32, 1)
//! 24      4     universe (i32)          -- request only
//! 28      4     key size (u32, 128)     -- response only
//! 32      128   RSA-OAEP-SHA1 blob     -- response only
//! 160     4     CRC32 of the blob      -- response only
//! 164     4     zero                   -- response only
//! ```

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use rand::RngCore;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Oaep, RsaPublicKey};
use sha1::Sha1;
use tokio::sync::{broadcast, mpsc};

use crate::base::generated::steam_language::{EMsg, EResult, EUniverse};
use crate::error::SteamError;
use crate::util::key_dictionary::KeyDictionary;

use super::disconnected_event_args::DisconnectedEventArgs;
use super::i_connection::{
    Connection, ConnectionEvent, ConnectionEventSenders, ConnectionEvents, IConnection,
};
use super::net_filter_encryption_with_hmac::NetFilterEncryptionWithHMAC;
use super::net_msg_event_args::NetMsgEventArgs;
use super::protocol_type::ProtocolTypes;

/// The size of the plain `MsgHdr` header.
const MSG_HDR_LENGTH: usize = 20;

/// The bit that marks a message as protobuf encoded.
const PROTO_MASK: u32 = 0x8000_0000;

/// The protocol version of the channel encryption handshake.
const PROTOCOL_VERSION: u32 = 1;

/// The size of the RSA blob in the channel encryption response.
const KEY_SIZE: u32 = 128;

/// The length of the session key.
const SESSION_KEY_LENGTH: usize = 32;

/// The minimum length of the random challenge sent by the server.
const MIN_CHALLENGE_LENGTH: usize = 16;

/// The state of the channel encryption handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncryptionState {
    Disconnected,
    Connected,
    Challenged,
    Encrypted,
}

/// Gets to see the unencrypted handshake messages.
pub use crate::util::debug_network_listener::IDebugNetworkListener as DebugNetworkListener;

struct EnvelopeShared {
    inner: Arc<dyn Connection>,
    events: ConnectionEventSenders,
    universe: EUniverse,
    state: Mutex<EncryptionState>,
    encryption: Mutex<Option<Arc<NetFilterEncryptionWithHMAC>>>,
    debug_network_listener: Option<Arc<dyn DebugNetworkListener>>,
    forwarders_started: AtomicBool,
}

/// A connection that runs Steam's channel encryption handshake on top of
/// another connection.
pub struct EnvelopeEncryptedConnection {
    shared: Arc<EnvelopeShared>,
}

impl std::fmt::Debug for EnvelopeEncryptedConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvelopeEncryptedConnection").finish_non_exhaustive()
    }
}

impl EnvelopeEncryptedConnection {
    /// Creates a new envelope connection wrapping `inner`.
    pub fn new(inner: Arc<dyn Connection>, universe: EUniverse) -> Self {
        Self::with_debug_network_listener(inner, universe, None)
    }

    /// Creates a new envelope connection that reports the handshake messages to
    /// `debug_network_listener`.
    pub fn with_debug_network_listener(
        inner: Arc<dyn Connection>,
        universe: EUniverse,
        debug_network_listener: Option<Arc<dyn DebugNetworkListener>>,
    ) -> Self {
        let shared = Arc::new(EnvelopeShared {
            inner,
            events: ConnectionEventSenders::with_default_capacity(),
            universe,
            state: Mutex::new(EncryptionState::Disconnected),
            encryption: Mutex::new(None),
            debug_network_listener,
            forwarders_started: AtomicBool::new(false),
        });

        Self { shared }
    }

    /// Starts the tasks that forward the inner connection's events. Done on the
    /// first `connect` rather than in `new`, so creating a connection doesn't
    /// need a tokio runtime.
    fn spawn_forwarders(&self) {
        if self
            .shared
            .forwarders_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        spawn_forwarders(self.shared.clone(), self.shared.inner.clone());
    }

    /// Subscribes to the `NetMsgReceived` event.
    pub fn subscribe_net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs> {
        self.shared.events.subscribe_net_msg_received()
    }

    /// Subscribes to the `Connected` event.
    pub fn subscribe_connected(&self) -> broadcast::Receiver<()> {
        self.shared.events.subscribe_connected()
    }

    /// Subscribes to the `Disconnected` event.
    pub fn subscribe_disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs> {
        self.shared.events.subscribe_disconnected()
    }
}

#[async_trait]
impl IConnection for EnvelopeEncryptedConnection {
    fn current_end_point(&self) -> Option<SocketAddr> {
        self.shared.inner.current_end_point()
    }

    fn protocol_types(&self) -> ProtocolTypes {
        self.shared.inner.protocol_types()
    }

    async fn connect(&self, end_point: SocketAddr, timeout: Duration) -> Result<(), SteamError> {
        self.spawn_forwarders();

        self.shared.inner.connect(end_point, timeout).await
    }

    async fn disconnect(&self, user_initiated: bool) {
        self.shared.inner.disconnect(user_initiated).await;
    }

    async fn send(&self, data: Bytes) -> Result<(), SteamError> {
        let state = lock(&self.shared.state).clone();

        if state != EncryptionState::Encrypted {
            return self.shared.inner.send(data).await;
        }

        let encryption = lock(&self.shared.encryption).clone();

        let Some(encryption) = encryption else {
            tracing::debug!(
                "EnvelopeEncryptedConnection: Attempting to send client data when not connected."
            );
            return Ok(());
        };

        let encrypted = encryption.process_outgoing(&data)?;
        self.shared.inner.send(Bytes::from(encrypted)).await
    }

    fn local_ip(&self) -> Option<IpAddr> {
        self.shared.inner.local_ip()
    }
}

impl ConnectionEvents for EnvelopeEncryptedConnection {
    fn events(&self) -> mpsc::UnboundedReceiver<ConnectionEvent> {
        self.shared.events.subscribe_events()
    }

    fn net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs> {
        self.shared.events.subscribe_net_msg_received()
    }

    fn connected(&self) -> broadcast::Receiver<()> {
        self.shared.events.subscribe_connected()
    }

    fn disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs> {
        self.shared.events.subscribe_disconnected()
    }
}

/// Forwards the inner connection's events.
///
/// One task handles all three events, in order. That matters: the server sends
/// `ChannelEncryptRequest` as soon as the socket opens, and it's only accepted
/// once `Connected` has moved the state out of `Disconnected`.
///
/// The task only holds a weak reference to the connection. A strong one would
/// make a cycle (connection -> inner connection -> event sender) and leak it.
fn spawn_forwarders(shared: Arc<EnvelopeShared>, inner: Arc<dyn Connection>) {
    // Subscribe before `connect` is called so no event is missed.
    let mut events = inner.events();
    let weak = Arc::downgrade(&shared);
    drop(shared);

    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let Some(shared) = weak.upgrade() else {
                break;
            };

            match event {
                ConnectionEvent::NetMsgReceived(args) => {
                    handle_net_msg_received(&shared, args).await;
                }
                ConnectionEvent::Connected => {
                    *lock(&shared.state) = EncryptionState::Connected;
                }
                ConnectionEvent::Disconnected(args) => {
                    *lock(&shared.state) = EncryptionState::Disconnected;
                    *lock(&shared.encryption) = None;
                    shared
                        .events
                        .emit_disconnected(DisconnectedEventArgs::new(args.user_initiated));
                }
            }
        }
    });
}

async fn handle_net_msg_received(shared: &Arc<EnvelopeShared>, args: NetMsgEventArgs) {
    let state = lock(&shared.state).clone();

    if state == EncryptionState::Encrypted {
        let encryption = lock(&shared.encryption).clone();

        let Some(encryption) = encryption else {
            return;
        };

        match encryption.process_incoming(&args.data) {
            Ok(plain_text) => {
                shared.events.emit_net_msg_received(args.with_data(plain_text));
            }
            Err(err) => {
                tracing::debug!(
                    "EnvelopeEncryptedConnection: Unable to decrypt incoming packet: {err}"
                );
            }
        }

        return;
    }

    let Some(msg_type) = parse_msg_type(&args.data) else {
        tracing::debug!(
            "EnvelopeEncryptedConnection: Failed to parse message during channel setup, shutting down connection"
        );
        shared.inner.disconnect(false).await;
        return;
    };

    if let Some(listener) = &shared.debug_network_listener {
        listener.on_incoming_network_message(msg_type, &args.data);
    }

    if !is_expected_emsg(state, msg_type) {
        tracing::debug!("EnvelopeEncryptedConnection: Rejected EMsg during channel setup");
        return;
    }

    match msg_type {
        EMsg::ChannelEncryptRequest => handle_encrypt_request(shared, &args.data).await,
        EMsg::ChannelEncryptResult => handle_encrypt_result(shared, &args.data).await,
        _ => {}
    }
}

async fn handle_encrypt_request(shared: &Arc<EnvelopeShared>, data: &[u8]) {
    if data.len() < MSG_HDR_LENGTH + 8 {
        tracing::debug!(
            "EnvelopeEncryptedConnection: Encryption handshake has a payload that is less than 16 bytes"
        );
        shared.inner.disconnect(false).await;
        return;
    }

    let protocol_version = read_u32(data, MSG_HDR_LENGTH);
    let universe =
        EUniverse::from_value(read_i32(data, MSG_HDR_LENGTH + 4)).unwrap_or(EUniverse::Invalid);

    tracing::debug!(
        "EnvelopeEncryptedConnection: Got encryption request. Universe: {universe:?} Protocol ver: {protocol_version}"
    );

    // Only logged (SteamKit2 does the same); a mismatch doesn't abort the handshake.
    if protocol_version != PROTOCOL_VERSION {
        tracing::debug!(
            "EnvelopeEncryptedConnection: Encryption handshake protocol version mismatch!"
        );
    }

    if universe != shared.universe {
        tracing::debug!(
            "EnvelopeEncryptedConnection: Expected universe {:?} but server reported universe {universe:?}",
            shared.universe
        );
    }

    let random_challenge = &data[MSG_HDR_LENGTH + 8..];
    if random_challenge.len() < MIN_CHALLENGE_LENGTH {
        tracing::debug!(
            "EnvelopeEncryptedConnection: Encryption handshake has a payload that is less than 16 bytes"
        );
        shared.inner.disconnect(false).await;
        return;
    }

    let Some(public_key) = public_key_for_universe(universe) else {
        tracing::debug!(
            "EnvelopeEncryptedConnection: HandleEncryptRequest got request for invalid universe!"
        );
        shared.inner.disconnect(false).await;
        return;
    };

    let mut temp_session_key = [0u8; SESSION_KEY_LENGTH];
    rand::thread_rng().fill_bytes(&mut temp_session_key);

    let mut blob_to_encrypt = Vec::with_capacity(SESSION_KEY_LENGTH + random_challenge.len());
    blob_to_encrypt.extend_from_slice(&temp_session_key);
    blob_to_encrypt.extend_from_slice(random_challenge);

    let encrypted_handshake_blob = match rsa_encrypt_oaep_sha1(public_key, &blob_to_encrypt) {
        Ok(blob) => blob,
        Err(err) => {
            tracing::debug!("EnvelopeEncryptedConnection: {err}");
            shared.inner.disconnect(false).await;
            return;
        }
    };

    let key_crc = crc32fast::hash(&encrypted_handshake_blob);

    let encryption = match NetFilterEncryptionWithHMAC::new(&temp_session_key) {
        Ok(encryption) => encryption,
        Err(err) => {
            tracing::debug!("EnvelopeEncryptedConnection: {err}");
            shared.inner.disconnect(false).await;
            return;
        }
    };

    let serialized = build_channel_encrypt_response(&encrypted_handshake_blob, key_crc);

    if let Some(listener) = &shared.debug_network_listener {
        listener.on_outgoing_network_message(EMsg::ChannelEncryptResponse, &serialized);
    }

    *lock(&shared.encryption) = Some(Arc::new(encryption));
    *lock(&shared.state) = EncryptionState::Challenged;

    let _ = shared.inner.send(Bytes::from(serialized)).await;
}

async fn handle_encrypt_result(shared: &Arc<EnvelopeShared>, data: &[u8]) {
    if data.len() < MSG_HDR_LENGTH + 4 {
        shared.inner.disconnect(false).await;
        return;
    }

    let result = EResult::from_value(read_i32(data, MSG_HDR_LENGTH)).unwrap_or(EResult::Invalid);
    tracing::debug!("EnvelopeEncryptedConnection: Encryption result: {result:?}");

    let has_encryption = lock(&shared.encryption).is_some();

    if result == EResult::OK && has_encryption {
        *lock(&shared.state) = EncryptionState::Encrypted;
        shared.events.emit_connected();
    } else {
        tracing::debug!("EnvelopeEncryptedConnection: Encryption channel setup failed");
        shared.inner.disconnect(false).await;
    }
}

fn is_expected_emsg(state: EncryptionState, msg: EMsg) -> bool {
    match state {
        EncryptionState::Disconnected => false,
        EncryptionState::Connected => msg == EMsg::ChannelEncryptRequest,
        EncryptionState::Challenged => msg == EMsg::ChannelEncryptResult,
        EncryptionState::Encrypted => true,
    }
}

fn parse_msg_type(data: &[u8]) -> Option<EMsg> {
    if data.len() < 4 {
        return None;
    }

    let raw = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    EMsg::from_value((raw & !PROTO_MASK) as i32)
}

fn build_channel_encrypt_response(encrypted_handshake_blob: &[u8], key_crc: u32) -> Vec<u8> {
    let mut serialized = Vec::with_capacity(MSG_HDR_LENGTH + 8 + encrypted_handshake_blob.len() + 8);
    serialized.extend_from_slice(&(EMsg::ChannelEncryptResponse as i32).to_le_bytes());
    serialized.extend_from_slice(&u64::MAX.to_le_bytes());
    serialized.extend_from_slice(&u64::MAX.to_le_bytes());
    serialized.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    serialized.extend_from_slice(&KEY_SIZE.to_le_bytes());
    serialized.extend_from_slice(encrypted_handshake_blob);
    serialized.extend_from_slice(&key_crc.to_le_bytes());
    serialized.extend_from_slice(&0u32.to_le_bytes());
    serialized
}

fn rsa_encrypt_oaep_sha1(
    public_key: &[u8],
    blob_to_encrypt: &[u8],
) -> Result<Vec<u8>, SteamError> {
    let key = RsaPublicKey::from_public_key_der(public_key)
        .map_err(|err| SteamError::Crypto(format!("invalid RSA public key: {err}")))?;

    let padding = Oaep::new::<Sha1>();
    let mut rng = rand::thread_rng();

    key.encrypt(&mut rng, padding, blob_to_encrypt)
        .map_err(|err| SteamError::Crypto(format!("RSA encryption failed: {err}")))
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The Steam public key for `universe`, from [`KeyDictionary`].
pub fn public_key_for_universe(universe: EUniverse) -> Option<&'static [u8]> {
    KeyDictionary::get_public_key(universe)
}
