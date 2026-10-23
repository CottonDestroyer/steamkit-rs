//! `SmartCMServerList` tests.
//!
//! Every test builds its own list from its own configuration, so they're
//! independent.
//!
//! * The WebSocket record factory resolves the host name up front, so it's
//!   async. The names used here resolve locally (`localhost` or IP literals).
//! * `get_next_server_candidate` doesn't wait for a background fetch; it
//!   returns the best candidate known right now. Every test fills the list
//!   first, so that doesn't matter here.

use std::net::SocketAddr;
use std::time::Duration;

use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::steam::discovery::server_record::ServerRecord;
use steamkit::steam::discovery::smart_cm_server_list::{ServerQuality, SmartCMServerList};
use steamkit::steam::steam_client::configuration::i_steam_configuration_builder::ISteamConfigurationBuilder;
use steamkit::steam::steam_client::configuration::steam_configuration::SteamConfiguration;

/// A configuration with directory fetch disabled, so no test can reach the
/// Steam Directory Web API.
fn configuration() -> SteamConfiguration {
    SteamConfiguration::create(|builder| {
        builder.with_directory_fetch(false);
    })
}

/// A fresh server list.
fn server_list() -> SmartCMServerList {
    SmartCMServerList::new(configuration().state().clone())
}

/// A socket server record.
fn socket_server(ip: [u8; 4], port: u16) -> ServerRecord {
    ServerRecord::create_socket_server(SocketAddr::from((ip, port)))
}

/// A WebSocket server record. The factory resolves the host name, so it's
/// async.
async fn websocket_server(host: &str, port: u16) -> ServerRecord {
    ServerRecord::create_websocket_server(&format!("{host}:{port}"))
        .await
        .expect("the websocket server address should resolve")
}

/// Replaces the list and writes it to the provider.
async fn replace_list(list: &SmartCMServerList, endpoint_list: Vec<ServerRecord>) {
    let result = list.replace_list(endpoint_list, true, None).await;
    assert!(result.is_ok(), "ReplaceList should succeed: {result:?}");
}

/// Marks a server with the given quality.
fn try_mark(
    list: &SmartCMServerList,
    record: &ServerRecord,
    protocol_types: ProtocolTypes,
    quality: ServerQuality,
) -> bool {
    list.try_mark(record.end_point(), protocol_types, quality)
}

/// The next server candidate, which must exist.
fn next_candidate(list: &SmartCMServerList, protocol_types: ProtocolTypes) -> ServerRecord {
    list.get_next_server_candidate(protocol_types)
        .expect("GetNextServerCandidate should return a record")
}

#[tokio::test]
async fn try_merge_with_list_adds_to_head_and_moves_existing() {
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let seed_list = vec![
        socket_server([127, 0, 0, 1], 27025),
        socket_server([127, 0, 0, 1], 27035),
        socket_server([127, 0, 0, 1], 27045),
        socket_server([127, 0, 0, 1], 27105),
    ];
    replace_list(&list, seed_list.clone()).await;
    assert_eq!(4, seed_list.len());

    let list_to_replace = vec![
        socket_server([127, 0, 0, 1], 27015),
        socket_server([127, 0, 0, 1], 27035),
        socket_server([127, 0, 0, 1], 27105),
    ];

    replace_list(&list, list_to_replace.clone()).await;

    let addresses = list.get_all_end_points();
    assert_eq!(3, addresses.len());
    assert_eq!(list_to_replace[0], addresses[0]);
    assert_eq!(list_to_replace[1], addresses[1]);
    assert_eq!(list_to_replace[2], addresses[2]);
}

#[test]
fn get_next_server_candidate_returns_null_if_list_is_empty() {
    let list = server_list();

    assert!(
        list.get_next_server_candidate(ProtocolTypes::TCP).is_none(),
        "an empty list has no candidates"
    );
}

#[tokio::test]
async fn get_next_server_candidate_returns_server_if_list_has_servers() {
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let record = socket_server([127, 0, 0, 1], 27015);
    replace_list(&list, vec![record.clone()]).await;

    let next_record = next_candidate(&list, ProtocolTypes::TCP);
    assert_eq!(record.end_point(), next_record.end_point());
    assert_eq!(ProtocolTypes::TCP, next_record.protocol_types());
}

#[tokio::test]
async fn get_next_server_candidate_only_returns_matching_server_of_type() {
    let list = server_list();

    let record = websocket_server("localhost", 443).await;
    replace_list(&list, vec![record.clone()]).await;

    assert!(list.get_next_server_candidate(ProtocolTypes::TCP).is_none());
    assert!(list.get_next_server_candidate(ProtocolTypes::UDP).is_none());
    assert!(
        list.get_next_server_candidate(ProtocolTypes::TCP | ProtocolTypes::UDP)
            .is_none()
    );

    let end_point = next_candidate(&list, ProtocolTypes::WEB_SOCKET);
    assert_eq!(record.end_point(), end_point.end_point());
    assert_eq!(ProtocolTypes::WEB_SOCKET, end_point.protocol_types());

    let end_point = next_candidate(&list, ProtocolTypes::ALL);
    assert_eq!(record.end_point(), end_point.end_point());
    assert_eq!(ProtocolTypes::WEB_SOCKET, end_point.protocol_types());

    let record = socket_server([127, 0, 0, 1], 27015);
    replace_list(&list, vec![record.clone()]).await;

    assert!(list.get_next_server_candidate(ProtocolTypes::WEB_SOCKET).is_none());

    let end_point = next_candidate(&list, ProtocolTypes::TCP);
    assert_eq!(record.end_point(), end_point.end_point());
    assert_eq!(ProtocolTypes::TCP, end_point.protocol_types());

    let end_point = next_candidate(&list, ProtocolTypes::UDP);
    assert_eq!(record.end_point(), end_point.end_point());
    assert_eq!(ProtocolTypes::UDP, end_point.protocol_types());

    let end_point = next_candidate(&list, ProtocolTypes::TCP | ProtocolTypes::UDP);
    assert_eq!(record.end_point(), end_point.end_point());
    assert_eq!(ProtocolTypes::TCP, end_point.protocol_types());

    let end_point = next_candidate(&list, ProtocolTypes::ALL);
    assert_eq!(record.end_point(), end_point.end_point());
    assert_eq!(ProtocolTypes::TCP, end_point.protocol_types());
}

#[tokio::test]
async fn get_next_server_candidate_returns_server_if_list_has_servers_even_if_all_servers_are_bad()
{
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let record = socket_server([127, 0, 0, 1], 27015);
    replace_list(&list, vec![record.clone()]).await;
    assert!(try_mark(
        &list,
        &record,
        record.protocol_types(),
        ServerQuality::Bad
    ));

    let next_record = next_candidate(&list, ProtocolTypes::TCP);
    assert_eq!(record.end_point(), next_record.end_point());
    assert_eq!(ProtocolTypes::TCP, next_record.protocol_types());
}

#[tokio::test]
async fn get_next_server_candidate_is_biased_towards_server_ordering() {
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let server_a = [10, 0, 0, 1];
    let server_b = [10, 0, 0, 2];

    let good_record = socket_server(server_a, 27015);
    let neutral_record = socket_server(server_a, 27016);
    let bad_record = socket_server(server_a, 27017);
    let server_b_record = socket_server(server_b, 27017);

    replace_list(
        &list,
        vec![
            bad_record.clone(),
            neutral_record.clone(),
            good_record.clone(),
            server_b_record.clone(),
        ],
    )
    .await;

    assert!(try_mark(
        &list,
        &bad_record,
        bad_record.protocol_types(),
        ServerQuality::Bad
    ));
    assert!(try_mark(
        &list,
        &good_record,
        good_record.protocol_types(),
        ServerQuality::Good
    ));

    // Server A's endpoints were all marked bad, with goodRecord being recovered
    let next_record = next_candidate(&list, ProtocolTypes::TCP);
    assert_eq!(good_record.end_point(), next_record.end_point());
    assert_eq!(ProtocolTypes::TCP, next_record.protocol_types());

    assert!(try_mark(
        &list,
        &bad_record,
        bad_record.protocol_types(),
        ServerQuality::Good
    ));

    // Server A's bad record is now at the front, having been marked good
    let next_record = next_candidate(&list, ProtocolTypes::TCP);
    assert_eq!(bad_record.end_point(), next_record.end_point());
    assert_eq!(ProtocolTypes::TCP, next_record.protocol_types());
}

#[tokio::test]
async fn get_next_server_candidate_all_endpoints_by_host_are_bad() {
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let server_a = [10, 0, 0, 1];
    let server_b = [10, 0, 0, 2];

    let good_record = socket_server(server_a, 27015);
    let neutral_record = socket_server(server_a, 27016);
    let bad_record = socket_server(server_a, 27017);
    let server_b_record = socket_server(server_b, 27017);

    replace_list(
        &list,
        vec![
            good_record.clone(),
            neutral_record.clone(),
            bad_record.clone(),
            server_b_record.clone(),
        ],
    )
    .await;

    assert!(try_mark(
        &list,
        &good_record,
        good_record.protocol_types(),
        ServerQuality::Good
    ));
    assert!(try_mark(
        &list,
        &bad_record,
        bad_record.protocol_types(),
        ServerQuality::Bad
    ));

    // Server A's endpoints are all bad. Server B is our next candidate.
    let next_record = next_candidate(&list, ProtocolTypes::TCP);
    assert_eq!(server_b_record.end_point(), next_record.end_point());
    assert_eq!(ProtocolTypes::TCP, next_record.protocol_types());
}

#[tokio::test]
async fn get_next_server_candidate_mark_iterate_all_candidates() {
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let record_a = websocket_server("10.0.0.1", 27030).await;
    let record_b = websocket_server("10.0.0.2", 27030).await;
    let record_c = websocket_server("10.0.0.3", 27030).await;

    // Add all candidates
    replace_list(&list, vec![record_a, record_b, record_c]).await;

    let mut candidates_returned: Vec<ServerRecord> = Vec::new();

    async fn dequeue_and_mark_candidate(
        list: &SmartCMServerList,
        candidates_returned: &mut Vec<ServerRecord>,
    ) {
        let candidate = next_candidate(list, ProtocolTypes::WEB_SOCKET);

        assert!(
            !candidates_returned.contains(&candidate),
            "Candidate {} already seen",
            candidate.end_point()
        );
        candidates_returned.push(candidate.clone());

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(list.try_mark(
            candidate.end_point(),
            ProtocolTypes::WEB_SOCKET,
            ServerQuality::Bad
        ));
    }

    // We must dequeue all servers as they all get marked bad
    dequeue_and_mark_candidate(&list, &mut candidates_returned).await;
    dequeue_and_mark_candidate(&list, &mut candidates_returned).await;
    dequeue_and_mark_candidate(&list, &mut candidates_returned).await;
    assert_eq!(3, candidates_returned.len(), "All candidates returned");
}

#[tokio::test]
async fn get_next_server_candidate_mark_iterate_all_bad_candidates() {
    let list = server_list();

    // Make sure the (empty) list has been resolved.
    assert!(list.get_all_end_points().is_empty());

    let record_a = websocket_server("10.0.0.1", 27030).await;
    let record_b = websocket_server("10.0.0.2", 27030).await;
    let record_c = websocket_server("10.0.0.3", 27030).await;

    // Add all candidates and mark them bad
    replace_list(&list, vec![record_a.clone(), record_b.clone(), record_c.clone()]).await;
    assert!(try_mark(&list, &record_a, ProtocolTypes::WEB_SOCKET, ServerQuality::Bad));
    assert!(try_mark(&list, &record_b, ProtocolTypes::WEB_SOCKET, ServerQuality::Bad));
    assert!(try_mark(&list, &record_c, ProtocolTypes::WEB_SOCKET, ServerQuality::Bad));

    let mut candidates_returned: Vec<ServerRecord> = Vec::new();

    async fn dequeue_and_mark_candidate(
        list: &SmartCMServerList,
        candidates_returned: &mut Vec<ServerRecord>,
    ) {
        let candidate = next_candidate(list, ProtocolTypes::WEB_SOCKET);

        assert!(
            !candidates_returned.contains(&candidate),
            "Candidate {} already seen",
            candidate.end_point()
        );
        candidates_returned.push(candidate.clone());

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(list.try_mark(
            candidate.end_point(),
            ProtocolTypes::WEB_SOCKET,
            ServerQuality::Bad
        ));
    }

    // We must dequeue all candidates from a bad list
    dequeue_and_mark_candidate(&list, &mut candidates_returned).await;
    dequeue_and_mark_candidate(&list, &mut candidates_returned).await;
    dequeue_and_mark_candidate(&list, &mut candidates_returned).await;
    assert_eq!(3, candidates_returned.len(), "All candidates returned");
}

#[tokio::test]
async fn try_mark_returns_true_if_server_in_list() {
    let list = server_list();

    let record = socket_server([127, 0, 0, 1], 27015);
    replace_list(&list, vec![record.clone()]).await;

    let marked = try_mark(&list, &record, record.protocol_types(), ServerQuality::Good);
    assert!(marked);
}

#[tokio::test]
async fn try_mark_returns_false_if_server_not_in_list() {
    let list = server_list();

    let record = socket_server([127, 0, 0, 1], 27015);
    replace_list(&list, vec![record.clone()]).await;

    let not_in_list = socket_server([127, 0, 0, 1], 27016);
    let marked = try_mark(&list, &not_in_list, record.protocol_types(), ServerQuality::Good);
    assert!(!marked);
}
