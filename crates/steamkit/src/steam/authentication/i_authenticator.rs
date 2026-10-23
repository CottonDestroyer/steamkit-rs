//! The trait for answering Steam Guard prompts during authentication.

use async_trait::async_trait;

use crate::error::SteamError;

/// Answers Steam Guard prompts for `SteamAuthentication`.
#[async_trait]
pub trait IAuthenticator: Send + Sync {
    /// Called when the account needs a code from the authenticator app.
    /// `previous_code_was_incorrect` is `true` if the last code was rejected.
    async fn get_device_code(
        &self,
        previous_code_was_incorrect: bool,
    ) -> Result<String, SteamError>;

    /// Called when the account uses email Steam Guard. `email` is where the
    /// code was sent; `previous_code_was_incorrect` is `true` if the last code
    /// was rejected.
    async fn get_email_code(
        &self,
        email: &str,
        previous_code_was_incorrect: bool,
    ) -> Result<String, SteamError>;

    /// Called when the account has the Steam mobile app and can confirm the
    /// login there. Return `false` to fall back to entering a code.
    async fn accept_device_confirmation(&self) -> Result<bool, SteamError>;
}
