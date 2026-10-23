//! Callbacks posted by [`SteamUserStats`](super::steam_user_stats::SteamUserStats).

use std::any::Any;

use steamkit_protos::steam::{
    c_msg_client_lbs_get_lb_entries_response, CMsgClientLbsFindOrCreateLbResponse,
    CMsgClientLbsGetLbEntriesResponse, CMsgDpGetNumberOfCurrentPlayersResponse,
};

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::generated::steam_language::{
    ELeaderboardDisplayType, ELeaderboardSortMethod, EResult,
};
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::global_id::UGCHandle;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

/// The response to
/// [`SteamUserStats::get_number_of_current_players`](crate::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats::get_number_of_current_players).
#[derive(Clone, Debug)]
pub struct NumberOfPlayersCallback {
    job_id: JobID,
    result: EResult,
    num_players: u32,
}

impl NumberOfPlayersCallback {
    /// Builds the callback from a `ClientGetNumberOfCurrentPlayersDP` response.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let msg =
            ClientMsgProtobuf::<CMsgDpGetNumberOfCurrentPlayersResponse>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(msg.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            num_players: msg.body.player_count.unwrap_or_default() as u32,
        })
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The current number of players according to Steam.
    pub fn num_players(&self) -> u32 {
        self.num_players
    }
}

impl CallbackMsg for NumberOfPlayersCallback {
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

/// The response to
/// [`SteamUserStats::find_leaderboard`](crate::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats::find_leaderboard)
/// and
/// [`SteamUserStats::create_leaderboard`](crate::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats::create_leaderboard).
#[derive(Clone, Debug)]
pub struct FindOrCreateLeaderboardCallback {
    job_id: JobID,
    result: EResult,
    id: i32,
    entry_count: i32,
    sort_method: ELeaderboardSortMethod,
    display_type: ELeaderboardDisplayType,
}

impl FindOrCreateLeaderboardCallback {
    /// Builds the callback from a `ClientLBSFindOrCreateLB` response.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let msg =
            ClientMsgProtobuf::<CMsgClientLbsFindOrCreateLbResponse>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(msg.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            id: msg.body.leaderboard_id.unwrap_or_default(),
            entry_count: msg.body.leaderboard_entry_count.unwrap_or_default(),
            sort_method: ELeaderboardSortMethod::from_value(
                msg.body.leaderboard_sort_method.unwrap_or_default(),
            )
            .unwrap_or(ELeaderboardSortMethod::None),
            display_type: ELeaderboardDisplayType::from_value(
                msg.body.leaderboard_display_type.unwrap_or_default(),
            )
            .unwrap_or(ELeaderboardDisplayType::None),
        })
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The leaderboard ID.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// How many entries there are for the requested leaderboard.
    pub fn entry_count(&self) -> i32 {
        self.entry_count
    }

    /// The sort method of the leaderboard.
    pub fn sort_method(&self) -> ELeaderboardSortMethod {
        self.sort_method
    }

    /// The display type of the leaderboard.
    pub fn display_type(&self) -> ELeaderboardDisplayType {
        self.display_type
    }
}

impl CallbackMsg for FindOrCreateLeaderboardCallback {
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

/// The response to
/// [`SteamUserStats::get_leaderboard_entries`](crate::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats::get_leaderboard_entries)
/// and
/// [`SteamUserStats::download_leaderboard_entries_for_users`](crate::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats::download_leaderboard_entries_for_users).
#[derive(Clone, Debug)]
pub struct LeaderboardEntriesCallback {
    job_id: JobID,
    result: EResult,
    entry_count: i32,
    entries: Vec<LeaderboardEntry>,
}

/// A single leaderboard entry.
#[derive(Clone, Debug)]
pub struct LeaderboardEntry {
    steam_id: SteamID,
    global_rank: i32,
    score: i32,
    ugc_id: UGCHandle,
    details: Vec<i32>,
}

impl LeaderboardEntry {
    /// Builds the entry from its protobuf form.
    fn from_entry(entry: &c_msg_client_lbs_get_lb_entries_response::Entry) -> Self {
        let details = entry
            .details
            .as_deref()
            .map(|bytes| {
                bytes
                    .chunks_exact(4)
                    .map(|chunk| i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            steam_id: SteamID::from_id(entry.steam_id_user.unwrap_or_default()),
            global_rank: entry.global_rank.unwrap_or_default(),
            score: entry.score.unwrap_or_default(),
            ugc_id: UGCHandle::from_value(entry.ugc_id.unwrap_or_default()),
            details,
        }
    }

    /// The [`SteamID`] for this entry.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The global rank for this entry.
    pub fn global_rank(&self) -> i32 {
        self.global_rank
    }

    /// The score for this entry.
    pub fn score(&self) -> i32 {
        self.score
    }

    /// The [`UGCHandle`] attached to this entry.
    pub fn ugc_id(&self) -> UGCHandle {
        self.ugc_id
    }

    /// Extra game-defined info about how the user got the score.
    pub fn details(&self) -> &[i32] {
        &self.details
    }
}

impl LeaderboardEntriesCallback {
    /// Builds the callback from a `ClientLBSGetLBEntries` response.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let msg = ClientMsgProtobuf::<CMsgClientLbsGetLbEntriesResponse>::from_packet(packet_msg)?;

        let entries = msg
            .body
            .entries
            .iter()
            .map(LeaderboardEntry::from_entry)
            .collect();

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(msg.body.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            entry_count: msg.body.leaderboard_entry_count.unwrap_or_default(),
            entries,
        })
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// How many entries the leaderboard has.
    pub fn entry_count(&self) -> i32 {
        self.entry_count
    }

    /// The entries in this response.
    pub fn entries(&self) -> &[LeaderboardEntry] {
        &self.entries
    }
}

impl CallbackMsg for LeaderboardEntriesCallback {
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
