//! Generating auth session tickets and handling Steam's verification of them.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::Mutex;
use rand::RngCore;
use steamkit_protos::steam::{
    CMsgAuthTicket, CMsgClientAuthList, CMsgClientAuthListAck, CMsgClientGameConnectTokens,
    CMsgClientTicketAuthComplete,
};
use tokio::sync::Notify;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{EMsg, EResult};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::handlers::steam_apps::steam_apps::SteamApps;
use crate::steam::steam_client::steam_client::SteamClient;

use super::callbacks::{TicketAcceptedCallback, TicketAuthCompleteCallback};
use super::ticket_info::TicketInfo;

/// Size of a Web API ticket: 2560 bytes, per
/// <https://partner.steamgames.com/doc/api/ISteamUser#GetTicketForWebApiResponse_t>.
const WEB_API_TICKET_SIZE: usize = 2560;

/// How long to wait for Steam to acknowledge a ticket.
const TICKET_ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// The sequence number put into every generated ticket. Shared by all handler
/// instances.
static SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// The type of an auth session ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum TicketType {
    /// A regular auth session ticket.
    AuthSession = 2,

    /// A Web API auth session ticket.
    WebApiTicket = 5,
}

impl TicketType {
    /// The numeric value of this ticket type.
    pub const fn value(self) -> u32 {
        self as u32
    }
}

/// Generates auth session tickets and handles Steam's verification of them.
pub struct SteamAuthTicket {
    client: Weak<SteamClient>,
    game_connect_tokens: Mutex<VecDeque<Vec<u8>>>,
    tickets_by_game: Mutex<HashMap<u32, Vec<CMsgAuthTicket>>>,
    ticket_acks: DashMap<u32, Arc<Notify>>,
}

impl std::fmt::Debug for SteamAuthTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamAuthTicket")
            .field("game_connect_tokens", &self.game_connect_tokens.lock().len())
            .field("ticket_acks", &self.ticket_acks.len())
            .finish()
    }
}

impl SteamAuthTicket {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
            game_connect_tokens: Mutex::new(VecDeque::new()),
            tickets_by_game: Mutex::new(HashMap::new()),
            ticket_acks: DashMap::new(),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client
            .upgrade()
            .ok_or_else(|| SteamError::Connection("SteamClient has been dropped".to_string()))
    }

    /// Generates an auth session ticket for `appid` and waits for Steam to
    /// accept it.
    pub async fn get_auth_session_ticket(&self, appid: u32) -> SteamResult<TicketInfo> {
        self.get_auth_session_ticket_internal(appid, TicketType::AuthSession, None)
            .await
    }

    /// Generates a Web API auth session ticket for `appid` and waits for Steam
    /// to accept it. `identity` is a string identifying the remote service
    /// that will check the ticket.
    pub async fn get_auth_ticket_for_web_api(
        &self,
        appid: u32,
        identity: &str,
    ) -> SteamResult<TicketInfo> {
        self.get_auth_session_ticket_internal(appid, TicketType::WebApiTicket, Some(identity))
            .await
    }

    /// The shared implementation of [`SteamAuthTicket::get_auth_session_ticket`]
    /// and [`SteamAuthTicket::get_auth_ticket_for_web_api`].
    pub(crate) async fn get_auth_session_ticket_internal(
        &self,
        appid: u32,
        ticket_type: TicketType,
        identity: Option<&str>,
    ) -> SteamResult<TicketInfo> {
        let client = self.client()?;

        if client.cell_id().is_none() {
            return Err(SteamError::Connection("User not logged in.".to_string()));
        }

        let apps = client.get_required_handler::<SteamApps>()?;
        let app_ticket = apps.get_app_ownership_ticket(appid).await?;

        if app_ticket.result() != EResult::OK {
            return Err(SteamError::Other(format!(
                "Failed to obtain app ownership ticket. Result: {:?}. \
                 The user may not own the game or there was an error.",
                app_ticket.result()
            )));
        }

        let game_connect_token = {
            let mut tokens = self.game_connect_tokens.lock();
            tokens.pop_front()
        };

        let Some(game_connect_token) = game_connect_token else {
            return Err(SteamError::Other(
                "There's no available game connect tokens left.".to_string(),
            ));
        };

        let auth_ticket = build_auth_ticket(&game_connect_token, ticket_type);

        // Same as the Steam client: prefix the identity with "str:" and
        // null-terminate it.
        let server_secret = identity
            .filter(|identity| !identity.is_empty())
            .map(|identity| {
                let mut secret = format!("str:{identity}").into_bytes();
                secret.push(0);
                secret
            });

        let crc = self.register_and_send_ticket(&client, appid, &auth_ticket, server_secret.as_deref())?;

        // Wait for Steam to ack this ticket (matched by CRC). Steam no longer
        // reliably re-acks repeated auth lists, so a missing ack isn't a
        // failure: the ticket is submitted and gets validated server-side
        // through BeginAuthSession.
        if !self.wait_for_ticket_ack(crc).await {
            tracing::warn!(
                "Ticket CRC {} for app {} was not acknowledged by Steam within {}s; \
                 returning it anyway as it validates server-side (see issue #1567).",
                crc,
                appid,
                TICKET_ACK_TIMEOUT.as_secs()
            );
        }

        let token = combine_tickets(
            &auth_ticket,
            app_ticket.ticket(),
            ticket_type == TicketType::WebApiTicket,
        );

        Ok(TicketInfo::new(&client, appid, token))
    }

    /// Registers the ticket for `appid`, sends the auth list to Steam and
    /// returns the ticket's CRC32.
    fn register_and_send_ticket(
        &self,
        client: &Arc<SteamClient>,
        appid: u32,
        auth_token: &[u8],
        server_secret: Option<&[u8]>,
    ) -> SteamResult<u32> {
        let crc = crc32fast::hash(auth_token);

        {
            let mut tickets = self.tickets_by_game.lock();

            // Add the ticket to the list of the specified game.
            tickets.entry(appid).or_default().push(CMsgAuthTicket {
                gameid: Some(u64::from(appid)),
                ticket: Some(auth_token.to_vec()),
                ticket_crc: Some(crc),
                server_secret: server_secret.map(|secret| secret.to_vec()),
                ..Default::default()
            });
        }

        // Register the ack waiter before sending so we can't miss the ack.
        self.ticket_acks.insert(crc, Arc::new(Notify::new()));

        self.send_tickets(client)?;

        Ok(crc)
    }

    /// Removes the ticket from its app's auth list, so Steam stops honoring it.
    pub(crate) fn cancel_auth_ticket(&self, ticket: &TicketInfo) {
        {
            let mut tickets = self.tickets_by_game.lock();
            if let Some(tickets) = tickets.get_mut(&ticket.app_id()) {
                tickets.retain(|entry| entry.ticket_crc != Some(ticket.ticket_crc()));
            }
        }

        if let Ok(client) = self.client() {
            if let Err(error) = self.send_tickets(&client) {
                tracing::warn!("failed to send the auth list after cancelling a ticket: {error}");
            }
        }
    }

    /// Sends the current auth list to Steam.
    fn send_tickets(&self, client: &Arc<SteamClient>) -> SteamResult<()> {
        let mut auth = ClientMsgProtobuf::<CMsgClientAuthList>::new(EMsg::ClientAuthList);

        {
            let tokens = self.game_connect_tokens.lock();
            auth.body.tokens_left = Some(tokens.len() as u32);
        }

        {
            let tickets = self.tickets_by_game.lock();

            // Flatten the map into a ticket list.
            auth.body.app_ids = tickets.keys().copied().collect();
            auth.body.tickets = tickets.values().flatten().cloned().collect();
        }

        let job_id = client.get_next_job_id();
        auth.set_source_job_id(job_id);

        client.send(&auth)?;

        Ok(())
    }

    /// Waits for Steam to ack the ticket with the given CRC. Returns `false` if
    /// it didn't arrive in time.
    async fn wait_for_ticket_ack(&self, crc: u32) -> bool {
        let Some(notifier) = self
            .ticket_acks
            .get(&crc)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return false;
        };

        let acknowledged = tokio::time::timeout(TICKET_ACK_TIMEOUT, notifier.notified()).await;

        self.ticket_acks.remove(&crc);

        acknowledged.is_ok()
    }

    /// Handles a `ClientAuthListAck` message.
    fn handle_ticket_acknowledged(
        &self,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<TicketAcceptedCallback> {
        let auth_ack = ClientMsgProtobuf::<CMsgClientAuthListAck>::from_packet(packet_msg)?;

        // Match acks by ticket CRC, not job id: since Steam's 2025-08 change it
        // no longer echoes the job id on repeated auth lists (SteamKit issue
        // #1567).
        for crc in &auth_ack.body.ticket_crc {
            if let Some(entry) = self.ticket_acks.get(crc) {
                entry.value().notify_one();
            }
        }

        Ok(TicketAcceptedCallback::new(
            auth_ack.target_job_id(),
            &auth_ack.body,
        ))
    }

    /// Handles a `ClientGameConnectTokens` message.
    fn handle_game_connect_tokens(&self, packet_msg: &dyn IPacketMsg) -> SteamResult<()> {
        let body = ClientMsgProtobuf::<CMsgClientGameConnectTokens>::from_packet(packet_msg)?;

        let max_tokens_to_keep = body.body.max_tokens_to_keep.unwrap_or_default();

        let mut tokens = self.game_connect_tokens.lock();

        // Add the tokens.
        for token in &body.body.tokens {
            tokens.push_back(token.clone());
        }

        // Keep only the required number, dropping the oldest.
        while tokens.len() > max_tokens_to_keep as usize {
            tokens.pop_front();
        }

        Ok(())
    }
}

/// Builds the auth ticket for the given game connect token.
fn build_auth_ticket(game_connect_token: &[u8], ticket_type: TicketType) -> Vec<u8> {
    const SESSION_SIZE: u32 = 4 // unknown, always 1
        + 4 // TicketType, 2 or 5
        + 4 // public IP v4, optional
        + 4 // private IP v4, optional
        + 4 // timestamp & uint.MaxValue
        + 4; // sequence

    let mut ticket = Vec::with_capacity(game_connect_token.len() + 4 + SESSION_SIZE as usize);

    ticket.extend_from_slice(&(game_connect_token.len() as u32).to_le_bytes());
    ticket.extend_from_slice(game_connect_token);

    ticket.extend_from_slice(&SESSION_SIZE.to_le_bytes());
    ticket.extend_from_slice(&1u32.to_le_bytes());
    ticket.extend_from_slice(&ticket_type.value().to_le_bytes());
    ticket.extend_from_slice(&random_bytes(8));
    ticket.extend_from_slice(&timestamp().to_le_bytes());
    ticket.extend_from_slice(
        &SEQUENCE
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
            .to_le_bytes(),
    );

    ticket
}

/// Combines the auth ticket and the app ownership ticket into one token.
fn combine_tickets(auth_ticket: &[u8], app_ticket: &[u8], pad_to_web_api_size: bool) -> Vec<u8> {
    let len = app_ticket.len();

    let raw_size = auth_ticket.len() + 4 + app_ticket.len();
    let target = if pad_to_web_api_size {
        raw_size.max(WEB_API_TICKET_SIZE)
    } else {
        raw_size
    };

    let mut token = vec![0u8; target];

    token[..auth_ticket.len()].copy_from_slice(auth_ticket);
    token[auth_ticket.len()..auth_ticket.len() + 4].copy_from_slice(&(len as u32).to_le_bytes());
    token[auth_ticket.len() + 4..raw_size].copy_from_slice(app_ticket);

    // A Web API ticket is always 2560 bytes; whatever follows the tickets is
    // just leftover memory.
    if pad_to_web_api_size && raw_size < target {
        let trash = random_bytes(target - raw_size);
        token[raw_size..target].copy_from_slice(&trash);
    }

    token
}

/// `len` random bytes.
fn random_bytes(len: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut buffer);
    buffer
}

/// A monotonic tick counter. Only used as a nonce inside the ticket.
fn timestamp() -> u32 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

    let start = START.get_or_init(Instant::now);
    start.elapsed().as_nanos() as u32
}

#[async_trait]
impl ClientMsgHandler for SteamAuthTicket {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        match packet_msg.msg_type() {
            EMsg::ClientAuthListAck => {
                let callback = self.handle_ticket_acknowledged(packet_msg)?;
                client.post_callback(Box::new(callback));
            }
            EMsg::ClientTicketAuthComplete => {
                let complete =
                    ClientMsgProtobuf::<CMsgClientTicketAuthComplete>::from_packet(packet_msg)?;
                let callback =
                    TicketAuthCompleteCallback::new(complete.target_job_id(), &complete.body);
                client.post_callback(Box::new(callback));
            }
            EMsg::ClientGameConnectTokens => {
                self.handle_game_connect_tokens(packet_msg)?;
            }
            EMsg::ClientLogOff => {
                // Drop all game connect tokens on log off.
                self.game_connect_tokens.lock().clear();
            }
            // Not a message we handle.
            _ => {}
        }

        Ok(())
    }
}
