//! Callbacks posted by [`SteamNetworking`](super::steam_networking::SteamNetworking).

use std::any::Any;

use steamkit_protos::steam::CMsgClientNetworkingCertReply;

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::job_id::JobID;

/// The response to
/// [`SteamNetworking::request_networking_certificate`](crate::steam::handlers::steam_networking::steam_networking::SteamNetworking::request_networking_certificate).
/// Use it to fill in a `CMsgSteamDatagramCertificateSigned` for socket
/// communication.
#[derive(Clone, Debug)]
pub struct NetworkingCertificateCallback {
    job_id: JobID,
    certificate: Vec<u8>,
    ca_key_id: u64,
    ca_signature: Vec<u8>,
}

impl NetworkingCertificateCallback {
    /// Builds the callback from a `ClientNetworkingCertRequest` response.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let resp = ClientMsgProtobuf::<CMsgClientNetworkingCertReply>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            certificate: resp.body.cert.unwrap_or_default(),
            ca_key_id: resp.body.ca_key_id.unwrap_or_default(),
            ca_signature: resp.body.ca_signature.unwrap_or_default(),
        })
    }

    /// The certificate signed by the Steam CA. Contains a
    /// `CMsgSteamDatagramCertificate` with the public key you sent.
    pub fn certificate(&self) -> &[u8] {
        &self.certificate
    }

    /// The ID of the CA used to sign this certificate.
    pub fn ca_key_id(&self) -> u64 {
        self.ca_key_id
    }

    /// The signature used to verify the certificate.
    pub fn ca_signature(&self) -> &[u8] {
        &self.ca_signature
    }
}

impl CallbackMsg for NetworkingCertificateCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
