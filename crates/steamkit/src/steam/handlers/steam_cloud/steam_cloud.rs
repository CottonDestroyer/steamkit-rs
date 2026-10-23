//! Remote storage and user generated content, through the `Cloud` unified
//! service.

use std::fmt;
use std::sync::{Arc, OnceLock, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::{
    CCloudGetFileDetailsRequest, CCloudGetFileDetailsResponse, CCloudGetSingleFileInfoRequest,
    CCloudGetSingleFileInfoResponse, CCloudShareFileRequest, CCloudShareFileResponse,
};

use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::handlers::steam_unified_messages::steam_unified_messages::{
    SteamUnifiedMessages, UnifiedService,
};
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::global_id::UGCHandle;

use super::callbacks::{ShareFileCallback, SingleFileInfoCallback, UGCDetailsCallback};

/// The unified service this handler uses.
const SERVICE_NAME: &str = "Cloud";

/// Interacts with remote storage and user generated content.
pub struct SteamCloud {
    client: Weak<SteamClient>,
    service: OnceLock<Arc<UnifiedService>>,
}

impl fmt::Debug for SteamCloud {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamCloud").finish_non_exhaustive()
    }
}

impl SteamCloud {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
            service: OnceLock::new(),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client
            .upgrade()
            .ok_or_else(|| SteamError::Connection("SteamClient has been dropped".to_string()))
    }

    /// The `Cloud` unified service.
    fn service(&self) -> SteamResult<Arc<UnifiedService>> {
        if let Some(service) = self.service.get() {
            return Ok(Arc::clone(service));
        }

        let client = self.client()?;
        let unified_messages = client.get_required_handler::<SteamUnifiedMessages>()?;
        let service = unified_messages.create_service(SERVICE_NAME)?;

        service.set_response_type::<CCloudGetFileDetailsResponse>("GetFileDetails");
        service.set_response_type::<CCloudGetSingleFileInfoResponse>("GetSingleFileInfo");
        service.set_response_type::<CCloudShareFileResponse>("ShareFile");

        let _ = self.service.set(Arc::clone(&service));

        Ok(service)
    }

    /// Requests the details of a piece of user generated content.
    pub async fn request_ugc_details(&self, ugc_id: UGCHandle) -> SteamResult<UGCDetailsCallback> {
        let service = self.service()?;

        let request = CCloudGetFileDetailsRequest {
            ugcid: Some(ugc_id.value()),
            ..Default::default()
        };

        let response = service
            .send_message::<CCloudGetFileDetailsRequest, CCloudGetFileDetailsResponse>(
                "GetFileDetails",
                request,
            )
            .await?;

        Ok(UGCDetailsCallback::from_response(
            response.result(),
            response.body().clone(),
        ))
    }

    /// Requests the details of a file in the user's cloud storage.
    pub async fn get_single_file_info(
        &self,
        app_id: u32,
        filename: &str,
    ) -> SteamResult<SingleFileInfoCallback> {
        let service = self.service()?;

        let request = CCloudGetSingleFileInfoRequest {
            app_id: Some(app_id),
            file_name: Some(filename.to_string()),
            ..Default::default()
        };

        let response = service
            .send_message::<CCloudGetSingleFileInfoRequest, CCloudGetSingleFileInfoResponse>(
                "GetSingleFileInfo",
                request,
            )
            .await?;

        Ok(SingleFileInfoCallback::from_response(
            response.result(),
            response.body().clone(),
        ))
    }

    /// Commits the cloud file at `filename` so its UGC handle becomes public.
    pub async fn share_file(&self, app_id: u32, filename: &str) -> SteamResult<ShareFileCallback> {
        let service = self.service()?;

        let request = CCloudShareFileRequest {
            app_id: Some(app_id),
            file_name: Some(filename.to_string()),
            ..Default::default()
        };

        let response = service
            .send_message::<CCloudShareFileRequest, CCloudShareFileResponse>("ShareFile", request)
            .await?;

        Ok(ShareFileCallback::from_response(
            response.result(),
            response.body().clone(),
        ))
    }
}

#[async_trait]
impl ClientMsgHandler for SteamCloud {
    async fn handle_msg(
        &self,
        _client: &SteamClient,
        _packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        // Nothing to handle; all responses come through unified messages.
        Ok(())
    }
}
