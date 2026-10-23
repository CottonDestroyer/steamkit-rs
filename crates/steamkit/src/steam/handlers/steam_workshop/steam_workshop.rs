//! Requesting files published on the Steam Workshop.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::CMsgClientUcmEnumeratePublishedFilesByUserAction;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{EMsg, EWorkshopFileAction};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;

use super::callbacks::UserActionPublishedFilesCallback;

/// What to enumerate of the local user's workshop files.
#[derive(Clone, Debug)]
pub struct EnumerationUserDetails {
    /// The app whose workshop to enumerate.
    pub app_id: u32,

    /// The start index.
    pub start_index: u32,

    /// The user action to filter by. Only used by
    /// [`SteamWorkshop::enumerate_published_files_by_user_action`].
    pub user_action: EWorkshopFileAction,
}

impl Default for EnumerationUserDetails {
    fn default() -> Self {
        Self {
            app_id: 0,
            start_index: 0,
            // The first member of the enum.
            user_action: EWorkshopFileAction::Played,
        }
    }
}

/// Requests files published on the Steam Workshop.
pub struct SteamWorkshop {
    client: Weak<SteamClient>,
}

impl SteamWorkshop {
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

    /// Lists the current user's published files, filtered by user action.
    pub async fn enumerate_published_files_by_user_action(
        &self,
        details: &EnumerationUserDetails,
    ) -> SteamResult<UserActionPublishedFilesCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<UserActionPublishedFilesCallback>(job_id);

        let mut enum_request =
            ClientMsgProtobuf::<CMsgClientUcmEnumeratePublishedFilesByUserAction>::new(
                EMsg::ClientUCMEnumeratePublishedFilesByUserAction,
            );
        enum_request.set_source_job_id(job_id);

        enum_request.body.action = Some(details.user_action.value());
        enum_request.body.app_id = Some(details.app_id);
        enum_request.body.start_index = Some(details.start_index);

        client.send(&enum_request)?;

        job.wait().await
    }
}

#[async_trait]
impl ClientMsgHandler for SteamWorkshop {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientUCMEnumeratePublishedFilesByUserActionResponse => Some(Box::new(
                UserActionPublishedFilesCallback::from_packet(packet_msg)?,
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
