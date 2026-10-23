//! Game coordinator messaging: sends GC messages to an app's game coordinator
//! and posts the replies as callbacks.

use std::fmt;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::CMsgGcClient;

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::gc::msg_base_gc::IClientGCMsg;
use crate::base::generated::steam_language::EMsg;
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::util::msg_util::make_gc_msg;

use super::callbacks::MessageCallback;

/// Handles game coordinator messaging.
pub struct SteamGameCoordinator {
    client: Weak<SteamClient>,
}

impl fmt::Debug for SteamGameCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamGameCoordinator")
            .finish_non_exhaustive()
    }
}

impl SteamGameCoordinator {
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

    /// Sends a game coordinator message to the GC of `app_id`.
    pub fn send(&self, msg: &dyn IClientGCMsg, app_id: u32) -> SteamResult<()> {
        let client = self.client()?;

        let mut client_msg = ClientMsgProtobuf::<CMsgGcClient>::new(EMsg::ClientToGC);

        client_msg.proto_header_mut().routing_appid = Some(app_id);
        client_msg.body.msgtype = Some(make_gc_msg(msg.msg_type(), msg.is_proto()));
        client_msg.body.appid = Some(app_id);
        client_msg.body.payload = Some(msg.serialize());

        client.send(&client_msg)
    }
}

#[async_trait]
impl ClientMsgHandler for SteamGameCoordinator {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        if packet_msg.msg_type() == EMsg::ClientFromGC {
            let callback = MessageCallback::from_packet(packet_msg)?;
            client.post_callback(Box::new(callback));
        }

        Ok(())
    }
}
