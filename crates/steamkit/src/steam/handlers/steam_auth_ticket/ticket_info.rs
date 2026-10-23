//! A generated auth session ticket.
//!
//! Dropping a [`TicketInfo`] cancels the ticket (the handler stops sending it
//! to Steam) and wipes its bytes from memory.

use std::fmt;
use std::sync::{Arc, Weak};

use crate::steam::steam_client::steam_client::SteamClient;

use super::steam_auth_ticket::SteamAuthTicket;

/// A valid, accepted auth session ticket.
pub struct TicketInfo {
    client: Weak<SteamClient>,
    app_id: u32,
    ticket: Vec<u8>,
    ticket_crc: u32,
}

impl TicketInfo {
    /// Creates a ticket for the given app.
    pub(crate) fn new(client: &Arc<SteamClient>, app_id: u32, ticket: Vec<u8>) -> Self {
        Self {
            client: Arc::downgrade(client),
            app_id,
            ticket_crc: crc32fast::hash(&ticket),
            ticket,
        }
    }

    /// The app the ticket was generated for.
    pub(crate) fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The ticket bytes.
    pub fn ticket(&self) -> &[u8] {
        &self.ticket
    }

    /// The CRC32 of the ticket.
    pub(crate) fn ticket_crc(&self) -> u32 {
        self.ticket_crc
    }
}

impl fmt::Debug for TicketInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TicketInfo")
            .field("app_id", &self.app_id)
            .field("ticket_crc", &self.ticket_crc)
            .field("ticket_len", &self.ticket.len())
            .finish()
    }
}

impl Drop for TicketInfo {
    fn drop(&mut self) {
        // Cancel the ticket so the handler stops sending it to Steam. The
        // handler is reached through the weak client reference; holding it
        // here would keep the whole client alive as long as the ticket exists.
        if let Some(client) = self.client.upgrade() {
            if let Some(handler) = client.get_handler::<SteamAuthTicket>() {
                handler.cancel_auth_ticket(self);
            }
        }

        // Don't leave the ticket bytes lying around in memory.
        self.ticket.fill(0);
    }
}
