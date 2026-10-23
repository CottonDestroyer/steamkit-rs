//! An [`IAuthenticator`] that asks for the codes on the console.
//!
//! Input is read with `tokio::io::stdin`, so a worker thread isn't blocked
//! while the user types.

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdin};
use tokio::sync::Mutex;

use crate::error::SteamError;

use super::i_authenticator::IAuthenticator;

/// An [`IAuthenticator`] that asks for the 2-factor codes on the console.
#[derive(Debug)]
pub struct UserConsoleAuthenticator {
    reader: Mutex<BufReader<Stdin>>,
}

impl Default for UserConsoleAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl UserConsoleAuthenticator {
    /// Creates a new authenticator.
    pub fn new() -> Self {
        UserConsoleAuthenticator {
            reader: Mutex::new(BufReader::new(tokio::io::stdin())),
        }
    }

    /// Reads a line from the console. Returns `None` at the end of input.
    async fn read_line(&self) -> Result<Option<String>, SteamError> {
        let mut line = String::new();

        let read = {
            let mut reader = self.reader.lock().await;
            reader.read_line(&mut line).await?
        };

        if read == 0 {
            return Ok(None);
        }

        Ok(Some(line.trim().to_string()))
    }

    /// Writes a prompt to stderr.
    async fn write_prompt(prompt: &str) -> Result<(), SteamError> {
        let mut stderr = tokio::io::stderr();
        stderr.write_all(prompt.as_bytes()).await?;
        stderr.flush().await?;

        Ok(())
    }
}

#[async_trait]
impl IAuthenticator for UserConsoleAuthenticator {
    /// Prompts the user for the 2-factor code of their authenticator app.
    async fn get_device_code(
        &self,
        previous_code_was_incorrect: bool,
    ) -> Result<String, SteamError> {
        if previous_code_was_incorrect {
            Self::write_prompt("The previous 2-factor auth code you have provided is incorrect.\n")
                .await?;
        }

        loop {
            Self::write_prompt(
                "STEAM GUARD! Please enter your 2-factor auth code from your authenticator app: ",
            )
            .await?;

            let Some(code) = self.read_line().await? else {
                return Ok(String::new());
            };

            if !code.is_empty() {
                return Ok(code);
            }
        }
    }

    /// Prompts the user for the Steam Guard code that was sent to their email.
    async fn get_email_code(
        &self,
        email: &str,
        previous_code_was_incorrect: bool,
    ) -> Result<String, SteamError> {
        if previous_code_was_incorrect {
            Self::write_prompt("The previous 2-factor auth code you have provided is incorrect.\n")
                .await?;
        }

        loop {
            Self::write_prompt(&format!(
                "STEAM GUARD! Please enter the auth code sent to the email at {email}: "
            ))
            .await?;

            let Some(code) = self.read_line().await? else {
                return Ok(String::new());
            };

            if !code.is_empty() {
                return Ok(code);
            }
        }
    }

    /// Tells the user to confirm the sign-in in the Steam mobile app.
    async fn accept_device_confirmation(&self) -> Result<bool, SteamError> {
        Self::write_prompt("STEAM GUARD! Use the Steam Mobile App to confirm your sign in...\n")
            .await?;

        Ok(true)
    }
}
