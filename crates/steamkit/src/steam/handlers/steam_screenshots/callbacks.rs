//! Callbacks posted by [`SteamScreenshots`](super::steam_screenshots::SteamScreenshots).

use std::any::Any;

use steamkit_protos::steam::CMsgClientUcmAddScreenshotResponse;

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::generated::steam_language::EResult;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::global_id::UGCHandle;
use crate::types::job_id::JobID;

/// The response to adding a screenshot.
#[derive(Clone, Debug)]
pub struct ScreenshotAddedCallback {
    job_id: JobID,
    result: EResult,
    screenshot_id: UGCHandle,
}

impl ScreenshotAddedCallback {
    /// Builds the callback from a received `ClientUCMAddScreenshot` response.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let resp =
            ClientMsgProtobuf::<CMsgClientUcmAddScreenshotResponse>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(resp.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            screenshot_id: UGCHandle::from_value(resp.body.screenshotid.unwrap_or_default()),
        })
    }

    /// The result.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The screenshot ID of the newly added screenshot.
    pub fn screenshot_id(&self) -> UGCHandle {
        self.screenshot_id
    }
}

impl CallbackMsg for ScreenshotAddedCallback {
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
