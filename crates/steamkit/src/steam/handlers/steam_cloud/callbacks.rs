//! Results of the [`SteamCloud`](crate::steam::handlers::steam_cloud::steam_cloud::SteamCloud)
//! requests. These are returned directly, not posted as callbacks.

use chrono::{DateTime, Utc};
use steamkit_protos::steam::{
    CCloudGetFileDetailsResponse, CCloudGetSingleFileInfoResponse, CCloudShareFileResponse,
};

use crate::base::generated::steam_language::EResult;
use crate::types::steam_id::SteamID;

/// Converts a Unix timestamp to a UTC date time.
fn date_time_from_unix_time(seconds: u64) -> DateTime<Utc> {
    crate::util::date_utils::date_time_from_unix_time(seconds)
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| DateTime::<Utc>::from(std::time::UNIX_EPOCH))
}

/// Details of a piece of user generated content.
#[derive(Clone, Debug)]
pub struct UGCDetailsCallback {
    result: EResult,
    app_id: u32,
    ugc_id: u64,
    creator: SteamID,
    url: String,
    file_name: String,
    file_size: u32,
    timestamp: DateTime<Utc>,
    file_sha: String,
    compressed_file_size: u32,
    rangecheck_host: String,
}

impl UGCDetailsCallback {
    /// Builds the result from a `Cloud.GetFileDetails` response.
    pub(crate) fn from_response(result: EResult, response: CCloudGetFileDetailsResponse) -> Self {
        let details = response.details.unwrap_or_default();

        Self {
            result,
            app_id: details.appid.unwrap_or_default(),
            ugc_id: details.ugcid.unwrap_or_default(),
            creator: SteamID::from_id(details.steamid_creator.unwrap_or_default()),
            url: details.url.unwrap_or_default(),
            file_name: details.filename.unwrap_or_default(),
            file_size: details.file_size.unwrap_or_default(),
            timestamp: date_time_from_unix_time(details.timestamp.unwrap_or_default()),
            file_sha: details.file_sha.unwrap_or_default(),
            compressed_file_size: details.compressed_file_size.unwrap_or_default(),
            rangecheck_host: response.rangecheck_host.unwrap_or_default(),
        }
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The app id the UGC is for.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The UGC id.
    pub fn ugc_id(&self) -> u64 {
        self.ugc_id
    }

    /// The SteamID of the creator of the UGC.
    pub fn creator(&self) -> SteamID {
        self.creator
    }

    /// The URL the content is located at.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The name of the file.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// The size of the file.
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    /// The timestamp of the file.
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    /// The SHA hash of the file.
    pub fn file_sha(&self) -> &str {
        &self.file_sha
    }

    /// The compressed size of the file.
    pub fn compressed_file_size(&self) -> u32 {
        self.compressed_file_size
    }

    /// The rangecheck host.
    pub fn rangecheck_host(&self) -> &str {
        &self.rangecheck_host
    }
}

/// Info about a single file in the user's cloud storage.
#[derive(Clone, Debug)]
pub struct SingleFileInfoCallback {
    result: EResult,
    app_id: u32,
    file_name: String,
    sha_hash: Vec<u8>,
    timestamp: DateTime<Utc>,
    file_size: u32,
    is_explicit_delete: bool,
}

impl SingleFileInfoCallback {
    /// Builds the result from a `Cloud.GetSingleFileInfo` response.
    pub(crate) fn from_response(
        result: EResult,
        response: CCloudGetSingleFileInfoResponse,
    ) -> Self {
        Self {
            result,
            app_id: response.app_id.unwrap_or_default(),
            file_name: response.file_name.unwrap_or_default(),
            sha_hash: response.sha_file.unwrap_or_default(),
            timestamp: date_time_from_unix_time(response.time_stamp.unwrap_or_default()),
            file_size: response.raw_file_size.unwrap_or_default(),
            is_explicit_delete: response.is_explicit_delete.unwrap_or_default(),
        }
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The app id the file is for.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The name of the file that was requested.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// The SHA hash of the file.
    pub fn sha_hash(&self) -> &[u8] {
        &self.sha_hash
    }

    /// The timestamp of the file.
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    /// The size of the file.
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    /// Whether the file was explicitly deleted by the user.
    pub fn is_explicit_delete(&self) -> bool {
        self.is_explicit_delete
    }
}

/// The result of sharing a cloud file.
#[derive(Clone, Debug)]
pub struct ShareFileCallback {
    result: EResult,
    ugc_id: u64,
}

impl ShareFileCallback {
    /// Builds the result from a `Cloud.ShareFile` response.
    pub(crate) fn from_response(result: EResult, response: CCloudShareFileResponse) -> Self {
        Self {
            result,
            ugc_id: response.hcontent.unwrap_or_default(),
        }
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The resulting UGC handle.
    pub fn ugc_id(&self) -> u64 {
        self.ugc_id
    }
}
