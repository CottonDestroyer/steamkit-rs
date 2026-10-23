//! The trait every message handler implements.
//!
//! Handlers keep a `Weak<SteamClient>` back-reference, and the client is also
//! passed to [`ClientMsgHandler::handle_msg`], so a handler can reply without
//! holding a strong reference to the client (which would be a cycle).
//!
//! To expect a disconnect (e.g. before logging off), handlers call
//! `SteamClient::set_expect_disconnection`.

use async_trait::async_trait;

use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::steam_client::SteamClient;

/// A message handler. It gets the incoming messages it cares about and replies
/// through the [`SteamClient`].
#[async_trait]
pub trait ClientMsgHandler: Send + Sync {
    /// Handles a client message. Called by the client; don't call it yourself.
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()>;
}
