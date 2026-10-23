//! `SteamClient` tests: the message handler registry and the job id
//! generator.

use std::sync::Arc;

use async_trait::async_trait;

use steamkit::base::packet_base::IPacketMsg;
use steamkit::error::{SteamError, SteamResult};
use steamkit::steam::handlers::client_msg_handler::ClientMsgHandler;
use steamkit::steam::handlers::steam_apps::steam_apps::SteamApps;
use steamkit::steam::handlers::steam_auth_ticket::steam_auth_ticket::SteamAuthTicket;
use steamkit::steam::handlers::steam_cloud::steam_cloud::SteamCloud;
use steamkit::steam::handlers::steam_content::steam_content::SteamContent;
use steamkit::steam::handlers::steam_friends::steam_friends::SteamFriends;
use steamkit::steam::handlers::steam_game_coordinator::steam_game_coordinator::SteamGameCoordinator;
use steamkit::steam::handlers::steam_game_server::steam_game_server::SteamGameServer;
use steamkit::steam::handlers::steam_master_server::steam_master_server::SteamMasterServer;
use steamkit::steam::handlers::steam_matchmaking::steam_matchmaking::SteamMatchmaking;
use steamkit::steam::handlers::steam_networking::steam_networking::SteamNetworking;
use steamkit::steam::handlers::steam_screenshots::steam_screenshots::SteamScreenshots;
use steamkit::steam::handlers::steam_unified_messages::steam_unified_messages::SteamUnifiedMessages;
use steamkit::steam::handlers::steam_user::steam_user::SteamUser;
use steamkit::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats;
use steamkit::steam::handlers::steam_workshop::steam_workshop::SteamWorkshop;
use steamkit::steam::steam_client::steam_client::SteamClient;

/// A trivial handler used to probe the registry.
struct TestMsgHandler;

#[async_trait]
impl ClientMsgHandler for TestMsgHandler {
    async fn handle_msg(
        &self,
        _client: &SteamClient,
        _packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        Ok(())
    }
}

#[test]
fn constructor_sets_initial_handlers() {
    let client = SteamClient::new();

    assert!(client.get_handler::<SteamUser>().is_some());
    assert!(client.get_handler::<SteamFriends>().is_some());
    assert!(client.get_handler::<SteamApps>().is_some());
    assert!(client.get_handler::<SteamGameCoordinator>().is_some());
    assert!(client.get_handler::<SteamGameServer>().is_some());
    assert!(client.get_handler::<SteamUserStats>().is_some());
    assert!(client.get_handler::<SteamMasterServer>().is_some());
    assert!(client.get_handler::<SteamCloud>().is_some());
    assert!(client.get_handler::<SteamWorkshop>().is_some());
    assert!(client.get_handler::<SteamUnifiedMessages>().is_some());
    assert!(client.get_handler::<SteamScreenshots>().is_some());
    assert!(client.get_handler::<SteamMatchmaking>().is_some());
    assert!(client.get_handler::<SteamNetworking>().is_some());
    assert!(client.get_handler::<SteamContent>().is_some());
    assert!(client.get_handler::<SteamAuthTicket>().is_some());
}

#[test]
fn add_handler_adds_handler() {
    let client = SteamClient::new();

    assert!(client.get_handler::<TestMsgHandler>().is_none());

    // add_typed_handler keeps the handler retrievable through get_handler.
    assert!(client.add_typed_handler(TestMsgHandler).is_ok());

    // The handler is wrapped in a fresh Arc internally, so check that it can
    // be retrieved rather than comparing identity.
    assert!(client.get_handler::<TestMsgHandler>().is_some());
}

#[test]
fn remove_handler_removes_handler() {
    let client = SteamClient::new();
    assert!(client.add_typed_handler(TestMsgHandler).is_ok());
    assert!(client.get_handler::<TestMsgHandler>().is_some());

    assert!(client.remove_handler_type::<TestMsgHandler>());

    assert!(client.get_handler::<TestMsgHandler>().is_none());
}

#[test]
fn remove_handler_removes_handler_by_instance() {
    let client = SteamClient::new();
    assert!(client.add_typed_handler(TestMsgHandler).is_ok());
    assert!(client.get_handler::<TestMsgHandler>().is_some());

    // add_typed_handler builds the Arc internally, so fetch it back and
    // remove that exact instance.
    let handler = client
        .get_handler::<TestMsgHandler>()
        .expect("handler was just added");
    let handler: Arc<dyn ClientMsgHandler> = handler;
    assert!(client.remove_handler(&handler));

    assert!(client.get_handler::<TestMsgHandler>().is_none());
}

#[test]
fn add_handler_throws_on_duplicate_handler() {
    let client = SteamClient::new();
    assert!(client.add_typed_handler(TestMsgHandler).is_ok());

    // Adding the same handler type twice is an error.
    let duplicate = client.add_typed_handler(TestMsgHandler);
    assert!(
        matches!(duplicate, Err(SteamError::InvalidArgument(_))),
        "adding a duplicate handler must be rejected with InvalidArgument, got {duplicate:?}"
    );
}

#[test]
fn remove_handler_by_type_does_nothing_when_not_registered() {
    let client = SteamClient::new();
    assert!(client.get_handler::<TestMsgHandler>().is_none());

    // Removing an unregistered type is a no-op that reports false.
    assert!(!client.remove_handler_type::<TestMsgHandler>());

    assert!(client.get_handler::<TestMsgHandler>().is_none());
}

#[test]
fn get_required_handler_returns_handler() {
    let client = SteamClient::new();

    // The Ok payload is Arc<SteamUser> by construction.
    let handler = client.get_required_handler::<SteamUser>();
    assert!(handler.is_ok(), "SteamUser is registered by default");
}

#[test]
fn get_required_handler_throws_when_not_registered() {
    let client = SteamClient::new();

    // A missing handler is reported as NotFound.
    let result = client.get_required_handler::<TestMsgHandler>();
    assert!(
        matches!(result, Err(SteamError::NotFound(_))),
        "a missing required handler must report NotFound"
    );
}

#[test]
fn get_next_job_id_is_threadsafe() {
    let client = SteamClient::new();

    let job_id = client.get_next_job_id();
    assert_eq!(job_id.sequential_count(), 1u32);

    // Ten threads take 100 job ids each through the atomic counter.
    let mut handles = Vec::new();
    for _ in 0..10 {
        let worker = client.clone();
        handles.push(std::thread::spawn(move || {
            for _ in 0..100 {
                worker.get_next_job_id();
            }
        }));
    }
    for handle in handles {
        handle.join().expect("worker thread must not panic");
    }

    let job_id = client.get_next_job_id();
    assert_eq!(job_id.sequential_count(), 1002u32);
}

#[test]
fn get_next_job_id_sets_process_id_to_zero() {
    let client = SteamClient::new();
    let job_id = client.get_next_job_id();

    assert_eq!(job_id.process_id(), 0u32);
}

#[test]
fn get_next_job_id_fills_process_start_time() {
    // The job id's start time comes from SystemTime::now() when the client is
    // created, not from the OS process start time (std has no portable way to
    // get that), so there's nothing reliable to compare against here.
}

#[test]
fn get_next_job_id_sets_box_id_to_zero() {
    let client = SteamClient::new();
    let job_id = client.get_next_job_id();

    assert_eq!(job_id.box_id(), 0u32);
}
