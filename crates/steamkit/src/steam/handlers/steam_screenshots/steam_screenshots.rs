//! Adding screenshots to the user's screenshot library.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use steamkit_protos::steam::CMsgClientUcmAddScreenshot;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{EMsg, EUCMFilePrivacyState};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::game_id::GameID;

use super::callbacks::ScreenshotAddedCallback;

/// The width of a screenshot thumbnail.
pub const SCREENSHOT_THUMBNAIL_WIDTH: u32 = 200;

/// What's needed to add a screenshot.
#[derive(Clone, Debug)]
pub struct ScreenshotDetails {
    /// The Steam game ID this screenshot belongs to.
    pub game_id: Option<GameID>,

    /// The UFS path of the image.
    pub ufs_image_file_path: Option<String>,

    /// The UFS path of the thumbnail.
    pub ufs_thumbnail_file_path: Option<String>,

    /// The screenshot caption.
    pub caption: Option<String>,

    /// The screenshot privacy.
    pub privacy: EUCMFilePrivacyState,

    /// The screenshot width.
    pub width: u32,

    /// The screenshot height.
    pub height: u32,

    /// The creation time.
    pub creation_time: DateTime<Utc>,

    /// Whether the screenshot contains spoilers.
    pub contains_spoilers: bool,
}

impl Default for ScreenshotDetails {
    fn default() -> Self {
        Self {
            game_id: None,
            ufs_image_file_path: None,
            ufs_thumbnail_file_path: None,
            caption: None,
            // Means "not set"; the generated enum has no 0 member, so
            // `Invalid` stands in for it.
            privacy: EUCMFilePrivacyState::Invalid,
            width: 0,
            height: 0,
            // The Unix epoch, so `rtime32_created` is 0 ("not set").
            creation_time: DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(|| {
                DateTime::<Utc>::from_naive_utc_and_offset(NaiveDateTime::default(), Utc)
            }),
            contains_spoilers: false,
        }
    }
}

/// Adds screenshots to the user's screenshot library.
pub struct SteamScreenshots {
    client: Weak<SteamClient>,
}

impl SteamScreenshots {
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

    /// Adds a screenshot to the user's library. The image and thumbnail must
    /// already be on the UFS.
    pub async fn add_screenshot(
        &self,
        details: &ScreenshotDetails,
    ) -> SteamResult<ScreenshotAddedCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<ScreenshotAddedCallback>(job_id);

        let mut msg =
            ClientMsgProtobuf::<CMsgClientUcmAddScreenshot>::new(EMsg::ClientUCMAddScreenshot);
        msg.set_source_job_id(job_id);

        if let Some(game_id) = &details.game_id {
            msg.body.appid = Some(game_id.app_id());
        }

        msg.body.caption = details.caption.clone();
        msg.body.filename = details.ufs_image_file_path.clone();

        // `Invalid` means "not set", so leave the field out of the message.
        if details.privacy != EUCMFilePrivacyState::Invalid {
            msg.body.permissions = Some(details.privacy.value() as u32);
        }

        msg.body.thumbname = details.ufs_thumbnail_file_path.clone();
        msg.body.width = Some(details.width);
        msg.body.height = Some(details.height);
        msg.body.rtime32_created = Some(details.creation_time.timestamp() as u32);
        msg.body.spoiler_tag = Some(details.contains_spoilers);

        client.send(&msg)?;

        job.wait().await
    }
}

#[async_trait]
impl ClientMsgHandler for SteamScreenshots {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientUCMAddScreenshotResponse => {
                Some(Box::new(ScreenshotAddedCallback::from_packet(packet_msg)?))
            }
            // Not a message we handle.
            _ => None,
        };

        if let Some(callback) = callback {
            client.post_callback(callback);
        }

        Ok(())
    }
}
