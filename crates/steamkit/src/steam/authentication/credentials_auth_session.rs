//! An authentication session started with a username and password.

use std::sync::Arc;
use std::time::Duration;

use steamkit_protos::steam::{CAuthenticationAllowedConfirmation, EAuthSessionGuardType};
use tokio_util::sync::CancellationToken;

use crate::error::SteamError;
use crate::types::steam_id::SteamID;

use super::auth_poll_result::AuthPollResult;
use super::auth_session::AuthSession;
use super::i_authenticator::IAuthenticator;

/// An authentication session started with a username and password.
pub struct CredentialsAuthSession {
    session: AuthSession,
}

impl std::fmt::Debug for CredentialsAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialsAuthSession")
            .field("session", &self.session)
            .finish()
    }
}

impl CredentialsAuthSession {
    /// Creates a credentials session.
    pub(crate) fn new(session: AuthSession) -> Self {
        CredentialsAuthSession { session }
    }

    /// SteamID of the account logging in. Only set if the credentials were
    /// correct.
    pub fn steam_id(&self) -> SteamID {
        self.session.steam_id()
    }

    /// Sends a Steam Guard code of the given type for this session.
    pub async fn send_steam_guard_code(
        &self,
        code: &str,
        code_type: EAuthSessionGuardType,
    ) -> Result<(), SteamError> {
        self.session.send_steam_guard_code(code, code_type).await?;

        Ok(())
    }

    /// The authenticator used for 2-factor authentication, if any.
    pub fn authenticator(&self) -> Option<&Arc<dyn IAuthenticator>> {
        self.session.authenticator()
    }

    /// Unique id of the requestor. Also used for routing, and part of the QR code.
    pub fn client_id(&self) -> u64 {
        self.session.client_id()
    }

    /// Request id to present when polling.
    pub fn request_id(&self) -> &[u8] {
        self.session.request_id()
    }

    /// How often to call `poll_auth_session_status`.
    pub fn polling_interval(&self) -> Duration {
        self.session.polling_interval()
    }

    /// The confirmation types that can confirm the request.
    pub fn allowed_confirmations(&self) -> &[CAuthenticationAllowedConfirmation] {
        self.session.allowed_confirmations()
    }

    /// Handles any 2-factor authentication and polls until authentication
    /// succeeds.
    pub async fn polling_wait_for_result(
        &self,
        cancellation: CancellationToken,
    ) -> Result<AuthPollResult, SteamError> {
        self.session.polling_wait_for_result(cancellation).await
    }

    /// Polls the authentication status once.
    pub async fn poll_auth_session_status(&self) -> Result<Option<AuthPollResult>, SteamError> {
        self.session.poll_auth_session_status().await
    }
}
