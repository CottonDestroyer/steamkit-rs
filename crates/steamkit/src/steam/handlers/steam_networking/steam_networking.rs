//! Steam networking sockets: networking certificates.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::CMsgClientNetworkingCertRequest;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::EMsg;
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;

use super::callbacks::NetworkingCertificateCallback;

/// Steam networking sockets.
pub struct SteamNetworking {
    client: Weak<SteamClient>,
}

impl SteamNetworking {
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

    /// Requests a networking certificate for your Ed25519 public key for the
    /// given app.
    pub async fn request_networking_certificate(
        &self,
        app_id: u32,
        public_key: &[u8],
    ) -> SteamResult<NetworkingCertificateCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<NetworkingCertificateCallback>(job_id);

        let mut msg = ClientMsgProtobuf::<CMsgClientNetworkingCertRequest>::new(
            EMsg::ClientNetworkingCertRequest,
        );
        msg.set_source_job_id(job_id);

        msg.body.app_id = Some(app_id);
        msg.body.key_data = Some(public_key.to_vec());

        client.send(&msg)?;

        job.wait().await
    }
}

#[async_trait]
impl ClientMsgHandler for SteamNetworking {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientNetworkingCertRequestResponse => Some(Box::new(
                NetworkingCertificateCallback::from_packet(packet_msg)?,
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
