//! Tests for `networking/steam3/tcp_connection.rs` against a real socket on the
//! loopback interface.
//!
//! The wire format is a 4 byte little-endian payload length, the magic
//! `VT01`, then the payload.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use bytes::Bytes;
use steamkit::networking::steam3::disconnected_event_args::DisconnectedEventArgs;
use steamkit::networking::steam3::i_connection::{ConnectionEvent, ConnectionEvents, IConnection};
use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::networking::steam3::tcp_connection::TcpConnection;
use steamkit::SteamError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedReceiver;

const TIMEOUT: Duration = Duration::from_secs(5);

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut framed = (payload.len() as u32).to_le_bytes().to_vec();
    framed.extend_from_slice(b"VT01");
    framed.extend_from_slice(payload);
    framed
}

async fn listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

/// Connects a fresh client and returns it with its event stream and the
/// server's end of the socket.
async fn connected_pair() -> (TcpConnection, UnboundedReceiver<ConnectionEvent>, TcpStream) {
    let (listener, address) = listener().await;
    let client = TcpConnection::new();
    let events = client.events();

    let (connected, accepted) = tokio::join!(client.connect(address, TIMEOUT), listener.accept());
    connected.unwrap();
    let (server, _) = accepted.unwrap();

    (client, events, server)
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

fn payload_of(event: ConnectionEvent) -> Vec<u8> {
    match event {
        ConnectionEvent::NetMsgReceived(args) => args.data,
        other => panic!("expected a message, got {other:?}"),
    }
}

#[tokio::test]
async fn reports_its_endpoints_and_protocol() {
    let (client, mut events, _server) = connected_pair().await;

    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);
    assert!(client.current_end_point().is_some());
    assert_eq!(Some(IpAddr::V4(Ipv4Addr::LOCALHOST)), client.local_ip());
    assert_eq!(ProtocolTypes::TCP, client.protocol_types());
}

#[tokio::test]
async fn sends_each_message_in_its_own_frame() {
    let (client, _events, mut server) = connected_pair().await;

    client.send(Bytes::from_static(b"hello")).await.unwrap();
    client.send(Bytes::from_static(b"")).await.unwrap();
    client.send(Bytes::from(vec![0xAB; 3000])).await.unwrap();

    let mut expected = frame(b"hello");
    expected.extend(frame(b""));
    expected.extend(frame(&[0xAB; 3000]));

    let mut received = vec![0u8; expected.len()];
    tokio::time::timeout(TIMEOUT, server.read_exact(&mut received)).await.unwrap().unwrap();
    assert_eq!(expected, received);
}

#[tokio::test]
async fn receives_messages_split_across_reads() {
    let (_client, mut events, mut server) = connected_pair().await;
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    let framed = frame(b"split across many writes");
    for byte in framed {
        server.write_all(&[byte]).await.unwrap();
        server.flush().await.unwrap();
    }

    assert_eq!(b"split across many writes".to_vec(), payload_of(next(&mut events).await));
}

/// The server speaks first: it sends `ChannelEncryptRequest` the moment the
/// socket is open. `Connected` must still come before that message, every time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connected_always_precedes_the_first_message() {
    for _ in 0..50 {
        let (listener, address) = listener().await;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.write_all(&frame(b"first")).await.unwrap();
            socket
        });

        let client = TcpConnection::new();
        let mut events = client.events();
        client.connect(address, TIMEOUT).await.unwrap();
        let _server = server.await.unwrap();

        assert_eq!(ConnectionEvent::Connected, next(&mut events).await);
        assert_eq!(b"first".to_vec(), payload_of(next(&mut events).await));
    }
}

/// The scenario that used to drop messages: many messages arrive while the
/// consumer is busy.
#[tokio::test]
async fn a_burst_of_messages_is_delivered_completely_and_in_order() {
    let (_client, mut events, mut server) = connected_pair().await;

    const COUNT: u32 = 5_000;
    let mut burst = Vec::new();
    for index in 0..COUNT {
        burst.extend(frame(&index.to_le_bytes()));
    }
    server.write_all(&burst).await.unwrap();

    // The consumer is busy while the burst arrives.
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);
    for expected in 0..COUNT {
        assert_eq!(expected.to_le_bytes().to_vec(), payload_of(next(&mut events).await));
    }
}

#[tokio::test]
async fn an_invalid_magic_closes_the_connection() {
    let (_client, mut events, mut server) = connected_pair().await;
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    let mut framed = frame(b"payload");
    framed[4..8].copy_from_slice(b"VT02");
    server.write_all(&framed).await.unwrap();

    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false)),
        next(&mut events).await
    );
}

#[tokio::test]
async fn an_implausible_length_closes_the_connection_without_allocating_it() {
    let (_client, mut events, mut server) = connected_pair().await;
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    let mut header = u32::MAX.to_le_bytes().to_vec();
    header.extend_from_slice(b"VT01");
    server.write_all(&header).await.unwrap();

    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false)),
        next(&mut events).await
    );
}

#[tokio::test]
async fn a_frame_cut_off_by_the_server_is_not_delivered() {
    let (_client, mut events, mut server) = connected_pair().await;
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    let framed = frame(b"this payload never fully arrives");
    server.write_all(&framed[..framed.len() - 5]).await.unwrap();
    drop(server);

    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false)),
        next(&mut events).await
    );
    assert_no_event(&mut events).await;
}

#[tokio::test]
async fn the_server_closing_raises_disconnected_once() {
    let (client, mut events, server) = connected_pair().await;
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    drop(server);
    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false)),
        next(&mut events).await
    );

    // Disconnecting a connection the server has already closed says nothing more.
    client.disconnect(true).await;
    assert_no_event(&mut events).await;
}

#[tokio::test]
async fn a_user_disconnect_raises_disconnected_once_and_closes_the_socket() {
    let (client, mut events, mut server) = connected_pair().await;
    assert_eq!(ConnectionEvent::Connected, next(&mut events).await);

    client.disconnect(true).await;
    assert_eq!(
        ConnectionEvent::Disconnected(DisconnectedEventArgs::new(true)),
        next(&mut events).await
    );

    client.disconnect(true).await;
    assert_no_event(&mut events).await;

    let mut buffer = [0u8; 1];
    let read = tokio::time::timeout(TIMEOUT, server.read(&mut buffer)).await.unwrap();
    assert!(matches!(read, Ok(0) | Err(_)), "the server must see the socket close");
}

#[tokio::test]
async fn sending_after_a_disconnect_is_a_silent_no_op() {
    let (client, _events, _server) = connected_pair().await;
    client.disconnect(true).await;
    client.send(Bytes::from_static(b"late")).await.unwrap();
}

#[tokio::test]
async fn sending_before_connecting_is_a_silent_no_op() {
    let client = TcpConnection::new();
    client.send(Bytes::from_static(b"early")).await.unwrap();
    assert_eq!(None, client.current_end_point());
}

#[tokio::test]
async fn connecting_to_a_closed_port_fails() {
    let (listener, address) = listener().await;
    drop(listener);

    let client = TcpConnection::new();
    let result = client.connect(address, TIMEOUT).await;
    assert!(
        matches!(result, Err(SteamError::Connection(_)) | Err(SteamError::Timeout(_))),
        "got {result:?}"
    );
}

#[tokio::test]
async fn the_connection_can_be_reused_after_a_disconnect() {
    let (listener, address) = listener().await;
    let client = TcpConnection::new();
    let mut events = client.events();

    for round in 0..3u8 {
        let (connected, accepted) = tokio::join!(client.connect(address, TIMEOUT), listener.accept());
        connected.unwrap();
        let (mut server, _) = accepted.unwrap();

        assert_eq!(ConnectionEvent::Connected, next(&mut events).await);
        server.write_all(&frame(&[round])).await.unwrap();
        assert_eq!(vec![round], payload_of(next(&mut events).await));

        client.disconnect(true).await;
        assert_eq!(
            ConnectionEvent::Disconnected(DisconnectedEventArgs::new(true)),
            next(&mut events).await
        );
    }
}
