//! What's needed to start an authentication session.

use std::sync::Arc;

use steamkit_protos::steam::EAuthTokenPlatformType;

use crate::base::generated::steam_language::EOSType;
use crate::util::utils::Utils;

use super::i_authenticator::IAuthenticator;

/// The details needed to authenticate with Steam.
#[derive(Clone)]
pub struct AuthSessionDetails {
    /// The username of the account that is logging on.
    pub username: Option<String>,

    /// The password of the account that is logging on.
    pub password: Option<String>,

    /// The device name (or user agent) of the client.
    pub device_friendly_name: Option<String>,

    /// The platform type that the login will be performed for.
    pub platform_type: EAuthTokenPlatformType,

    /// The client operating system type.
    pub client_os_type: EOSType,

    /// Whether the session should persist.
    pub is_persistent_session: bool,

    /// The website id the login is for. Known values: "Unknown", "Client",
    /// "Mobile", "Website", "Store", "Community", "Partner", "SteamStats".
    pub website_id: Option<String>,

    /// Steam Guard data for client login. Pass
    /// [`AuthPollResult::new_guard_data`](crate::steam::authentication::auth_poll_result::AuthPollResult::new_guard_data)
    /// from an earlier login, if you have it.
    pub guard_data: Option<String>,

    /// The authenticator used for 2-factor authentication, if any.
    pub authenticator: Option<Arc<dyn IAuthenticator>>,
}

impl Default for AuthSessionDetails {
    /// Same as [`AuthSessionDetails::new`].
    fn default() -> Self {
        AuthSessionDetails {
            username: None,
            password: None,
            device_friendly_name: Some(format!("{} (SteamKit2)", machine_name())),
            platform_type: EAuthTokenPlatformType::KEAuthTokenPlatformTypeSteamClient,
            client_os_type: Utils::get_os_type(),
            is_persistent_session: false,
            website_id: Some("Client".to_string()),
            guard_data: None,
            authenticator: None,
        }
    }
}

impl AuthSessionDetails {
    /// Details with the usual defaults: Steam client platform, website
    /// "Client", a non-persistent session, and "`<machine name>` (SteamKit2)" as
    /// the device name.
    pub fn new() -> Self {
        AuthSessionDetails::default()
    }
}

impl std::fmt::Debug for AuthSessionDetails {
    /// Redacts the password and doesn't try to print the authenticator.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSessionDetails")
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("device_friendly_name", &self.device_friendly_name)
            .field("platform_type", &self.platform_type)
            .field("client_os_type", &self.client_os_type)
            .field("is_persistent_session", &self.is_persistent_session)
            .field("website_id", &self.website_id)
            .field("guard_data", &self.guard_data)
            .field(
                "authenticator",
                &self.authenticator.as_ref().map(|_| "Arc<dyn IAuthenticator>"),
            )
            .finish()
    }
}

/// The name of this machine.
fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
