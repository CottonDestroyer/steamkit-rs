//! Tests for `networking/steam3/envelope_encrypted_connection.rs`, the channel
//! encryption handshake that runs on top of the TCP and UDP transports.
//!
//! The CM server is played by a [`mock::MockConnection`]. The test can't
//! decrypt the session key (that takes Valve's private key), but everything
//! the client does up to and including the switch to the encrypted state is
//! observable.

#[allow(dead_code)]
mod mock;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use mock::{eventually, settle, ConnectBehaviour, MockConnection};
use steamkit::base::generated::steam_language::{EMsg, EResult, EUniverse};
use steamkit::networking::steam3::disconnected_event_args::DisconnectedEventArgs;
use steamkit::networking::steam3::envelope_encrypted_connection::{
    DebugNetworkListener, EnvelopeEncryptedConnection,
};
use steamkit::networking::steam3::i_connection::{ConnectionEvent, ConnectionEvents, IConnection};
use tokio::sync::mpsc::UnboundedReceiver;

const TIMEOUT: Duration = Duration::from_secs(5);

/// `MsgHdr`: the message type, then the target and source job ids.
fn msg_hdr(msg: EMsg) -> Vec<u8> {
    let mut data = (msg as i32).to_le_bytes().to_vec();
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    data
}

fn encrypt_request(universe: i32, challenge_length: usize) -> Vec<u8> {
    let mut data = msg_hdr(EMsg::ChannelEncryptRequest);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&universe.to_le_bytes());
    data.extend((0..challenge_length).map(|index| index as u8));
    data
}

fn encrypt_result(result: EResult) -> Vec<u8> {
    let mut data = msg_hdr(EMsg::ChannelEncryptResult);
    data.extend_from_slice(&(result as i32).to_le_bytes());
    data
}

/// CRC-32 (IEEE), the checksum the response carries over the encrypted key.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Checks the layout of a `ChannelEncryptResponse`.
fn assert_is_encrypt_response(data: &[u8]) {
    // header (20) + protocol version (4) + key size (4) + key (128) + crc (4) + 0 (4)
    assert_eq!(164, data.len(), "response length");
    assert_eq!(msg_hdr(EMsg::ChannelEncryptResponse), data[..20], "response header");
    assert_eq!(1u32.to_le_bytes(), data[20..24], "protocol version");
    assert_eq!(128u32.to_le_bytes(), data[24..28], "key size");

    let key = &data[28..156];
    assert_eq!(crc32(key).to_le_bytes(), data[156..160], "key crc");
    assert_eq!([0u8; 4], data[160..164], "trailing zero");
}

struct Fixture {
    inner: Arc<MockConnection>,
    envelope: EnvelopeEncryptedConnection,
    events: UnboundedReceiver<ConnectionEvent>,
}

async fn fixture() -> Fixture {
    let inner = MockConnection::new(ConnectBehaviour::Silent);
    let envelope = EnvelopeEncryptedConnection::new(inner.clone(), EUniverse::Public);
    let events = envelope.events();
    envelope.connect(mock::default_end_point(), TIMEOUT).await.unwrap();
    Fixture { inner, envelope, events }
}

async fn next(events: &mut UnboundedReceiver<ConnectionEvent>) -> ConnectionEvent {
    tokio::time::timeout(TIMEOUT, events.recv())
        .await
        .expect("no event arrived")
        .expect("the event stream ended")
}

async fn assert_no_event(events: &mut UnboundedReceiver<ConnectionEvent>) {
    if let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), events.recv()).await {
        panic!("unexpected event {event:?}");
    }
}

/// Runs the handshake up to the encrypted state.
async fn handshake(fixture: &mut Fixture) {
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
    fixture.inner.receive(encrypt_result(EResult::OK));
    assert_eq!(ConnectionEvent::Connected, next(&mut fixture.events).await);
}

/// The request arrives right behind `Connected`, with nothing in between - as
/// it does on a real socket. The two events used to travel on separate
/// channels and tasks, and the request could be handled first and rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn answers_a_request_that_arrives_right_behind_connected() {
    for _ in 0..50 {
        let fixture = fixture().await;
        fixture.inner.raise_connected();
        fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));

        eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
        assert_is_encrypt_response(&fixture.inner.sent()[0]);
    }
}

#[tokio::test]
async fn a_longer_challenge_is_accepted() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 32));

    eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
    assert_is_encrypt_response(&fixture.inner.sent()[0]);
}

/// The session key and the challenge are encrypted together with RSA-1024
/// OAEP-SHA1, which holds at most 86 bytes: 32 for the key leaves 54 for the
/// challenge. A longer challenge can't be answered.
#[tokio::test]
async fn a_challenge_too_long_to_encrypt_disconnects() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 55));

    eventually("the connection was closed", || fixture.inner.disconnect_calls() == vec![false]).await;
    assert!(fixture.inner.sent().is_empty());
}

#[tokio::test]
async fn every_response_carries_a_fresh_session_key() {
    let mut keys = Vec::new();
    for _ in 0..3 {
        let fixture = fixture().await;
        fixture.inner.raise_connected();
        fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
        eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
        keys.push(fixture.inner.sent()[0][28..156].to_vec());
    }
    assert_ne!(keys[0], keys[1]);
    assert_ne!(keys[1], keys[2]);
}

#[tokio::test]
async fn connected_is_raised_only_once_the_channel_is_encrypted() {
    let mut fixture = fixture().await;

    fixture.inner.raise_connected();
    assert_no_event(&mut fixture.events).await;

    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
    assert_no_event(&mut fixture.events).await;

    fixture.inner.receive(encrypt_result(EResult::OK));
    assert_eq!(ConnectionEvent::Connected, next(&mut fixture.events).await);
}

#[tokio::test]
async fn a_failed_encrypt_result_disconnects() {
    let mut fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    eventually("the response was sent", || fixture.inner.sent().len() == 1).await;

    fixture.inner.receive(encrypt_result(EResult::Fail));

    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false)),
        next(&mut fixture.events).await
    );
    assert_eq!(vec![false], fixture.inner.disconnect_calls());
}

#[tokio::test]
async fn a_challenge_shorter_than_16_bytes_disconnects() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 15));

    eventually("the connection was closed", || fixture.inner.disconnect_calls() == vec![false]).await;
    assert!(fixture.inner.sent().is_empty());
}

#[tokio::test]
async fn a_truncated_request_disconnects() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(msg_hdr(EMsg::ChannelEncryptRequest));

    eventually("the connection was closed", || fixture.inner.disconnect_calls() == vec![false]).await;
    assert!(fixture.inner.sent().is_empty());
}

#[tokio::test]
async fn a_request_for_a_universe_without_a_key_disconnects() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Invalid as i32, 16));

    eventually("the connection was closed", || fixture.inner.disconnect_calls() == vec![false]).await;
    assert!(fixture.inner.sent().is_empty());
}

/// A universe mismatch is only logged, and the request is still answered.
#[tokio::test]
async fn a_request_for_another_universe_is_still_answered() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Beta as i32, 16));

    eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
    assert_is_encrypt_response(&fixture.inner.sent()[0]);
}

/// The handshake used to carry its own copy of the key table, whose `Dev` key
/// was corrupt: a `Dev` universe server could never be answered.
#[tokio::test]
async fn a_request_is_answered_in_every_universe() {
    for universe in [EUniverse::Public, EUniverse::Beta, EUniverse::Internal, EUniverse::Dev] {
        let inner = MockConnection::new(ConnectBehaviour::Silent);
        let envelope = EnvelopeEncryptedConnection::new(inner.clone(), universe);
        envelope.connect(mock::default_end_point(), TIMEOUT).await.unwrap();

        inner.raise_connected();
        inner.receive(encrypt_request(universe as i32, 16));

        eventually("the response was sent", || inner.sent().len() == 1 || !inner.disconnect_calls().is_empty())
            .await;
        assert!(inner.disconnect_calls().is_empty(), "{universe:?}: the handshake was abandoned");
        assert_is_encrypt_response(&inner.sent()[0]);
    }
}

#[tokio::test]
async fn a_message_too_short_to_parse_during_setup_disconnects() {
    let fixture = fixture().await;
    fixture.inner.raise_connected();
    fixture.inner.receive(vec![1, 2, 3]);

    eventually("the connection was closed", || fixture.inner.disconnect_calls() == vec![false]).await;
}

#[tokio::test]
async fn unexpected_messages_during_setup_are_ignored() {
    let fixture = fixture().await;

    // Before `Connected`, nothing is expected.
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    settle().await;
    assert!(fixture.inner.sent().is_empty());

    // After `Connected`, only a request is expected.
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_result(EResult::OK));
    fixture.inner.receive(msg_hdr(EMsg::ClientLogOnResponse));
    settle().await;
    assert!(fixture.inner.sent().is_empty());
    assert!(fixture.inner.disconnect_calls().is_empty());

    // And the handshake still works afterwards.
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    eventually("the response was sent", || fixture.inner.sent().len() == 1).await;
}

#[tokio::test]
async fn messages_are_sent_in_the_clear_before_and_encrypted_after_the_handshake() {
    let mut fixture = fixture().await;

    fixture.envelope.send(Bytes::from_static(b"plain")).await.unwrap();
    assert_eq!(b"plain".to_vec(), fixture.inner.sent()[0]);

    handshake(&mut fixture).await;

    let message = vec![0x42u8; 40];
    fixture.envelope.send(Bytes::from(message.clone())).await.unwrap();
    let sent = fixture.inner.sent().pop().unwrap();
    assert_ne!(message, sent);
    assert_eq!(16 + 48, sent.len(), "the IV block plus the padded message");
}

#[tokio::test]
async fn an_encrypted_packet_that_does_not_decrypt_is_dropped() {
    let mut fixture = fixture().await;
    handshake(&mut fixture).await;

    fixture.inner.receive(vec![0u8; 48]);
    fixture.inner.receive(vec![0u8; 7]);

    assert_no_event(&mut fixture.events).await;
    assert!(fixture.inner.disconnect_calls().is_empty());
}

#[tokio::test]
async fn the_inner_disconnection_is_passed_on_and_resets_the_handshake() {
    let mut fixture = fixture().await;
    handshake(&mut fixture).await;

    fixture.inner.server_disconnect();
    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false)),
        next(&mut fixture.events).await
    );

    // A message sent now goes out in the clear: there is no session key.
    fixture.envelope.send(Bytes::from_static(b"after")).await.unwrap();
    assert_eq!(b"after".to_vec(), fixture.inner.sent().pop().unwrap());

    // And a new handshake works on the same object.
    let sent_before = fixture.inner.sent().len();
    fixture.inner.raise_connected();
    fixture.inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    eventually("the second response was sent", || fixture.inner.sent().len() == sent_before + 1).await;
    fixture.inner.receive(encrypt_result(EResult::OK));
    assert_eq!(ConnectionEvent::Connected, next(&mut fixture.events).await);
}

#[tokio::test]
async fn a_user_disconnect_is_passed_on_as_user_initiated() {
    let mut fixture = fixture().await;
    handshake(&mut fixture).await;

    fixture.envelope.disconnect(true).await;

    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(true)),
        next(&mut fixture.events).await
    );
    assert_eq!(vec![true], fixture.inner.disconnect_calls());
}

#[tokio::test]
async fn the_endpoints_are_those_of_the_inner_connection() {
    let fixture = fixture().await;
    assert_eq!(fixture.inner.current_end_point(), fixture.envelope.current_end_point());
    assert_eq!(fixture.inner.local_ip(), fixture.envelope.local_ip());
    assert_eq!(fixture.inner.protocol_types(), fixture.envelope.protocol_types());
}

/// The forwarding task used to hold the connection strongly, through a cycle
/// that kept both alive forever.
#[tokio::test]
async fn the_forwarding_task_ends_when_the_connection_is_dropped() {
    let fixture = fixture().await;
    let inner = fixture.inner.clone();
    assert_eq!(1, inner.subscriber_count());

    drop(fixture);
    // The task notices on the next event.
    inner.raise_connected();

    eventually("the forwarding task let go of the inner connection", || inner.subscriber_count() == 0)
        .await;
    assert_eq!(1, Arc::strong_count(&inner), "nothing else holds the inner connection");
}

#[derive(Default)]
struct RecordingListener {
    incoming: Mutex<Vec<EMsg>>,
    outgoing: Mutex<Vec<EMsg>>,
}

impl DebugNetworkListener for RecordingListener {
    fn on_incoming_network_message(&self, msg_type: EMsg, _data: &[u8]) {
        self.incoming.lock().unwrap().push(msg_type);
    }

    fn on_outgoing_network_message(&self, msg_type: EMsg, _data: &[u8]) {
        self.outgoing.lock().unwrap().push(msg_type);
    }
}

#[tokio::test]
async fn the_debug_listener_sees_the_handshake() {
    let inner = MockConnection::new(ConnectBehaviour::Silent);
    let listener = Arc::new(RecordingListener::default());
    let envelope = EnvelopeEncryptedConnection::with_debug_network_listener(
        inner.clone(),
        EUniverse::Public,
        Some(listener.clone()),
    );
    let mut events = envelope.events();
    envelope.connect(mock::default_end_point(), TIMEOUT).await.unwrap();

    inner.raise_connected();
    inner.receive(encrypt_request(EUniverse::Public as i32, 16));
    eventually("the response was sent", || inner.sent().len() == 1).await;
    inner.receive(encrypt_result(EResult::OK));
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    assert_eq!(
        vec![EMsg::ChannelEncryptRequest, EMsg::ChannelEncryptResult],
        *listener.incoming.lock().unwrap()
    );
    assert_eq!(vec![EMsg::ChannelEncryptResponse], *listener.outgoing.lock().unwrap());
}
