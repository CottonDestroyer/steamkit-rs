//! User stats: player counts and leaderboards.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::{
    CMsgClientLbsFindOrCreateLb, CMsgClientLbsGetLbEntries, CMsgDpGetNumberOfCurrentPlayers,
};

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{
    ELeaderboardDataRequest, ELeaderboardDisplayType, ELeaderboardSortMethod, EMsg,
};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::steam_id::SteamID;

use super::callbacks::{
    FindOrCreateLeaderboardCallback, LeaderboardEntriesCallback, NumberOfPlayersCallback,
};

/// User stats: player counts and leaderboards.
pub struct SteamUserStats {
    client: Weak<SteamClient>,
}

impl SteamUserStats {
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

    /// The current number of players of an app.
    pub async fn get_number_of_current_players(
        &self,
        app_id: u32,
    ) -> SteamResult<NumberOfPlayersCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<NumberOfPlayersCallback>(job_id);

        let mut msg = ClientMsgProtobuf::<CMsgDpGetNumberOfCurrentPlayers>::new(
            EMsg::ClientGetNumberOfCurrentPlayersDP,
        );
        msg.set_source_job_id(job_id);
        msg.body.appid = Some(app_id);

        client.send(&msg)?;

        job.wait().await
    }

    /// Looks up a leaderboard by name.
    pub async fn find_leaderboard(
        &self,
        app_id: u32,
        name: &str,
    ) -> SteamResult<FindOrCreateLeaderboardCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<FindOrCreateLeaderboardCallback>(job_id);

        let mut msg =
            ClientMsgProtobuf::<CMsgClientLbsFindOrCreateLb>::new(EMsg::ClientLBSFindOrCreateLB);
        msg.set_source_job_id(job_id);

        // routing_appid has to be set or there's no response.
        msg.header_mut().proto.routing_appid = Some(app_id);

        msg.body.app_id = Some(app_id);
        msg.body.leaderboard_name = Some(name.to_string());
        msg.body.create_if_not_found = Some(false);

        client.send(&msg)?;

        job.wait().await
    }

    /// Looks up a leaderboard by name, creating it if it doesn't exist.
    pub async fn create_leaderboard(
        &self,
        app_id: u32,
        name: &str,
        sort_method: ELeaderboardSortMethod,
        display_type: ELeaderboardDisplayType,
    ) -> SteamResult<FindOrCreateLeaderboardCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<FindOrCreateLeaderboardCallback>(job_id);

        let mut msg =
            ClientMsgProtobuf::<CMsgClientLbsFindOrCreateLb>::new(EMsg::ClientLBSFindOrCreateLB);
        msg.set_source_job_id(job_id);

        // routing_appid has to be set or there's no response.
        msg.header_mut().proto.routing_appid = Some(app_id);

        msg.body.app_id = Some(app_id);
        msg.body.leaderboard_name = Some(name.to_string());
        msg.body.leaderboard_display_type = Some(display_type.value());
        msg.body.leaderboard_sort_method = Some(sort_method.value());
        msg.body.create_if_not_found = Some(true);

        client.send(&msg)?;

        job.wait().await
    }

    /// Requests a range of rows of a leaderboard.
    pub async fn get_leaderboard_entries(
        &self,
        app_id: u32,
        id: i32,
        range_start: i32,
        range_end: i32,
        data_request: ELeaderboardDataRequest,
    ) -> SteamResult<LeaderboardEntriesCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<LeaderboardEntriesCallback>(job_id);

        let mut msg =
            ClientMsgProtobuf::<CMsgClientLbsGetLbEntries>::new(EMsg::ClientLBSGetLBEntries);
        msg.set_source_job_id(job_id);

        // routing_appid has to be set or there's no response.
        msg.header_mut().proto.routing_appid = Some(app_id);

        msg.body.app_id = Some(app_id as i32);
        msg.body.leaderboard_id = Some(id);
        msg.body.leaderboard_data_request = Some(data_request.value());
        msg.body.range_start = Some(range_start);
        msg.body.range_end = Some(range_end);

        client.send(&msg)?;

        job.wait().await
    }

    /// Requests the leaderboard rows of specific users.
    pub async fn download_leaderboard_entries_for_users(
        &self,
        app_id: u32,
        id: i32,
        users: &[SteamID],
    ) -> SteamResult<LeaderboardEntriesCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<LeaderboardEntriesCallback>(job_id);

        let mut msg =
            ClientMsgProtobuf::<CMsgClientLbsGetLbEntries>::new(EMsg::ClientLBSGetLBEntries);
        msg.set_source_job_id(job_id);

        // routing_appid has to be set or there's no response.
        msg.header_mut().proto.routing_appid = Some(app_id);

        msg.body.app_id = Some(app_id as i32);
        msg.body.leaderboard_id = Some(id);
        msg.body.leaderboard_data_request = Some(ELeaderboardDataRequest::Users.value());
        msg.body.steamids = users
            .iter()
            .map(|steam_id| steam_id.convert_to_u64())
            .collect();

        client.send(&msg)?;

        job.wait().await
    }
}

#[async_trait]
impl ClientMsgHandler for SteamUserStats {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientGetNumberOfCurrentPlayersDPResponse => {
                Some(Box::new(NumberOfPlayersCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientLBSFindOrCreateLBResponse => Some(Box::new(
                FindOrCreateLeaderboardCallback::from_packet(packet_msg)?,
            )),
            EMsg::ClientLBSGetLBEntriesResponse => Some(Box::new(
                LeaderboardEntriesCallback::from_packet(packet_msg)?,
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
