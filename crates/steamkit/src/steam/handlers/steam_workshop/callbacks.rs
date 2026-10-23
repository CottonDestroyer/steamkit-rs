//! Callbacks posted by [`SteamWorkshop`](super::steam_workshop::SteamWorkshop).

use std::any::Any;

use chrono::{DateTime, NaiveDateTime, Utc};
use steamkit_protos::steam::{
    c_msg_client_ucm_enumerate_published_files_by_user_action_response,
    CMsgClientUcmEnumeratePublishedFilesByUserActionResponse,
};

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::generated::steam_language::EResult;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::job_id::JobID;
use crate::types::pub_file::PublishedFileID;

/// The response to
/// [`SteamWorkshop::enumerate_published_files_by_user_action`](crate::steam::handlers::steam_workshop::steam_workshop::SteamWorkshop::enumerate_published_files_by_user_action).
#[derive(Clone, Debug)]
pub struct UserActionPublishedFilesCallback {
    job_id: JobID,
    result: EResult,
    files: Vec<File>,
    total_results: i32,
}

/// A single published file.
#[derive(Clone, Debug)]
pub struct File {
    file_id: PublishedFileID,
    timestamp: DateTime<Utc>,
}

impl File {
    /// Builds the file from its protobuf form.
    fn from_file(
        file: &c_msg_client_ucm_enumerate_published_files_by_user_action_response::PublishedFileId,
    ) -> Self {
        let timestamp = u64::from(file.rtime_time_stamp.unwrap_or_default());

        Self {
            file_id: PublishedFileID::from_value(file.published_file_id.unwrap_or_default()),
            timestamp: DateTime::<Utc>::from_timestamp(timestamp as i64, 0)
                .or_else(|| DateTime::<Utc>::from_timestamp(0, 0))
                .unwrap_or_else(|| {
                    DateTime::<Utc>::from_naive_utc_and_offset(NaiveDateTime::default(), Utc)
                }),
        }
    }

    /// The file ID.
    pub fn file_id(&self) -> PublishedFileID {
        self.file_id
    }

    /// The timestamp of this file.
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }
}

impl UserActionPublishedFilesCallback {
    /// Builds the callback from a `ClientUCMEnumeratePublishedFilesByUserAction`
    /// response.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response =
            ClientMsgProtobuf::<CMsgClientUcmEnumeratePublishedFilesByUserActionResponse>::from_packet(
                packet_msg,
            )?;

        let files = response
            .body
            .published_files
            .iter()
            .map(File::from_file)
            .collect();

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(response.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            files,
            total_results: response.body.total_results.unwrap_or_default() as i32,
        })
    }

    /// The result.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The list of enumerated files.
    pub fn files(&self) -> &[File] {
        &self.files
    }

    /// The total number of results.
    pub fn total_results(&self) -> i32 {
        self.total_results
    }
}

impl CallbackMsg for UserActionPublishedFilesCallback {
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
