//! Logging on and off as a user.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::{c_msg_ip_address, CMsgClientLogOff, CMsgClientLogon, CMsgIpAddress};

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::internal::MsgClientLogon;
use crate::base::generated::steam_language::{
    EAccountType, EGamingDeviceType, EMsg, EOSType, EResult, EUIMode, EUniverse,
};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::steam_id::SteamID;

use super::callbacks::{
    AccountInfoCallback, EmailAddrInfoCallback, LoggedOffCallback, LoggedOnCallback,
    MarketingMessageCallback, PlayingSessionStateCallback, SessionTokenCallback,
    VanityURLChangedCallback, WalletInfoCallback, WebAPIUserNonceCallback,
};

/// The account instance of desktop clients.
const DESKTOP_INSTANCE: u32 = 1;

/// The client package version reported at logon. Needed to get a proper sentry
/// file for Steam Guard.
const CLIENT_PACKAGE_VERSION: u32 = 1771;

/// The mask applied to the private IP address in the logon.
const OBFUSCATION_MASK: u32 = MsgClientLogon::OBFUSCATION_MASK;

/// The chat mode to log on with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ChatMode {
    /// The default chat mode.
    Default = 0,

    /// The new Steam group chat.
    NewSteamChat = 2,
}

/// What's needed to log on as a user.
#[derive(Clone, Debug)]
pub struct LogOnDetails {
    /// The username.
    pub username: Option<String>,

    /// The password.
    pub password: Option<String>,

    /// The cell id.
    pub cell_id: Option<u32>,

    /// Identifies the logon session.
    ///
    /// Steam doesn't allow more than one session with the same login id on an
    /// account from the same public IP at the same time, so give every
    /// session from the same IP its own login id if you need several. If
    /// `None`, it's derived from the machine's primary address (the same for
    /// every session). If in doubt, leave it `None`.
    pub login_id: Option<u32>,

    /// The Steam Guard code from the email.
    pub auth_code: Option<String>,

    /// The 2-factor code from the authenticator app.
    pub two_factor_code: Option<String>,

    /// "Remember password". Used together with the access token for
    /// password-less login.
    pub should_remember_password: bool,

    /// The access token from a successful login through the authentication
    /// module (e.g. the QR flow).
    pub access_token: Option<String>,

    /// The account instance: 1 for PC, 2 for console (PS3).
    pub account_instance: u32,

    /// The account id for console instance logins.
    pub account_id: u32,

    /// Whether to request a Steam2 ticket (only needed for Steam2 content
    /// downloads).
    pub request_steam2_ticket: bool,

    /// The client operating system type.
    pub client_os_type: EOSType,

    /// The client language.
    pub client_language: String,

    /// The machine name.
    pub machine_name: Option<String>,

    /// The chat mode.
    pub chat_mode: ChatMode,

    /// The UI mode.
    pub ui_mode: EUIMode,

    /// The gaming device type.
    pub gaming_device_type: EGamingDeviceType,
}

impl Default for LogOnDetails {
    fn default() -> Self {
        Self {
            username: None,
            password: None,
            cell_id: None,
            login_id: None,
            auth_code: None,
            two_factor_code: None,
            should_remember_password: false,
            access_token: None,
            // The default PC instance.
            account_instance: DESKTOP_INSTANCE,
            account_id: 0,
            request_steam2_ticket: false,
            client_os_type: crate::util::utils::get_os_type(),
            client_language: "english".to_string(),
            machine_name: Some(format!("{} (SteamKit2)", machine_name())),
            chat_mode: ChatMode::Default,
            ui_mode: EUIMode::Unknown,
            gaming_device_type: EGamingDeviceType::Unknown,
        }
    }
}

/// What's needed to log on anonymously.
#[derive(Clone, Debug)]
pub struct AnonymousLogOnDetails {
    /// The cell id.
    pub cell_id: Option<u32>,

    /// The client operating system type.
    pub client_os_type: EOSType,

    /// The client language.
    pub client_language: String,
}

impl Default for AnonymousLogOnDetails {
    fn default() -> Self {
        Self {
            cell_id: None,
            client_os_type: crate::util::utils::get_os_type(),
            client_language: "english".to_string(),
        }
    }
}

/// The name of this machine.
fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Packs SteamID components into the 64-bit form: account id in bits 0-31,
/// instance in 32-51, account type in 52-55 and universe in 56-63.
fn pack_steam_id(
    account_id: u32,
    account_instance: u32,
    universe: EUniverse,
    account_type: EAccountType,
) -> u64 {
    ((universe.value() as u64) & 0xFF) << 56
        | ((account_type.value() as u64) & 0x0F) << 52
        | ((account_instance as u64) & 0x000F_FFFF) << 32
        | (account_id as u64)
}

/// Logging on and off as a user.
pub struct SteamUser {
    client: Weak<SteamClient>,
}

impl SteamUser {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client
            .upgrade()
            .ok_or_else(|| SteamError::Connection("SteamClient has been dropped".to_string()))
    }

    /// Our SteamID, set after a successful logon.
    pub fn steam_id(&self) -> Option<SteamID> {
        self.client().ok().and_then(|client| client.steam_id())
    }

    /// Whether the server is about to close the connection on purpose.
    pub fn expect_disconnection(&self) -> bool {
        self.client
            .upgrade()
            .map(|client| client.expect_disconnection())
            .unwrap_or(false)
    }

    /// Logs on. The client has to be connected already. The result comes in a
    /// [`LoggedOnCallback`].
    pub fn log_on(&self, details: &LogOnDetails) -> SteamResult<()> {
        let username = details.username.as_deref().unwrap_or_default();
        let has_password = details
            .password
            .as_deref()
            .is_some_and(|password| !password.is_empty());
        let has_access_token = details
            .access_token
            .as_deref()
            .is_some_and(|token| !token.is_empty());

        if username.is_empty() || (!has_password && !has_access_token) {
            return Err(SteamError::InvalidArgument(
                "LogOn requires a username and password or access token to be set in 'details'."
                    .to_string(),
            ));
        }

        let client = self.client()?;

        if !client.is_connected() {
            client.post_callback(Box::new(LoggedOnCallback::from_result(
                EResult::NoConnection,
            )));
            return Ok(());
        }

        let universe = client.configuration().universe();
        let steam_id = SteamID::from_id(pack_steam_id(
            details.account_id,
            details.account_instance,
            universe,
            EAccountType::Individual,
        ));

        let mut logon = ClientMsgProtobuf::<CMsgClientLogon>::new(EMsg::ClientLogon);

        let obfuscated_private_ip = match details.login_id {
            Some(login_id) => CMsgIpAddress {
                ip: Some(c_msg_ip_address::Ip::V4(login_id)),
            },
            None => {
                let local_ip = client.local_ip().ok_or_else(|| {
                    SteamError::Connection("the local IP address is not known yet".to_string())
                })?;
                obfuscate_private_ip(local_ip)
            }
        };

        logon.body.obfuscated_private_ip = Some(obfuscated_private_ip.clone());

        // Legacy field, but the Steam client still sets it.
        if let Some(c_msg_ip_address::Ip::V4(v4)) = obfuscated_private_ip.ip {
            logon.body.deprecated_obfustucated_private_ip = Some(v4);
        }

        logon.set_session_id(0);
        logon.set_steam_id(steam_id);

        logon.body.account_name = details.username.clone();
        logon.body.password = details.password.clone();
        logon.body.should_remember_password = Some(details.should_remember_password);

        logon.body.protocol_version = Some(MsgClientLogon::CURRENT_PROTOCOL);
        logon.body.client_os_type = Some(details.client_os_type as u32);
        logon.body.client_language = Some(details.client_language.clone());
        logon.body.cell_id = Some(
            details
                .cell_id
                .unwrap_or_else(|| client.configuration().cell_id()),
        );

        logon.body.steam2_ticket_request = Some(details.request_steam2_ticket);

        // Required to get a proper sentry file for Steam Guard.
        logon.body.client_package_version = Some(CLIENT_PACKAGE_VERSION);
        logon.body.supports_rate_limit_response = Some(true);
        logon.body.machine_name = details.machine_name.clone();
        logon.body.machine_id = crate::util::hardware_utils::get_machine_id(
            client.configuration().machine_info_provider(),
        );

        if details.chat_mode != ChatMode::Default {
            logon.body.chat_mode = Some(details.chat_mode as u32);
        }

        if details.ui_mode != EUIMode::Unknown {
            logon.body.ui_mode = Some(details.ui_mode as u32);
        }

        if details.gaming_device_type != EGamingDeviceType::Unknown {
            logon.body.gaming_device_type = Some(details.gaming_device_type as u32);
        }

        // Steam Guard
        logon.body.auth_code = details.auth_code.clone();
        logon.body.two_factor_code = details.two_factor_code.clone();

        logon.body.access_token = details.access_token.clone();

        client.send(&logon)?;

        Ok(())
    }

    /// Logs on anonymously. The client has to be connected already. The result
    /// comes in a [`LoggedOnCallback`].
    pub fn log_on_anonymous(&self) -> SteamResult<()> {
        self.log_on_anonymous_with_details(&AnonymousLogOnDetails::default())
    }

    /// Logs on anonymously with the given details. The client has to be
    /// connected already. The result comes in a [`LoggedOnCallback`].
    pub fn log_on_anonymous_with_details(
        &self,
        details: &AnonymousLogOnDetails,
    ) -> SteamResult<()> {
        let client = self.client()?;

        if !client.is_connected() {
            client.post_callback(Box::new(LoggedOnCallback::from_result(
                EResult::NoConnection,
            )));
            return Ok(());
        }

        let universe = client.configuration().universe();
        let au_id = SteamID::from_id(pack_steam_id(0, 0, universe, EAccountType::AnonUser));

        let mut logon = ClientMsgProtobuf::<CMsgClientLogon>::new(EMsg::ClientLogon);

        logon.set_session_id(0);
        logon.set_steam_id(au_id);

        logon.body.protocol_version = Some(MsgClientLogon::CURRENT_PROTOCOL);
        logon.body.client_os_type = Some(details.client_os_type as u32);
        logon.body.client_language = Some(details.client_language.clone());
        logon.body.cell_id = Some(
            details
                .cell_id
                .unwrap_or_else(|| client.configuration().cell_id()),
        );

        logon.body.machine_id = crate::util::hardware_utils::get_machine_id(
            client.configuration().machine_info_provider(),
        );

        client.send(&logon)?;

        Ok(())
    }

    /// Logs off. The server then disconnects us and a `DisconnectedCallback`
    /// is posted.
    pub fn log_off(&self) -> SteamResult<()> {
        let client = self.client()?;

        client.set_expect_disconnection(true);

        let log_off = ClientMsgProtobuf::<CMsgClientLogOff>::new(EMsg::ClientLogOff);
        client.send(&log_off)?;

        Ok(())
    }
}

/// The local IP address in the obfuscated form Steam expects in the logon.
fn obfuscate_private_ip(local_ip: std::net::IpAddr) -> CMsgIpAddress {
    match local_ip {
        std::net::IpAddr::V4(v4) => {
            // Network byte order.
            let raw = u32::from_be_bytes(v4.octets());
            CMsgIpAddress {
                ip: Some(c_msg_ip_address::Ip::V4(raw ^ OBFUSCATION_MASK)),
            }
        }
        std::net::IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            for chunk in octets.chunks_exact_mut(4) {
                chunk[0] ^= 0x0D;
                chunk[1] ^= 0xF0;
                chunk[2] ^= 0xAD;
                chunk[3] ^= 0xBA;
            }
            CMsgIpAddress {
                ip: Some(c_msg_ip_address::Ip::V6(octets.to_vec())),
            }
        }
    }
}

#[async_trait]
impl ClientMsgHandler for SteamUser {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientLogOnResponse => Some(Box::new(LoggedOnCallback::from_packet(packet_msg)?)),
            EMsg::ClientLoggedOff => Some(Box::new(LoggedOffCallback::from_packet(packet_msg)?)),
            EMsg::ClientSessionToken => {
                Some(Box::new(SessionTokenCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientAccountInfo => {
                Some(Box::new(AccountInfoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientEmailAddrInfo => {
                Some(Box::new(EmailAddrInfoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientWalletInfoUpdate => {
                Some(Box::new(WalletInfoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientRequestWebAPIAuthenticateUserNonceResponse => {
                Some(Box::new(WebAPIUserNonceCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientVanityURLChangedNotification => {
                Some(Box::new(VanityURLChangedCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientMarketingMessageUpdate2 => {
                Some(Box::new(MarketingMessageCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientPlayingSessionState => Some(Box::new(
                PlayingSessionStateCallback::from_packet(packet_msg)?,
            )),
            // Not a message we handle.
            _ => None,
        };

        if let Some(callback) = callback {
            client.post_callback(callback);
        }

        Ok(())
    }
}
