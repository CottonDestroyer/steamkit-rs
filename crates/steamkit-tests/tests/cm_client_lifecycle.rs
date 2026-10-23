//! Tests for the connection lifecycle of `steam/cm_client.rs`: connecting,
//! dispatching, logging on, moving to another CM server, and disconnecting.
//!
//! The CM servers are played by [`mock::MockConnection`]s handed out by the
//! client's connection factory, and what the client reports is recorded
//! through a [`ClientMsgDispatcher`].

#[allow(dead_code)]
mod mock;

use std::collections::VecDeque;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mock::{eventually, raw_msg_type, settle, ConnectBehaviour, MockConnection};
use steamkit::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use steamkit::base::generated::steam_language::{EMsg, EResult};
use steamkit::base::packet_base::IPacketMsg;
use steamkit::networking::steam3::i_connection::Connection;
use steamkit::steam::cm_client::{
    CMClient, ClientMsgDispatcher, CURRENT_PROTOCOL, MAX_CONSECUTIVE_SERVER_SWITCHES,
};
use steamkit::steam::discovery::server_record::ServerRecord;
use steamkit::steam::steam_client::configuration::i_steam_configuration_builder::ISteamConfigurationBuilder;
use steamkit::steam::steam_client::configuration::steam_configuration::SteamConfiguration;
use steamkit::types::steam_id::SteamID;
use steamkit_protos::steam::{
    CMsgClientHeartBeat, CMsgClientHello, CMsgClientLoggedOff, CMsgClientLogon,
    CMsgClientLogonResponse, CMsgClientSessionToken, CMsgMulti,
};

/// The number of servers a single move tries (`SWITCH_ATTEMPTS`).
const SWITCH_ATTEMPTS: usize = 3;

const STEAM_ID: u64 = 76_561_197_960_287_930;
const SESSION_ID: i32 = 1234;

/// What the client reported through its dispatcher.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reported {
    Connected,
    Message { msg: u32, job: u64 },
    Disconnected { user_initiated: bool },
}

#[derive(Default)]
struct Recorder {
    reported: Mutex<Vec<Reported>>,
    /// Makes the first message take a while, so that a backlog builds up.
    slow_first_message: AtomicBool,
}

impl Recorder {
    fn reported(&self) -> Vec<Reported> {
        self.reported.lock().unwrap().clone()
    }

    /// The dispatched messages, without the `Multi` wrappers (the client hands
    /// a `Multi` to the handlers too, after the messages inside it).
    fn messages(&self) -> Vec<(u32, u64)> {
        self.reported()
            .into_iter()
            .filter_map(|reported| match reported {
                Reported::Message { msg, job } if msg != EMsg::Multi as u32 => Some((msg, job)),
                _ => None,
            })
            .collect()
    }

    fn multi_count(&self) -> usize {
        self.reported()
            .iter()
            .filter(|reported| matches!(reported, Reported::Message { msg, .. } if *msg == EMsg::Multi as u32))
            .count()
    }

    fn disconnections(&self) -> Vec<bool> {
        self.reported()
            .into_iter()
            .filter_map(|reported| match reported {
                Reported::Disconnected { user_initiated } => Some(user_initiated),
                _ => None,
            })
            .collect()
    }

    fn connected_count(&self) -> usize {
        self.reported().iter().filter(|reported| **reported == Reported::Connected).count()
    }
}

#[async_trait]
impl ClientMsgDispatcher for Recorder {
    async fn on_client_msg_received(&self, _client: &CMClient, packet: &dyn IPacketMsg) -> bool {
        if self.slow_first_message.swap(false, Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.reported.lock().unwrap().push(Reported::Message {
            msg: packet.msg_type() as u32,
            job: packet.target_job_id().value(),
        });
        true
    }

    async fn on_client_connected(&self, _client: &CMClient) {
        self.reported.lock().unwrap().push(Reported::Connected);
    }

    async fn on_client_disconnected(&self, _client: &CMClient, user_initiated: bool) {
        self.reported.lock().unwrap().push(Reported::Disconnected { user_initiated });
    }
}

struct Fixture {
    client: Arc<CMClient>,
    recorder: Arc<Recorder>,
    connections: Arc<Mutex<Vec<Arc<MockConnection>>>>,
    /// The behaviour of the next connections the factory creates; once it is
    /// empty, connections connect normally.
    behaviours: Arc<Mutex<VecDeque<ConnectBehaviour>>>,
}

impl Fixture {
    async fn new() -> Fixture {
        Self::with_behaviours(Vec::new()).await
    }

    async fn with_behaviours(behaviours: Vec<ConnectBehaviour>) -> Fixture {
        let configuration = Arc::new(SteamConfiguration::create(|builder| {
            builder.with_directory_fetch(false);
        }));
        let servers = (1..=3)
            .map(|index| ServerRecord::create_socket_server(SocketAddr::from(([10, 0, 0, index], 27017))))
            .collect();
        configuration.server_list().replace_list(servers, true, None).await.unwrap();

        let recorder = Arc::new(Recorder::default());
        let client = Arc::new(CMClient::new(configuration, "test", recorder.clone()));

        let connections: Arc<Mutex<Vec<Arc<MockConnection>>>> = Arc::default();
        let behaviours = Arc::new(Mutex::new(VecDeque::from(behaviours)));

        let factory_connections = connections.clone();
        let factory_behaviours = behaviours.clone();
        client.set_connection_factory(Arc::new(move |_client, _record, _protocol| {
            let behaviour = factory_behaviours
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ConnectBehaviour::Connected);
            let connection = MockConnection::new(behaviour);
            factory_connections.lock().unwrap().push(connection.clone());
            Ok(connection as Arc<dyn Connection>)
        }));

        Fixture { client, recorder, connections, behaviours }
    }

    fn connection(&self, index: usize) -> Arc<MockConnection> {
        self.connections.lock().unwrap()[index].clone()
    }

    fn connection_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }

    /// Connects and waits until the client has reported the connection.
    async fn connect(&self) -> Arc<MockConnection> {
        let before = self.recorder.connected_count();
        self.client.connect(None).await.unwrap();
        eventually("the client reported the connection", || {
            self.recorder.connected_count() == before + 1
        })
        .await;
        self.connection(self.connection_count() - 1)
    }

    /// Connects, logs on and waits until the logon has been accepted.
    async fn connect_and_log_on(&self) -> Arc<MockConnection> {
        let connection = self.connect().await;
        self.client.send(&logon()).unwrap();
        eventually("the logon was sent", || sent_count(&connection, EMsg::ClientLogon) == 1).await;
        connection.receive(logon_response(EResult::OK));
        eventually("the logon was accepted", || self.client.steam_id().is_some()).await;
        connection
    }
}

fn logon() -> ClientMsgProtobuf<CMsgClientLogon> {
    let mut logon = ClientMsgProtobuf::<CMsgClientLogon>::new(EMsg::ClientLogon);
    logon.body_mut().protocol_version = Some(CURRENT_PROTOCOL);
    logon.body_mut().cell_id = Some(7);
    logon
}

/// A server message with an otherwise empty body; `job` travels in the header.
fn message(msg: EMsg, job: u64) -> Vec<u8> {
    let mut message = ClientMsgProtobuf::<CMsgClientHeartBeat>::new(msg);
    message.proto_header_mut().jobid_target = Some(job);
    message.serialize()
}

fn logon_response(result: EResult) -> Vec<u8> {
    let mut response = ClientMsgProtobuf::<CMsgClientLogonResponse>::new(EMsg::ClientLogOnResponse);
    response.body_mut().eresult = Some(result as i32);
    response.body_mut().cell_id = Some(42);
    response.body_mut().legacy_out_of_game_heartbeat_seconds = Some(1);
    response.proto_header_mut().steamid = Some(STEAM_ID);
    response.proto_header_mut().client_sessionid = Some(SESSION_ID);
    response.serialize()
}

fn logged_off(result: EResult) -> Vec<u8> {
    let mut message = ClientMsgProtobuf::<CMsgClientLoggedOff>::new(EMsg::ClientLoggedOff);
    message.body_mut().eresult = Some(result as i32);
    message.serialize()
}

fn multi(messages: &[Vec<u8>], compressed: bool) -> Vec<u8> {
    let mut body = Vec::new();
    for message in messages {
        body.extend_from_slice(&(message.len() as i32).to_le_bytes());
        body.extend_from_slice(message);
    }

    let mut multi = ClientMsgProtobuf::<CMsgMulti>::new(EMsg::Multi);
    if compressed {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&body).unwrap();
        multi.body_mut().size_unzipped = Some(body.len() as u32);
        multi.body_mut().message_body = Some(encoder.finish().unwrap());
    } else {
        multi.body_mut().message_body = Some(body);
    }
    multi.serialize()
}

fn sent_count(connection: &MockConnection, msg: EMsg) -> usize {
    connection.sent_msg_types().iter().filter(|sent| **sent == msg as u32).count()
}

/// What a connection sent, without the heartbeats, whose timing is not under
/// the test's control.
fn sent_without_heartbeats(connection: &MockConnection) -> Vec<u32> {
    connection
        .sent_msg_types()
        .into_iter()
        .filter(|msg| *msg != EMsg::ClientHeartBeat as u32)
        .collect()
}

fn sent_of_type(connection: &MockConnection, msg: EMsg) -> Vec<Vec<u8>> {
    connection
        .sent()
        .into_iter()
        .filter(|data| raw_msg_type(data) == msg as u32)
        .collect()
}

// ---------------------------------------------------------------------------
// Connecting and dispatching
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connecting_sends_the_hello_and_reports_the_connection() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    assert!(fixture.client.is_connected());
    assert!(fixture.client.current_end_point().is_some());

    eventually("the hello was sent", || sent_count(&connection, EMsg::ClientHello) == 1).await;
    let hello = sent_of_type(&connection, EMsg::ClientHello).remove(0);
    let packet = fixture.client.get_packet_msg(hello).expect("the hello parses");
    let hello = ClientMsgProtobuf::<CMsgClientHello>::from_packet(packet.as_ref()).unwrap();
    assert_eq!(Some(CURRENT_PROTOCOL), hello.body().protocol_version);
}

#[tokio::test]
async fn a_burst_of_messages_is_dispatched_completely_and_in_order() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    const COUNT: u64 = 2_000;
    fixture.recorder.slow_first_message.store(true, Ordering::SeqCst);
    for job in 0..COUNT {
        connection.receive(message(EMsg::ClientPersonaState, job));
    }

    eventually("every message was dispatched", || fixture.recorder.messages().len() == COUNT as usize)
        .await;
    let jobs: Vec<u64> = fixture.recorder.messages().into_iter().map(|(_, job)| job).collect();
    assert_eq!((0..COUNT).collect::<Vec<_>>(), jobs);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_right_behind_connected_is_dispatched_after_the_connection() {
    for _ in 0..20 {
        let fixture = Fixture::with_behaviours(vec![ConnectBehaviour::Silent]).await;
        fixture.client.connect(None).await.unwrap();
        let connection = fixture.connection(0);

        connection.raise_connected();
        connection.receive(message(EMsg::ClientPersonaState, 1));

        eventually("the message was dispatched", || fixture.recorder.messages().len() == 1).await;
        assert_eq!(Reported::Connected, fixture.recorder.reported()[0]);
    }
}

#[tokio::test]
async fn a_packet_that_does_not_parse_disconnects() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    connection.receive(vec![1, 2]);

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
    assert_eq!(vec![false], connection.disconnect_calls());
    assert!(!fixture.client.is_connected());
}

#[tokio::test]
async fn server_unavailable_disconnects() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    connection.receive(message(EMsg::ClientServerUnavailable, 0));

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
}

#[tokio::test]
async fn the_session_token_is_recorded() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    let mut token = ClientMsgProtobuf::<CMsgClientSessionToken>::new(EMsg::ClientSessionToken);
    token.body_mut().token = Some(0xDEAD_BEEF);
    connection.receive(token.serialize());

    eventually("the token was recorded", || fixture.client.session_token() == 0xDEAD_BEEF).await;
}

#[tokio::test]
async fn sending_without_a_connection_fails() {
    let fixture = Fixture::new().await;
    assert!(fixture.client.send(&logon()).is_err());
}

// ---------------------------------------------------------------------------
// Multi messages
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_multi_message_is_unpacked_in_order() {
    for compressed in [false, true] {
        let fixture = Fixture::new().await;
        let connection = fixture.connect().await;

        let inner = vec![
            message(EMsg::ClientPersonaState, 1),
            message(EMsg::ClientFriendsList, 2),
            message(EMsg::ClientPersonaState, 3),
        ];
        connection.receive(multi(&inner, compressed));

        eventually("the inner messages were dispatched", || fixture.recorder.messages().len() == 3).await;
        eventually("the multi was handed on after them", || fixture.recorder.multi_count() == 1).await;
        assert_eq!(
            Some(&Reported::Message { msg: EMsg::Multi as u32, job: u64::MAX }),
            fixture.recorder.reported().last()
        );
        assert_eq!(
            vec![
                (EMsg::ClientPersonaState as u32, 1),
                (EMsg::ClientFriendsList as u32, 2),
                (EMsg::ClientPersonaState as u32, 3),
            ],
            fixture.recorder.messages(),
            "compressed: {compressed}"
        );
    }
}

#[tokio::test]
async fn a_multi_message_inside_a_multi_message_is_unpacked() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    let nested = multi(&[message(EMsg::ClientPersonaState, 2)], true);
    connection.receive(multi(&[message(EMsg::ClientPersonaState, 1), nested], false));

    eventually("the inner messages were dispatched", || fixture.recorder.messages().len() == 2).await;
    assert_eq!(vec![1, 2], fixture.recorder.messages().iter().map(|(_, job)| *job).collect::<Vec<_>>());
}

/// A message inside a multi that doesn't parse disconnects the client,
/// instead of leaving the connection open and unread.
#[tokio::test]
async fn a_multi_message_with_a_malformed_inner_message_disconnects() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    connection.receive(multi(&[message(EMsg::ClientPersonaState, 1), vec![9, 9]], false));

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
    assert_eq!(vec![(EMsg::ClientPersonaState as u32, 1)], fixture.recorder.messages());
    assert_eq!(vec![false], connection.disconnect_calls());
}

#[tokio::test]
async fn a_multi_message_that_does_not_decompress_is_dropped() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    let mut broken = ClientMsgProtobuf::<CMsgMulti>::new(EMsg::Multi);
    broken.body_mut().size_unzipped = Some(100);
    broken.body_mut().message_body = Some(vec![0x1F, 0x8B, 0, 0, 1, 2, 3]);
    connection.receive(broken.serialize());
    connection.receive(message(EMsg::ClientPersonaState, 5));

    eventually("the next message was dispatched", || fixture.recorder.messages().len() == 1).await;
    assert!(fixture.recorder.disconnections().is_empty());
}

// ---------------------------------------------------------------------------
// Logging on
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_accepted_logon_assigns_the_identity_that_outgoing_messages_carry() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect_and_log_on().await;

    assert_eq!(Some(SteamID::from_id(STEAM_ID)), fixture.client.steam_id());
    assert_eq!(Some(SESSION_ID), fixture.client.session_id());
    assert_eq!(Some(42), fixture.client.cell_id());

    fixture.client.send(&ClientMsgProtobuf::<CMsgClientHeartBeat>::new(EMsg::ClientPersonaState)).unwrap();
    eventually("the message was sent", || sent_count(&connection, EMsg::ClientPersonaState) == 1).await;

    let sent = sent_of_type(&connection, EMsg::ClientPersonaState).remove(0);
    let packet = fixture.client.get_packet_msg(sent).unwrap();
    let sent = ClientMsgProtobuf::<CMsgClientHeartBeat>::from_packet(packet.as_ref()).unwrap();
    assert_eq!(Some(STEAM_ID), sent.proto_header().steamid);
    assert_eq!(Some(SESSION_ID), sent.proto_header().client_sessionid);
}

#[tokio::test]
async fn an_accepted_logon_starts_the_heartbeat() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect_and_log_on().await;

    eventually("a heartbeat was sent", || sent_count(&connection, EMsg::ClientHeartBeat) >= 1).await;
}

#[tokio::test]
async fn being_logged_off_clears_the_identity() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect_and_log_on().await;

    connection.receive(logged_off(EResult::LoggedInElsewhere));

    eventually("the identity was cleared", || fixture.client.steam_id().is_none()).await;
    assert_eq!(None, fixture.client.session_id());
    assert_eq!(None, fixture.client.cell_id());
}

// ---------------------------------------------------------------------------
// The server closing the connection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_server_disconnect_before_any_logon_is_reported_once() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;

    connection.server_disconnect();

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
    settle().await;
    assert_eq!(vec![false], fixture.recorder.disconnections());
    assert_eq!(1, fixture.connection_count(), "nothing to resume, so no other server is tried");
    assert!(!fixture.client.is_connected());
}

#[tokio::test]
async fn a_logged_on_session_moves_to_another_server_when_the_server_closes_it() {
    let fixture = Fixture::new().await;
    let first = fixture.connect_and_log_on().await;

    first.server_disconnect();

    eventually("a new connection was made", || fixture.connection_count() == 2).await;
    let second = fixture.connection(1);
    eventually("the logon was repeated", || sent_count(&second, EMsg::ClientLogon) == 1).await;

    assert_eq!(
        vec![EMsg::ClientHello as u32, EMsg::ClientLogon as u32],
        sent_without_heartbeats(&second),
        "the hello comes first, then the logon"
    );
    assert_eq!(
        sent_of_type(&first, EMsg::ClientLogon),
        sent_of_type(&second, EMsg::ClientLogon),
        "the very same logon is repeated"
    );
    assert!(fixture.recorder.disconnections().is_empty(), "the session continues");
    assert_eq!(2, fixture.recorder.connected_count());
}

#[tokio::test]
async fn a_refused_logon_is_not_repeated() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;
    fixture.client.send(&logon()).unwrap();
    connection.receive(logon_response(EResult::InvalidPassword));

    connection.server_disconnect();

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
    settle().await;
    assert_eq!(1, fixture.connection_count(), "a wrong password must not be retried elsewhere");
}

#[tokio::test]
async fn a_logged_off_session_is_not_logged_back_on() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect_and_log_on().await;

    connection.receive(logged_off(EResult::LoggedInElsewhere));
    eventually("the log off was handled", || fixture.client.steam_id().is_none()).await;
    connection.server_disconnect();

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
    settle().await;
    assert_eq!(1, fixture.connection_count());
}

#[tokio::test]
async fn an_expected_disconnection_is_reported_as_user_initiated_and_not_resumed() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect_and_log_on().await;

    // What `SteamUser::log_off` does before it sends the log off.
    fixture.client.set_expect_disconnection(true);
    connection.server_disconnect();

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![true])
        .await;
    settle().await;
    assert_eq!(1, fixture.connection_count());
}

#[tokio::test]
async fn a_failed_move_to_another_server_is_reported_once() {
    let fixture = Fixture::with_behaviours(vec![ConnectBehaviour::Connected]).await;
    let first = fixture.connect_and_log_on().await;
    fixture
        .behaviours
        .lock()
        .unwrap()
        .extend(std::iter::repeat(ConnectBehaviour::Fail).take(SWITCH_ATTEMPTS));

    first.server_disconnect();

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![false])
        .await;
    settle().await;
    assert_eq!(vec![false], fixture.recorder.disconnections());
    assert_eq!(1 + SWITCH_ATTEMPTS, fixture.connection_count());
    assert!(!fixture.client.is_connected());
    assert_eq!(None, fixture.client.steam_id());
}

#[tokio::test]
async fn moving_to_another_server_gives_up_after_too_many_moves_without_a_logon() {
    let fixture = Fixture::new().await;
    let first = fixture.connect_and_log_on().await;

    // Every server closes the connection as soon as the logon is repeated.
    first.server_disconnect();
    for index in 1..=MAX_CONSECUTIVE_SERVER_SWITCHES as usize {
        eventually("a new connection was made", || fixture.connection_count() == index + 1).await;
        let connection = fixture.connection(index);
        eventually("the logon was repeated", || sent_count(&connection, EMsg::ClientLogon) == 1).await;
        connection.server_disconnect();
    }

    eventually("the client gave up", || fixture.recorder.disconnections() == vec![false]).await;
    settle().await;
    assert_eq!(1 + MAX_CONSECUTIVE_SERVER_SWITCHES as usize, fixture.connection_count());
}

#[tokio::test]
async fn a_successful_logon_resets_the_move_limit() {
    let fixture = Fixture::new().await;
    let mut connection = fixture.connect_and_log_on().await;

    // Twice the limit, with every new server accepting the logon.
    for index in 1..=2 * MAX_CONSECUTIVE_SERVER_SWITCHES as usize {
        connection.server_disconnect();
        eventually("a new connection was made", || fixture.connection_count() == index + 1).await;
        connection = fixture.connection(index);
        eventually("the logon was repeated", || sent_count(&connection, EMsg::ClientLogon) == 1).await;
        connection.receive(logon_response(EResult::OK));
        eventually("the logon was accepted", || fixture.client.session_id().is_some()).await;
    }

    assert!(fixture.recorder.disconnections().is_empty());
}

// ---------------------------------------------------------------------------
// The server asking for another server
// ---------------------------------------------------------------------------

#[tokio::test]
async fn try_another_cm_moves_to_another_server_and_repeats_the_logon() {
    for result in [EResult::TryAnotherCM, EResult::ServiceUnavailable] {
        let fixture = Fixture::new().await;
        let first = fixture.connect().await;
        fixture.client.send(&logon()).unwrap();
        eventually("the logon was sent", || sent_count(&first, EMsg::ClientLogon) == 1).await;

        first.receive(logon_response(result));

        eventually("a new connection was made", || fixture.connection_count() == 2).await;
        let second = fixture.connection(1);
        eventually("the logon was repeated", || sent_count(&second, EMsg::ClientLogon) == 1).await;
        eventually("the old connection was closed", || first.disconnect_calls() == vec![false]).await;
        assert!(fixture.recorder.disconnections().is_empty(), "{result:?}");
    }
}

// ---------------------------------------------------------------------------
// Disconnecting and reconnecting
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_user_disconnect_is_reported_once_and_ends_the_session() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect_and_log_on().await;
    let dispatched = fixture.recorder.messages().len();

    fixture.client.disconnect().await;

    assert_eq!(vec![true], fixture.recorder.disconnections());
    assert_eq!(vec![true], connection.disconnect_calls());
    assert!(!fixture.client.is_connected());
    assert_eq!(None, fixture.client.steam_id());

    // The connection is no longer listened to.
    connection.receive(message(EMsg::ClientPersonaState, 1));
    connection.server_disconnect();
    settle().await;
    assert_eq!(vec![true], fixture.recorder.disconnections());
    assert_eq!(dispatched, fixture.recorder.messages().len());
    assert_eq!(1, fixture.connection_count());

    // And disconnecting again says nothing more.
    fixture.client.disconnect().await;
    assert_eq!(vec![true], fixture.recorder.disconnections());
}

#[tokio::test]
async fn a_failed_connection_attempt_is_reported_once() {
    for behaviour in [ConnectBehaviour::Fail, ConnectBehaviour::FailWithDisconnected] {
        let fixture = Fixture::with_behaviours(vec![behaviour]).await;

        assert!(fixture.client.connect(None).await.is_err());

        settle().await;
        assert_eq!(vec![false], fixture.recorder.disconnections(), "{behaviour:?}");
        assert_eq!(1, fixture.connection_count(), "{behaviour:?}: no other server is tried");
        assert!(!fixture.client.is_connected());

        // A later disconnect has nothing to report.
        fixture.client.disconnect().await;
        assert_eq!(vec![false], fixture.recorder.disconnections(), "{behaviour:?}");
    }
}

#[tokio::test]
async fn connecting_again_ends_the_previous_session_once_and_starts_afresh() {
    let fixture = Fixture::new().await;
    let first = fixture.connect_and_log_on().await;

    let second = fixture.connect().await;

    assert_eq!(vec![true], fixture.recorder.disconnections());
    assert_eq!(vec![true], first.disconnect_calls());
    assert!(fixture.client.is_connected());

    // The new session does not log on by itself with the old session's logon.
    settle().await;
    assert_eq!(vec![EMsg::ClientHello as u32], sent_without_heartbeats(&second));

    // The old connection can no longer disturb the new session.
    first.receive(message(EMsg::ClientPersonaState, 99));
    first.server_disconnect();
    settle().await;
    assert_eq!(vec![true], fixture.recorder.disconnections());
    assert!(!fixture.recorder.messages().iter().any(|(_, job)| *job == 99));
    assert!(fixture.client.is_connected());

    second.receive(message(EMsg::ClientPersonaState, 100));
    eventually("the new session dispatches", || {
        fixture.recorder.messages().iter().any(|(_, job)| *job == 100)
    })
    .await;
}

#[tokio::test]
async fn a_server_disconnect_after_a_reconnect_is_not_resumed_with_the_old_logon() {
    let fixture = Fixture::new().await;
    fixture.connect_and_log_on().await;
    let second = fixture.connect().await;

    second.server_disconnect();

    eventually("the disconnection was reported", || fixture.recorder.disconnections() == vec![true, false])
        .await;
    settle().await;
    assert_eq!(2, fixture.connection_count());
}
