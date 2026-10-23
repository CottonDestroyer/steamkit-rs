//! Callbacks posted by [`SteamUser`](super::steam_user::SteamUser).

use std::any::Any;
use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, NaiveDateTime, Utc};
use prost::Message;
use steamkit_protos::steam::{
    c_msg_ip_address, CMsgClientAccountInfo, CMsgClientEmailAddrInfo, CMsgClientLoggedOff,
    CMsgClientLogonResponse, CMsgClientPlayingSessionState,
    CMsgClientRequestWebApiAuthenticateUserNonceResponse, CMsgClientSessionToken,
    CMsgClientVanityUrlChangedNotification, CMsgClientWalletInfoUpdate, CMsgIpAddress,
    ParentalSettings,
};

use crate::base::client_msg::{ClientMsg, ClientMsgProtobuf};
use crate::base::generated::steam_language::internal::{
    MsgClientLogOnResponse, MsgClientLoggedOff, MsgClientMarketingMessageUpdate2,
};
use crate::base::generated::steam_language::{
    EAccountFlags, ECurrencyCode, EMarketingMessageFlags, EResult,
};
use crate::base::msg_base::Reader;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::global_id::GlobalID;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

/// Converts a Unix timestamp to a UTC date time.
fn date_time_from_unix_time(seconds: u64) -> DateTime<Utc> {
    let secs = i64::try_from(seconds).unwrap_or(i64::MAX);
    DateTime::<Utc>::from_timestamp(secs, 0)
        .or_else(|| DateTime::<Utc>::from_timestamp(0, 0))
        .unwrap_or_else(|| {
            DateTime::<Utc>::from_naive_utc_and_offset(NaiveDateTime::default(), Utc)
        })
}

/// Converts the IP address in a protobuf message to an [`IpAddr`].
fn ip_address_from_msg(msg: &CMsgIpAddress) -> Option<IpAddr> {
    match &msg.ip {
        Some(c_msg_ip_address::Ip::V4(raw)) => {
            let octets = [
                ((raw & 0xFF00_0000) >> 24) as u8,
                ((raw & 0x00FF_0000) >> 8) as u8,
                ((raw & 0x0000_FF00) << 8) as u8,
                ((raw & 0x0000_00FF) << 24) as u8,
            ];
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        Some(c_msg_ip_address::Ip::V6(raw)) => {
            let mut octets = [0u8; 16];
            if raw.len() != 16 {
                return None;
            }
            octets.copy_from_slice(raw);
            Some(IpAddr::from(octets))
        }
        None => None,
    }
}

/// The response to logging on through
/// [`SteamUser`](crate::steam::handlers::steam_user::steam_user::SteamUser).
#[derive(Clone, Debug)]
pub struct LoggedOnCallback {
    job_id: JobID,
    result: EResult,
    extended_result: EResult,
    out_of_game_secs_per_heartbeat: i32,
    in_game_secs_per_heartbeat: i32,
    public_ip: Option<IpAddr>,
    server_time: DateTime<Utc>,
    account_flags: EAccountFlags,
    client_steam_id: Option<SteamID>,
    email_domain: Option<String>,
    cell_id: u32,
    cell_id_ping_threshold: u32,
    steam2_ticket: Option<Vec<u8>>,
    ip_country_code: Option<String>,
    user_country_code: Option<String>,
    vanity_url: Option<String>,
    num_login_failures_to_migrate: i32,
    num_disconnects_to_migrate: i32,
    parental_settings: Option<ParentalSettings>,
}

impl LoggedOnCallback {
    /// Builds the callback from a `ClientLogOnResponse`.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        if packet_msg.is_proto() {
            let logon_resp = ClientMsgProtobuf::<CMsgClientLogonResponse>::from_packet(packet_msg)?;
            let resp = &logon_resp.body;

            let parental_settings = resp
                .parental_settings
                .as_ref()
                .and_then(|data| ParentalSettings::decode(data.as_slice()).ok());

            Ok(Self {
                job_id: JobID::new(),
                result: EResult::from_value(resp.eresult.unwrap_or_default())
                    .unwrap_or(EResult::Invalid),
                extended_result: EResult::from_value(resp.eresult_extended.unwrap_or_default())
                    .unwrap_or(EResult::Invalid),
                out_of_game_secs_per_heartbeat: resp
                    .legacy_out_of_game_heartbeat_seconds
                    .unwrap_or_default(),
                in_game_secs_per_heartbeat: resp.heartbeat_seconds.unwrap_or_default(),
                public_ip: resp.public_ip.as_ref().and_then(ip_address_from_msg),
                server_time: date_time_from_unix_time(u64::from(
                    resp.rtime32_server_time.unwrap_or_default(),
                )),
                account_flags: EAccountFlags::from_value(
                    resp.account_flags.unwrap_or_default() as i32
                )
                .unwrap_or(EAccountFlags::NormalUser),
                client_steam_id: Some(SteamID::from_id(
                    resp.client_supplied_steamid.unwrap_or_default(),
                )),
                email_domain: resp.email_domain.clone(),
                cell_id: resp.cell_id.unwrap_or_default(),
                cell_id_ping_threshold: resp.cell_id_ping_threshold.unwrap_or_default(),
                steam2_ticket: resp.steam2_ticket.clone(),
                ip_country_code: resp.ip_country_code.clone(),
                user_country_code: resp.user_country.clone(),
                vanity_url: resp.vanity_url.clone(),
                num_login_failures_to_migrate: resp
                    .count_loginfailures_to_migrate
                    .unwrap_or_default(),
                num_disconnects_to_migrate: resp.count_disconnects_to_migrate.unwrap_or_default(),
                parental_settings,
            })
        } else {
            let logon_resp = ClientMsg::<MsgClientLogOnResponse>::from_packet(packet_msg)?;
            let resp = &logon_resp.body;

            Ok(Self {
                job_id: JobID::new(),
                result: resp.result,
                extended_result: EResult::Invalid,
                out_of_game_secs_per_heartbeat: resp.out_of_game_heartbeat_rate_sec,
                in_game_secs_per_heartbeat: resp.in_game_heartbeat_rate_sec,
                public_ip: Some(IpAddr::V4(Ipv4Addr::from(resp.ip_public.to_be_bytes()))),
                server_time: date_time_from_unix_time(u64::from(resp.server_real_time)),
                account_flags: EAccountFlags::NormalUser,
                client_steam_id: Some(SteamID::from_id(resp.client_supplied_steam_id)),
                email_domain: None,
                cell_id: 0,
                cell_id_ping_threshold: 0,
                steam2_ticket: None,
                ip_country_code: None,
                user_country_code: None,
                vanity_url: None,
                num_login_failures_to_migrate: 0,
                num_disconnects_to_migrate: 0,
                parental_settings: None,
            })
        }
    }

    /// Builds the callback for a logon that never reached the server.
    pub(crate) fn from_result(result: EResult) -> Self {
        Self {
            job_id: JobID::new(),
            result,
            extended_result: EResult::Invalid,
            out_of_game_secs_per_heartbeat: 0,
            in_game_secs_per_heartbeat: 0,
            public_ip: None,
            server_time: date_time_from_unix_time(0),
            account_flags: EAccountFlags::NormalUser,
            client_steam_id: None,
            email_domain: None,
            cell_id: 0,
            cell_id_ping_threshold: 0,
            steam2_ticket: None,
            ip_country_code: None,
            user_country_code: None,
            vanity_url: None,
            num_login_failures_to_migrate: 0,
            num_disconnects_to_migrate: 0,
            parental_settings: None,
        }
    }

    /// The result of the logon.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The extended result of the logon.
    pub fn extended_result(&self) -> EResult {
        self.extended_result
    }

    /// Seconds between heartbeats while not in a game.
    pub fn out_of_game_secs_per_heartbeat(&self) -> i32 {
        self.out_of_game_secs_per_heartbeat
    }

    /// Seconds between heartbeats while in a game.
    pub fn in_game_secs_per_heartbeat(&self) -> i32 {
        self.in_game_secs_per_heartbeat
    }

    /// The public IP of the client.
    pub fn public_ip(&self) -> Option<IpAddr> {
        self.public_ip
    }

    /// The server time.
    pub fn server_time(&self) -> DateTime<Utc> {
        self.server_time
    }

    /// The account flags assigned by the server.
    pub fn account_flags(&self) -> EAccountFlags {
        self.account_flags
    }

    /// The client steam ID.
    pub fn client_steam_id(&self) -> Option<SteamID> {
        self.client_steam_id
    }

    /// The email domain.
    pub fn email_domain(&self) -> Option<&str> {
        self.email_domain.as_deref()
    }

    /// The Steam2 CellID.
    pub fn cell_id(&self) -> u32 {
        self.cell_id
    }

    /// The Steam2 CellID ping threshold.
    pub fn cell_id_ping_threshold(&self) -> u32 {
        self.cell_id_ping_threshold
    }

    /// The Steam2 ticket.
    pub fn steam2_ticket(&self) -> Option<&[u8]> {
        self.steam2_ticket.as_deref()
    }

    /// The IP country code.
    pub fn ip_country_code(&self) -> Option<&str> {
        self.ip_country_code.as_deref()
    }

    /// The account country code.
    pub fn user_country_code(&self) -> Option<&str> {
        self.user_country_code.as_deref()
    }

    /// The vanity URL.
    pub fn vanity_url(&self) -> Option<&str> {
        self.vanity_url.as_deref()
    }

    /// How many login failures before Steam wants the client to move to
    /// another CM.
    pub fn num_login_failures_to_migrate(&self) -> i32 {
        self.num_login_failures_to_migrate
    }

    /// How many disconnects before Steam wants the client to move to another CM.
    pub fn num_disconnects_to_migrate(&self) -> i32 {
        self.num_disconnects_to_migrate
    }

    /// The Steam parental settings.
    pub fn parental_settings(&self) -> Option<&ParentalSettings> {
        self.parental_settings.as_ref()
    }
}

impl CallbackMsg for LoggedOnCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when the server tells the client to log off.
#[derive(Clone, Debug)]
pub struct LoggedOffCallback {
    job_id: JobID,
    result: EResult,
}

impl LoggedOffCallback {
    /// Builds the callback from a received `ClientLoggedOff` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let result = if packet_msg.is_proto() {
            let logged_off = ClientMsgProtobuf::<CMsgClientLoggedOff>::from_packet(packet_msg)?;
            EResult::from_value(logged_off.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid)
        } else {
            let logged_off = ClientMsg::<MsgClientLoggedOff>::from_packet(packet_msg)?;
            logged_off.body.result
        };

        Ok(Self {
            job_id: JobID::new(),
            result,
        })
    }

    /// The result of the log off.
    pub fn result(&self) -> EResult {
        self.result
    }
}

impl CallbackMsg for LoggedOffCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when the client gets its Steam3 session token.
#[derive(Clone, Debug)]
pub struct SessionTokenCallback {
    job_id: JobID,
    session_token: u64,
}

impl SessionTokenCallback {
    /// Builds the callback from a received `ClientSessionToken` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let sess_token = ClientMsgProtobuf::<CMsgClientSessionToken>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: JobID::new(),
            session_token: sess_token.body.token.unwrap_or_default(),
        })
    }

    /// The session token, used to authenticate with other services.
    pub fn session_token(&self) -> u64 {
        self.session_token
    }
}

impl CallbackMsg for SessionTokenCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted with the account info, usually right after logon.
#[derive(Clone, Debug)]
pub struct AccountInfoCallback {
    job_id: JobID,
    persona_name: String,
    country: String,
    count_authed_computers: i32,
    account_flags: EAccountFlags,
}

impl AccountInfoCallback {
    /// Builds the callback from a received `ClientAccountInfo` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let acc_info = ClientMsgProtobuf::<CMsgClientAccountInfo>::from_packet(packet_msg)?;
        let msg = &acc_info.body;

        Ok(Self {
            job_id: JobID::new(),
            persona_name: msg.persona_name.clone().unwrap_or_default(),
            country: msg.ip_country.clone().unwrap_or_default(),
            count_authed_computers: msg.count_authed_computers.unwrap_or_default(),
            account_flags: EAccountFlags::from_value(msg.account_flags.unwrap_or_default() as i32)
                .unwrap_or(EAccountFlags::NormalUser),
        })
    }

    /// The last persona name used by this account.
    pub fn persona_name(&self) -> &str {
        &self.persona_name
    }

    /// The country this account is connected from.
    pub fn country(&self) -> &str {
        &self.country
    }

    /// The number of Steam Guard authorized computers.
    pub fn count_authed_computers(&self) -> i32 {
        self.count_authed_computers
    }

    /// The account flags for this account.
    pub fn account_flags(&self) -> EAccountFlags {
        self.account_flags
    }
}

impl CallbackMsg for AccountInfoCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted with the account's email info.
#[derive(Clone, Debug)]
pub struct EmailAddrInfoCallback {
    job_id: JobID,
    email_address: String,
    is_validated: bool,
}

impl EmailAddrInfoCallback {
    /// Builds the callback from a received `ClientEmailAddrInfo` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let email_addr_info =
            ClientMsgProtobuf::<CMsgClientEmailAddrInfo>::from_packet(packet_msg)?;
        let msg = &email_addr_info.body;

        Ok(Self {
            job_id: JobID::new(),
            email_address: msg.email_address.clone().unwrap_or_default(),
            is_validated: msg.email_is_validated.unwrap_or_default(),
        })
    }

    /// The email address of this account.
    pub fn email_address(&self) -> &str {
        &self.email_address
    }

    /// Whether the email address is validated.
    pub fn is_validated(&self) -> bool {
        self.is_validated
    }
}

impl CallbackMsg for EmailAddrInfoCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted with the account's wallet info.
#[derive(Clone, Debug)]
pub struct WalletInfoCallback {
    job_id: JobID,
    has_wallet: bool,
    currency: ECurrencyCode,
    balance: i32,
    balance_delayed: i32,
    long_balance: i64,
    long_balance_delayed: i64,
}

impl WalletInfoCallback {
    /// Builds the callback from a received `ClientWalletInfoUpdate` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let wallet_info = ClientMsgProtobuf::<CMsgClientWalletInfoUpdate>::from_packet(packet_msg)?;
        let wallet = &wallet_info.body;

        Ok(Self {
            job_id: JobID::new(),
            has_wallet: wallet.has_wallet.unwrap_or_default(),
            currency: ECurrencyCode::from_value(wallet.currency.unwrap_or_default())
                .unwrap_or(ECurrencyCode::Invalid),
            balance: wallet.balance.unwrap_or_default(),
            balance_delayed: wallet.balance_delayed.unwrap_or_default(),
            long_balance: wallet.balance64.unwrap_or_default(),
            long_balance_delayed: wallet.balance64_delayed.unwrap_or_default(),
        })
    }

    /// Whether the account has a wallet.
    pub fn has_wallet(&self) -> bool {
        self.has_wallet
    }

    /// The currency code for this wallet.
    pub fn currency(&self) -> ECurrencyCode {
        self.currency
    }

    /// The balance of the wallet as a 32-bit integer, in cents.
    pub fn balance(&self) -> i32 {
        self.balance
    }

    /// The delayed (pending) balance of the wallet as a 32-bit integer, in cents.
    pub fn balance_delayed(&self) -> i32 {
        self.balance_delayed
    }

    /// The balance of the wallet as a 64-bit integer, in cents.
    pub fn long_balance(&self) -> i64 {
        self.long_balance
    }

    /// The delayed (pending) balance of the wallet as a 64-bit integer, in cents.
    pub fn long_balance_delayed(&self) -> i64 {
        self.long_balance_delayed
    }
}

impl CallbackMsg for WalletInfoCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to requesting a new Web API authentication user nonce.
#[derive(Clone, Debug)]
pub struct WebAPIUserNonceCallback {
    job_id: JobID,
    result: EResult,
    nonce: String,
}

impl WebAPIUserNonceCallback {
    /// Builds the callback from a `ClientRequestWebAPIAuthenticateUserNonceResponse`.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let user_nonce =
            ClientMsgProtobuf::<CMsgClientRequestWebApiAuthenticateUserNonceResponse>::from_packet(
                packet_msg,
            )?;
        let body = &user_nonce.body;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            nonce: body
                .webapi_authenticate_user_nonce
                .clone()
                .unwrap_or_default(),
        })
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The authentication nonce.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }
}

impl CallbackMsg for WebAPIUserNonceCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when the user's vanity URL changes.
#[derive(Clone, Debug)]
pub struct VanityURLChangedCallback {
    job_id: JobID,
    vanity_url: String,
}

impl VanityURLChangedCallback {
    /// Builds the callback from a `ClientVanityURLChangedNotification`.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let vanity_url =
            ClientMsgProtobuf::<CMsgClientVanityUrlChangedNotification>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            vanity_url: vanity_url.body.vanity_url.clone().unwrap_or_default(),
        })
    }

    /// The new vanity url.
    pub fn vanity_url(&self) -> &str {
        &self.vanity_url
    }
}

impl CallbackMsg for VanityURLChangedCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when the client gets a marketing message update.
#[derive(Clone, Debug)]
pub struct MarketingMessageCallback {
    job_id: JobID,
    update_time: DateTime<Utc>,
    messages: Vec<MarketingMessage>,
}

/// A single marketing message.
#[derive(Clone, Debug)]
pub struct MarketingMessage {
    id: GlobalID,
    url: String,
    flags: EMarketingMessageFlags,
}

impl MarketingMessage {
    /// Parses a marketing message from its binary form.
    fn from_data(data: &[u8]) -> SteamResult<Self> {
        let mut reader = Reader::new(data);

        let id = GlobalID::from_value(reader.read_u64()?);
        let url = reader.read_null_term_string()?;
        let flags = EMarketingMessageFlags::from_value(reader.read_u32()? as i32)
            .unwrap_or(EMarketingMessageFlags::None);

        Ok(Self { id, url, flags })
    }

    /// The unique identifier for this marketing message.
    pub fn id(&self) -> GlobalID {
        self.id
    }

    /// The URL for this marketing message.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The marketing message flags.
    pub fn flags(&self) -> EMarketingMessageFlags {
        self.flags
    }
}

impl MarketingMessageCallback {
    /// Builds the callback from a received `ClientMarketingMessageUpdate2` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let marketing_message =
            ClientMsg::<MsgClientMarketingMessageUpdate2>::from_packet(packet_msg)?;
        let (marketing_message_update_time, count) = {
            let body = &marketing_message.body;
            (body.marketing_message_update_time, body.count)
        };

        let mut payload = marketing_message.payload;
        let mut messages = Vec::new();

        for _ in 0..count {
            // The length includes its own 4 bytes.
            let data_len = (payload.read_i32()? as usize).saturating_sub(4);
            let data = payload.read_bytes(data_len);
            messages.push(MarketingMessage::from_data(&data)?);
        }

        Ok(Self {
            job_id: JobID::new(),
            update_time: date_time_from_unix_time(u64::from(marketing_message_update_time)),
            messages,
        })
    }

    /// The time of this marketing message update.
    pub fn update_time(&self) -> DateTime<Utc> {
        self.update_time
    }

    /// The messages.
    pub fn messages(&self) -> &[MarketingMessage] {
        &self.messages
    }
}

impl CallbackMsg for MarketingMessageCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when another client starts or stops playing a game. While playing is
/// blocked, sending `ClientGamesPlayed` logs you off with `LoggedInElsewhere`.
#[derive(Clone, Debug)]
pub struct PlayingSessionStateCallback {
    job_id: JobID,
    playing_blocked: bool,
    playing_app_id: u32,
}

impl PlayingSessionStateCallback {
    /// Builds the callback from a received `ClientPlayingSessionState` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let playing_session_state =
            ClientMsgProtobuf::<CMsgClientPlayingSessionState>::from_packet(packet_msg)?;
        let msg = &playing_session_state.body;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            playing_blocked: msg.playing_blocked.unwrap_or_default(),
            playing_app_id: msg.playing_app.unwrap_or_default(),
        })
    }

    /// Whether another client is currently blocking playing.
    pub fn playing_blocked(&self) -> bool {
        self.playing_blocked
    }

    /// The app being played elsewhere, when blocked.
    pub fn playing_app_id(&self) -> u32 {
        self.playing_app_id
    }
}

impl CallbackMsg for PlayingSessionStateCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
