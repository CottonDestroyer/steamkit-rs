//! An authentication session started with a QR code.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::error::SteamError;

use super::auth_poll_result::AuthPollResult;
use super::auth_session::AuthSession;
use super::i_authenticator::IAuthenticator;

/// The part of a QR session that its [`AuthSession`] also needs.
pub(crate) struct QrAuthSessionState {
    challenge_url: RwLock<String>,
    changed: broadcast::Sender<String>,
}

impl QrAuthSessionState {
    /// Creates the state for a session with the given challenge URL.
    pub(crate) fn new(challenge_url: String) -> Self {
        let (changed, _) = broadcast::channel(16);

        QrAuthSessionState {
            challenge_url: RwLock::new(challenge_url),
            changed,
        }
    }

    /// The current challenge URL.
    pub(crate) fn challenge_url(&self) -> String {
        self.challenge_url.read().clone()
    }

    /// Stores a new challenge URL and notifies subscribers.
    pub(crate) fn set_challenge_url(&self, challenge_url: String) {
        *self.challenge_url.write() = challenge_url.clone();

        // Sending only fails when nobody's subscribed, which is fine.
        let _ = self.changed.send(challenge_url);
    }

    /// Subscribes to challenge URL changes.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.changed.subscribe()
    }
}

/// An authentication session started with a QR code.
pub struct QrAuthSession {
    session: AuthSession,
    state: Arc<QrAuthSessionState>,
}

impl std::fmt::Debug for QrAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QrAuthSession")
            .field("session", &self.session)
            .field("challenge_url", &self.challenge_url())
            .finish()
    }
}

impl QrAuthSession {
    /// Creates a QR session.
    pub(crate) fn new(session: AuthSession, state: Arc<QrAuthSessionState>) -> Self {
        QrAuthSession { session, state }
    }

    /// The challenge URL, to be shown as a QR code.
    pub fn challenge_url(&self) -> String {
        self.state.challenge_url()
    }

    /// Subscribes to challenge URL changes. Steam refreshes the URL every so
    /// often; each change delivers the new URL, so redraw the QR code.
    pub fn challenge_url_changed(&self) -> broadcast::Receiver<String> {
        self.state.subscribe()
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
    pub fn allowed_confirmations(
        &self,
    ) -> &[steamkit_protos::steam::CAuthenticationAllowedConfirmation] {
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
