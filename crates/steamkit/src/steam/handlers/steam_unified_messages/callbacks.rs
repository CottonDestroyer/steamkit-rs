//! Callbacks posted by [`SteamUnifiedMessages`](super::steam_unified_messages::SteamUnifiedMessages).

use std::any::Any;

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::generated::steam_language::EResult;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::job_id::JobID;

/// The response to a service method called through
/// [`SteamUnifiedMessages`](crate::steam::handlers::steam_unified_messages::steam_unified_messages::SteamUnifiedMessages).
#[derive(Clone, Debug)]
pub struct ServiceMethodResponse<T> {
    job_id: JobID,
    result: EResult,
    body: T,
}

impl<T> ServiceMethodResponse<T>
where
    T: prost::Message + Default,
{
    /// Builds the callback from a received service method response.
    pub fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let msg = ClientMsgProtobuf::<T>::from_packet(packet_msg)?;

        let job_id = JobID::from_value(msg.header().proto.jobid_target.unwrap_or(u64::MAX));
        let result = EResult::from_value(msg.header().proto.eresult.unwrap_or_default())
            .unwrap_or(EResult::Invalid);

        Ok(Self {
            job_id,
            result,
            body: msg.body,
        })
    }

    /// Builds the callback from the raw parts of a service message.
    pub(crate) fn from_parts(job_id: JobID, result: EResult, body: &[u8]) -> SteamResult<Self> {
        Ok(Self {
            job_id,
            result,
            body: T::decode(body)?,
        })
    }

    /// The result of the message.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The protobuf body.
    pub fn body(&self) -> &T {
        &self.body
    }
}

impl<T> CallbackMsg for ServiceMethodResponse<T>
where
    T: prost::Message + Default + Clone + 'static,
{
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

/// A service notification received through
/// [`SteamUnifiedMessages`](crate::steam::handlers::steam_unified_messages::steam_unified_messages::SteamUnifiedMessages).
#[derive(Clone, Debug)]
pub struct ServiceMethodNotification<T> {
    job_id: JobID,
    job_name: String,
    body: T,
}

impl<T> ServiceMethodNotification<T>
where
    T: prost::Message + Default,
{
    /// Builds the callback from a received service notification.
    pub fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let msg = ClientMsgProtobuf::<T>::from_packet(packet_msg)?;

        let job_name = msg
            .header()
            .proto
            .target_job_name
            .clone()
            .unwrap_or_default();

        Ok(Self {
            // Notifications don't answer a request, so there's no job id.
            job_id: JobID::INVALID,
            job_name,
            body: msg.body,
        })
    }

    /// Builds the callback from the raw parts of a service message.
    pub(crate) fn from_parts(job_name: &str, body: &[u8]) -> SteamResult<Self> {
        Ok(Self {
            job_id: JobID::INVALID,
            job_name: job_name.to_string(),
            body: T::decode(body)?,
        })
    }

    /// The job name, as `Service.Method#Version`.
    pub fn job_name(&self) -> &str {
        &self.job_name
    }

    /// The protobuf body.
    pub fn body(&self) -> &T {
        &self.body
    }
}

impl<T> CallbackMsg for ServiceMethodNotification<T>
where
    T: prost::Message + Default + Clone + 'static,
{
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
