//! The content server directory, through the `ContentServerDirectory` unified
//! service: CDN servers, manifest request codes and CDN auth tokens.

use std::fmt;
use std::sync::{Arc, OnceLock, Weak};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use steamkit_protos::steam::{
    CContentServerDirectoryGetCdnAuthTokenRequest, CContentServerDirectoryGetCdnAuthTokenResponse,
    CContentServerDirectoryGetManifestRequestCodeRequest,
    CContentServerDirectoryGetManifestRequestCodeResponse,
    CContentServerDirectoryGetServersForSteamPipeRequest,
    CContentServerDirectoryGetServersForSteamPipeResponse,
};

use crate::base::generated::steam_language::EResult;
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::cdn::server::Server;
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::handlers::steam_unified_messages::steam_unified_messages::{
    SteamUnifiedMessages, UnifiedService,
};
use crate::steam::steam_client::steam_client::SteamClient;
use crate::steam::webapi::content_server_directory_service::ContentServerDirectoryService;

/// The unified service this handler uses.
const SERVICE_NAME: &str = "ContentServerDirectory";

/// Converts a Unix timestamp to a UTC date time.
fn date_time_from_unix_time(seconds: u64) -> DateTime<Utc> {
    crate::util::date_utils::date_time_from_unix_time(seconds)
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| DateTime::<Utc>::from(std::time::UNIX_EPOCH))
}

/// A CDN auth token from the content server directory.
#[derive(Clone, Debug)]
pub struct CDNAuthToken {
    /// The result of the operation.
    pub result: EResult,
    /// The CDN auth token.
    pub token: String,
    /// When the token expires.
    pub expiration: DateTime<Utc>,
}

impl CDNAuthToken {
    /// Builds the token from a `GetCDNAuthToken` response.
    fn from_response(
        result: EResult,
        response: CContentServerDirectoryGetCdnAuthTokenResponse,
    ) -> Self {
        Self {
            result,
            token: response.token.unwrap_or_default(),
            expiration: date_time_from_unix_time(u64::from(
                response.expiration_time.unwrap_or_default(),
            )),
        }
    }
}

/// Interacts with the content server directory.
pub struct SteamContent {
    client: Weak<SteamClient>,
    service: OnceLock<Arc<UnifiedService>>,
}

impl fmt::Debug for SteamContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamContent").finish_non_exhaustive()
    }
}

impl SteamContent {
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

    /// The `ContentServerDirectory` unified service.
    fn service(&self) -> SteamResult<Arc<UnifiedService>> {
        if let Some(service) = self.service.get() {
            return Ok(Arc::clone(service));
        }

        let client = self.client()?;
        let unified_messages = client.get_required_handler::<SteamUnifiedMessages>()?;
        let service = unified_messages.create_service(SERVICE_NAME)?;

        service.set_response_type::<CContentServerDirectoryGetServersForSteamPipeResponse>(
            "GetServersForSteamPipe",
        );
        service.set_response_type::<CContentServerDirectoryGetManifestRequestCodeResponse>(
            "GetManifestRequestCode",
        );
        service
            .set_response_type::<CContentServerDirectoryGetCdnAuthTokenResponse>("GetCDNAuthToken");

        let _ = self.service.set(Arc::clone(&service));

        Ok(service)
    }

    /// Loads a list of content servers from the content server directory.
    /// Same as
    /// [`ContentServerDirectoryService::load`](crate::steam::webapi::content_server_directory_service::ContentServerDirectoryService::load),
    /// but over the CM connection instead of the Web API.
    ///
    /// `cell_id` is the preferred cell id; `max_num_servers` caps the result.
    pub async fn get_servers_for_steam_pipe(
        &self,
        cell_id: Option<u32>,
        max_num_servers: Option<u32>,
    ) -> SteamResult<Vec<Server>> {
        let client = self.client()?;
        let service = self.service()?;

        let request = CContentServerDirectoryGetServersForSteamPipeRequest {
            cell_id: Some(cell_id.unwrap_or_else(|| client.cell_id().unwrap_or(0))),
            max_servers: max_num_servers,
            ..Default::default()
        };

        let response = service
            .send_message::<
                CContentServerDirectoryGetServersForSteamPipeRequest,
                CContentServerDirectoryGetServersForSteamPipeResponse,
            >("GetServersForSteamPipe", request)
            .await?;

        Ok(ContentServerDirectoryService::convert_server_list(
            response.body(),
        ))
    }

    /// Requests the manifest request code for a manifest.
    ///
    /// `branch` is the branch the manifest belongs to and `branch_password_hash`
    /// its password; a password needs a branch name. Returns 0 if no code was
    /// granted.
    pub async fn get_manifest_request_code(
        &self,
        depot_id: u32,
        app_id: u32,
        manifest_id: u64,
        branch: Option<&str>,
        branch_password_hash: Option<&str>,
    ) -> SteamResult<u64> {
        let service = self.service()?;

        // "public" is the default branch and never has a password.
        let (branch, branch_password_hash) =
            match branch.filter(|branch| branch.eq_ignore_ascii_case("public")) {
                Some(_) => (None, None),
                None => (branch, branch_password_hash),
            };

        if branch_password_hash.is_some() && branch.is_none() {
            return Err(SteamError::InvalidArgument(
                "Branch name may not be null if password is provided.".to_string(),
            ));
        }

        let request = CContentServerDirectoryGetManifestRequestCodeRequest {
            app_id: Some(app_id),
            depot_id: Some(depot_id),
            manifest_id: Some(manifest_id),
            app_branch: branch.map(str::to_string),
            branch_password_hash: branch_password_hash.map(str::to_string),
            ..Default::default()
        };

        let response = service
            .send_message::<
                CContentServerDirectoryGetManifestRequestCodeRequest,
                CContentServerDirectoryGetManifestRequestCodeResponse,
            >("GetManifestRequestCode", request)
            .await?;

        Ok(response.body().manifest_request_code.unwrap_or_default())
    }

    /// Requests a CDN auth token for the given app, depot and host name.
    pub async fn get_cdn_auth_token(
        &self,
        app: u32,
        depot: u32,
        host_name: &str,
    ) -> SteamResult<CDNAuthToken> {
        let service = self.service()?;

        let request = CContentServerDirectoryGetCdnAuthTokenRequest {
            app_id: Some(app),
            depot_id: Some(depot),
            host_name: Some(host_name.to_string()),
            ..Default::default()
        };

        let response = service
            .send_message::<
                CContentServerDirectoryGetCdnAuthTokenRequest,
                CContentServerDirectoryGetCdnAuthTokenResponse,
            >("GetCDNAuthToken", request)
            .await?;

        Ok(CDNAuthToken::from_response(
            response.result(),
            response.body().clone(),
        ))
    }
}

#[async_trait]
impl ClientMsgHandler for SteamContent {
    async fn handle_msg(
        &self,
        _client: &SteamClient,
        _packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        // Nothing to handle; all responses come through unified messages.
        Ok(())
    }
}
