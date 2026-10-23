//! An authentication session, used to finish authenticating and get the tokens.
//!
//! Credentials and QR sessions share [`AuthSession`]; what differs between them
//! lives in [`SessionKind`], and the public session types wrap an [`AuthSession`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use steamkit_protos::steam::{
    CAuthenticationAllowedConfirmation, CAuthenticationPollAuthSessionStatusResponse,
    EAuthSessionGuardType,
};
use tokio_util::sync::CancellationToken;

use crate::base::generated::steam_language::EResult;
use crate::error::SteamError;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::steam_id::SteamID;

use super::auth_poll_result::AuthPollResult;
use super::authentication_exception::AuthenticationException;
use super::i_authenticator::IAuthenticator;
use super::qr_auth_session::QrAuthSessionState;

/// What differs between a credentials session and a QR session.
pub(crate) enum SessionKind {
    /// A session that was started with a username and a password.
    Credentials {
        /// The SteamID of the account that is logging on.
        steam_id: SteamID,
    },

    /// A session that was started with a QR code.
    Qr {
        /// Holds the challenge URL of the session.
        state: Arc<QrAuthSessionState>,
    },
}

/// An authentication session, used to finish authenticating and get the tokens.
pub struct AuthSession {
    client: Weak<SteamClient>,
    authenticator: Option<Arc<dyn IAuthenticator>>,
    client_id: AtomicU64,
    request_id: Vec<u8>,
    allowed_confirmations: Vec<CAuthenticationAllowedConfirmation>,
    polling_interval: Duration,
    kind: SessionKind,
}

impl std::fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSession")
            .field("client_id", &self.client_id())
            .field("request_id", &self.request_id)
            .field("polling_interval", &self.polling_interval)
            .field(
                "allowed_confirmations",
                &self.allowed_confirmations.len(),
            )
            .finish()
    }
}

impl AuthSession {
    /// Creates an authentication session.
    pub(crate) fn new(
        client: Weak<SteamClient>,
        authenticator: Option<Arc<dyn IAuthenticator>>,
        client_id: u64,
        request_id: Vec<u8>,
        allowed_confirmations: Vec<CAuthenticationAllowedConfirmation>,
        polling_interval: f32,
        kind: SessionKind,
    ) -> Self {
        AuthSession {
            client,
            authenticator,
            client_id: AtomicU64::new(client_id),
            request_id,
            allowed_confirmations: sort_confirmations(allowed_confirmations),
            polling_interval: Duration::from_secs_f64(f64::from(polling_interval)),
            kind,
        }
    }

    /// The authenticator used for 2-factor authentication, if any.
    pub fn authenticator(&self) -> Option<&Arc<dyn IAuthenticator>> {
        self.authenticator.as_ref()
    }

    /// Unique id of the requestor. Also used for routing, and part of the QR code.
    pub fn client_id(&self) -> u64 {
        self.client_id.load(Ordering::Relaxed)
    }

    /// Request id to present when polling.
    pub fn request_id(&self) -> &[u8] {
        &self.request_id
    }

    /// How often to call `poll_auth_session_status`.
    pub fn polling_interval(&self) -> Duration {
        self.polling_interval
    }

    /// The confirmation types that can confirm the request.
    pub fn allowed_confirmations(&self) -> &[CAuthenticationAllowedConfirmation] {
        &self.allowed_confirmations
    }

    /// The SteamID of the account logging on. QR sessions don't have one yet,
    /// so they report an empty SteamID.
    pub(crate) fn steam_id(&self) -> SteamID {
        match &self.kind {
            SessionKind::Credentials { steam_id } => *steam_id,
            SessionKind::Qr { .. } => SteamID::new(),
        }
    }

    /// Handles any 2-factor authentication and polls until authentication
    /// succeeds.
    pub async fn polling_wait_for_result(
        &self,
        cancellation: CancellationToken,
    ) -> Result<AuthPollResult, SteamError> {
        let mut poll_loop = false;

        let Some(first_confirmation) = self.allowed_confirmations.first().cloned() else {
            return Err(SteamError::InvalidArgument(
                "There are no allowed confirmations".to_string(),
            ));
        };

        let mut preferred_confirmation = first_confirmation;

        if confirmation_type(&preferred_confirmation)
            == EAuthSessionGuardType::KEAuthSessionGuardTypeUnknown
        {
            return Err(SteamError::InvalidArgument(
                "There are no allowed confirmations".to_string(),
            ));
        }

        // With an authenticator and device confirmation available, let the
        // authenticator decide: wait for the confirmation in the app, or fall
        // back to the next confirmation type.
        if let Some(authenticator) = self.authenticator.as_ref() {
            if confirmation_type(&preferred_confirmation)
                == EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceConfirmation
            {
                let prefers_to_poll_for_confirmation =
                    authenticator.accept_device_confirmation().await?;

                if !prefers_to_poll_for_confirmation {
                    if self.allowed_confirmations.len() <= 1 {
                        return Err(SteamError::InvalidArgument(
                            "AcceptDeviceConfirmation returned false which indicates a fallback to another confirmation type, but there are no other confirmation types available.".to_string(),
                        ));
                    }

                    preferred_confirmation = self.allowed_confirmations[1].clone();
                }
            }
        }

        let guard_type = confirmation_type(&preferred_confirmation);

        match guard_type {
            // No Steam Guard.
            EAuthSessionGuardType::KEAuthSessionGuardTypeNone => {}

            // 2-factor code from the authenticator app, or sent by email.
            EAuthSessionGuardType::KEAuthSessionGuardTypeEmailCode
            | EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceCode => {
                let Some(authenticator) = self.authenticator.as_ref() else {
                    return Err(SteamError::InvalidArgument(
                        "This account requires an authenticator for login, but none was provided in AuthSessionDetails.".to_string(),
                    ));
                };

                let expected_invalid_code_result = match guard_type {
                    EAuthSessionGuardType::KEAuthSessionGuardTypeEmailCode => {
                        EResult::InvalidLoginAuthCode
                    }
                    EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceCode => {
                        EResult::TwoFactorCodeMismatch
                    }
                    other => {
                        return Err(SteamError::InvalidArgument(format!(
                            "Unsupported confirmation type {other:?}."
                        )))
                    }
                };

                let mut previous_code_was_incorrect = false;
                let mut waiting_for_valid_code = true;

                while waiting_for_valid_code {
                    if cancellation.is_cancelled() { return Err(SteamError::Cancelled); }

                    let code = match guard_type {
                        EAuthSessionGuardType::KEAuthSessionGuardTypeEmailCode => {
                            let email = preferred_confirmation
                                .associated_message
                                .as_deref()
                                .unwrap_or_default();

                            authenticator
                                .get_email_code(email, previous_code_was_incorrect)
                                .await?
                        }
                        EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceCode => {
                            authenticator
                                .get_device_code(previous_code_was_incorrect)
                                .await?
                        }
                        other => {
                            return Err(SteamError::InvalidArgument(format!(
                                "Unsupported confirmation type {other:?}."
                            )))
                        }
                    };

                    if cancellation.is_cancelled() { return Err(SteamError::Cancelled); }

                    if code.is_empty() {
                        return Err(SteamError::InvalidArgument(
                            "No code was provided by the authenticator.".to_string(),
                        ));
                    }

                    match self.send_steam_guard_code(&code, guard_type).await {
                        Ok(()) => waiting_for_valid_code = false,
                        Err(error) => {
                            if error.result() == Some(expected_invalid_code_result) {
                                previous_code_was_incorrect = true;
                            } else {
                                return Err(error.into());
                            }
                        }
                    }
                }
            }

            // The prompt in the Steam mobile app.
            EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceConfirmation => {
                poll_loop = true;
            }

            other => {
                return Err(SteamError::InvalidArgument(format!(
                    "Unsupported confirmation type {other:?}."
                )))
            }
        }

        if !poll_loop {
            if cancellation.is_cancelled() { return Err(SteamError::Cancelled); }

            let poll_response = self
                .poll_auth_session_status_raw()
                .await?
                .ok_or_else(|| {
                    SteamError::Authentication(
                        AuthenticationException::failed("Authentication failed", EResult::Fail)
                            .to_string(),
                    )
                })?;

            return Ok(poll_response);
        }

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => return Err(SteamError::Cancelled),
                _ = tokio::time::sleep(self.polling_interval) => {}
            }

            match self.poll_auth_session_status_raw().await {
                Ok(Some(poll_response)) => return Ok(poll_response),
                Ok(None) => {}
                Err(error) => {
                    // An error with a Steam result code is Steam rejecting the
                    // session, so stop. Without one it's a transport error (the
                    // CM connection dropped mid-request). The auth session lives
                    // on Steam's side and survives that, so just poll again.
                    if error.result().is_some() {
                        return Err(error.into());
                    }
                }
            }
        }
    }

    /// Polls the authentication status once. You probably want
    /// [`AuthSession::polling_wait_for_result`] instead.
    ///
    /// Returns the tokens for logging on, or `None` if the session isn't
    /// authenticated yet.
    pub async fn poll_auth_session_status(&self) -> Result<Option<AuthPollResult>, SteamError> {
        self.poll_auth_session_status_raw().await.map_err(Into::into)
    }

    /// Polls the authentication status once, keeping the Steam result code of
    /// a failure.
    pub(crate) async fn poll_auth_session_status_raw(
        &self,
    ) -> Result<Option<AuthPollResult>, AuthenticationException> {
        let client = self.client.upgrade().ok_or_else(|| {
            AuthenticationException::other("The SteamClient instance has been dropped.")
        })?;

        let response = client
            .authentication()
            .poll_auth_session_status(self.client_id(), self.request_id.clone())
            .await?;

        self.handle_poll_auth_session_status_response(&response);

        let refresh_token = response.refresh_token.clone().unwrap_or_default();
        if !refresh_token.is_empty() {
            return Ok(Some(AuthPollResult::from_response(&response)));
        }

        Ok(None)
    }

    /// Handles a poll response. QR sessions also pick up a refreshed challenge
    /// URL here (see [`SessionKind::Qr`]).
    pub fn handle_poll_auth_session_status_response(
        &self,
        response: &CAuthenticationPollAuthSessionStatusResponse,
    ) {
        if let Some(new_client_id) = response.new_client_id {
            if new_client_id != 0 {
                self.client_id.store(new_client_id, Ordering::Relaxed);
            }
        }

        if let SessionKind::Qr { state } = &self.kind {
            if let Some(challenge_url) = &response.new_challenge_url {
                if !challenge_url.is_empty() {
                    state.set_challenge_url(challenge_url.clone());
                }
            }
        }
    }

    /// Sends a Steam Guard code for this session.
    pub(crate) async fn send_steam_guard_code(
        &self,
        code: &str,
        code_type: EAuthSessionGuardType,
    ) -> Result<(), AuthenticationException> {
        let SessionKind::Credentials { steam_id } = &self.kind else {
            return Err(AuthenticationException::other(format!(
                "Got {code_type:?} confirmation type in a session that is not CredentialsAuthSession."
            )));
        };

        let client = self.client.upgrade().ok_or_else(|| {
            AuthenticationException::other("The SteamClient instance has been dropped.")
        })?;

        client
            .authentication()
            .send_steam_guard_code(self.client_id(), *steam_id, code, code_type)
            .await
    }
}

/// Reads the confirmation type of an allowed confirmation.
fn confirmation_type(confirmation: &CAuthenticationAllowedConfirmation) -> EAuthSessionGuardType {
    match confirmation.confirmation_type {
        Some(1) => EAuthSessionGuardType::KEAuthSessionGuardTypeNone,
        Some(2) => EAuthSessionGuardType::KEAuthSessionGuardTypeEmailCode,
        Some(3) => EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceCode,
        Some(4) => EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceConfirmation,
        Some(5) => EAuthSessionGuardType::KEAuthSessionGuardTypeEmailConfirmation,
        Some(6) => EAuthSessionGuardType::KEAuthSessionGuardTypeMachineToken,
        Some(7) => EAuthSessionGuardType::KEAuthSessionGuardTypeLegacyMachineAuth,
        _ => EAuthSessionGuardType::KEAuthSessionGuardTypeUnknown,
    }
}

/// Sorts the available confirmation types by preference.
fn sort_confirmations(
    confirmations: Vec<CAuthenticationAllowedConfirmation>,
) -> Vec<CAuthenticationAllowedConfirmation> {
    let preferred_confirmation_types = [
        EAuthSessionGuardType::KEAuthSessionGuardTypeNone,
        EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceConfirmation,
        EAuthSessionGuardType::KEAuthSessionGuardTypeDeviceCode,
        EAuthSessionGuardType::KEAuthSessionGuardTypeEmailCode,
        EAuthSessionGuardType::KEAuthSessionGuardTypeEmailConfirmation,
        EAuthSessionGuardType::KEAuthSessionGuardTypeMachineToken,
        EAuthSessionGuardType::KEAuthSessionGuardTypeUnknown,
    ];

    let mut sorted = confirmations;
    sorted.sort_by_key(|confirmation| {
        let confirmation_type = confirmation_type(confirmation);

        preferred_confirmation_types
            .iter()
            .position(|preferred| *preferred == confirmation_type)
            .unwrap_or(usize::MAX)
    });

    sorted
}
