//! Tests for the connection event plumbing in
//! `networking/steam3/i_connection.rs`.
//!
//! The events go through channels, which could drop or reorder them. These
//! tests make sure the ordered stream that drives the protocol does neither.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use steamkit::networking::steam3::disconnected_event_args::DisconnectedEventArgs;
use steamkit::networking::steam3::i_connection::{
    ConnectionEvent, ConnectionEventSenders, CONNECTION_EVENT_CAPACITY,
};
use steamkit::networking::steam3::net_msg_event_args::NetMsgEventArgs;
use tokio::sync::broadcast::error::RecvError;

fn end_point() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 27017))
}

fn message(index: u32) -> NetMsgEventArgs {
    NetMsgEventArgs {
        data: index.to_le_bytes().to_vec(),
        end_point: end_point(),
    }
}

fn index_of(event: &ConnectionEvent) -> u32 {
    match event {
        ConnectionEvent::NetMsgReceived(args) => {
            u32::from_le_bytes([args.data[0], args.data[1], args.data[2], args.data[3]])
        }
        other => panic!("expected a message, got {other:?}"),
    }
}

/// The scenario that used to lose messages: the connection raises far more
/// events than the broadcast capacity before the subscriber reads any.
#[tokio::test]
async fn ordered_stream_never_drops_events_when_the_subscriber_falls_behind() {
    let senders = ConnectionEventSenders::with_default_capacity();
    let mut events = senders.subscribe_events();

    let count = (CONNECTION_EVENT_CAPACITY * 50) as u32;
    senders.emit_connected();
    for index in 0..count {
        senders.emit_net_msg_received(message(index));
    }
    senders.emit_disconnected(DisconnectedEventArgs::new(false));

    assert_eq!(Some(ConnectionEvent::Connected), events.recv().await);
    for expected in 0..count {
        let event = events.recv().await.expect("the stream ended early");
        assert_eq!(expected, index_of(&event), "events were lost or reordered");
    }
    assert_eq!(
        Some(ConnectionEvent::Disconnected(DisconnectedEventArgs::new(false))),
        events.recv().await
    );
}

/// The broadcast receivers are still lossy; this documents why nothing in the
/// library uses them to drive the protocol.
#[tokio::test]
async fn broadcast_receiver_lags_where_the_ordered_stream_does_not() {
    let senders = ConnectionEventSenders::with_default_capacity();
    let mut broadcast = senders.subscribe_net_msg_received();
    let mut ordered = senders.subscribe_events();

    let count = (CONNECTION_EVENT_CAPACITY * 2) as u32;
    for index in 0..count {
        senders.emit_net_msg_received(message(index));
    }

    assert!(matches!(broadcast.recv().await, Err(RecvError::Lagged(_))));

    for expected in 0..count {
        assert_eq!(expected, index_of(&ordered.recv().await.unwrap()));
    }
}

#[tokio::test]
async fn ordered_stream_keeps_the_order_of_different_event_kinds() {
    let senders = ConnectionEventSenders::with_default_capacity();
    let mut events = senders.subscribe_events();

    senders.emit_connected();
    senders.emit_net_msg_received(message(1));
    senders.emit_disconnected(DisconnectedEventArgs::new(true));
    senders.emit_net_msg_received(message(2));
    senders.emit_connected();

    assert_eq!(Some(ConnectionEvent::Connected), events.recv().await);
    assert_eq!(1, index_of(&events.recv().await.unwrap()));
    assert_eq!(
        Some(ConnectionEvent::Disconnected(DisconnectedEventArgs::new(true))),
        events.recv().await
    );
    assert_eq!(2, index_of(&events.recv().await.unwrap()));
    assert_eq!(Some(ConnectionEvent::Connected), events.recv().await);
}

#[tokio::test]
async fn every_subscriber_receives_every_event() {
    let senders = ConnectionEventSenders::with_default_capacity();
    let mut first = senders.subscribe_events();
    let mut second = senders.subscribe_events();

    for index in 0..500 {
        senders.emit_net_msg_received(message(index));
    }

    for expected in 0..500 {
        assert_eq!(expected, index_of(&first.recv().await.unwrap()));
        assert_eq!(expected, index_of(&second.recv().await.unwrap()));
    }
}

#[tokio::test]
async fn a_subscriber_only_sees_events_raised_after_it_subscribed() {
    let senders = ConnectionEventSenders::with_default_capacity();
    senders.emit_net_msg_received(message(1));

    let mut events = senders.subscribe_events();
    senders.emit_net_msg_received(message(2));

    assert_eq!(2, index_of(&events.recv().await.unwrap()));
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn dropped_subscribers_are_forgotten() {
    let senders = ConnectionEventSenders::with_default_capacity();
    let first = senders.subscribe_events();
    let mut second = senders.subscribe_events();
    assert_eq!(2, senders.event_subscriber_count());

    drop(first);
    senders.emit_connected();
    assert_eq!(1, senders.event_subscriber_count());
    assert_eq!(Some(ConnectionEvent::Connected), second.recv().await);

    drop(second);
    senders.emit_connected();
    assert_eq!(0, senders.event_subscriber_count());
}

#[test]
fn emitting_without_subscribers_does_nothing() {
    let senders = ConnectionEventSenders::default();
    senders.emit_connected();
    senders.emit_net_msg_received(message(1));
    senders.emit_disconnected(DisconnectedEventArgs::new(false));
    assert_eq!(0, senders.event_subscriber_count());
}

#[tokio::test]
async fn clones_share_the_same_subscribers() {
    let senders = ConnectionEventSenders::with_default_capacity();
    let clone = senders.clone();
    let mut events = senders.subscribe_events();

    clone.emit_net_msg_received(message(7));

    assert_eq!(7, index_of(&events.recv().await.unwrap()));
}

/// Events raised from several threads at once still arrive in one order that
/// every subscriber agrees on, and each thread's own events keep their order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_emitters_produce_one_consistent_order() {
    const THREADS: u32 = 4;
    const PER_THREAD: u32 = 2_000;

    let senders = ConnectionEventSenders::with_default_capacity();
    let mut first = senders.subscribe_events();
    let mut second = senders.subscribe_events();

    let barrier = Arc::new(std::sync::Barrier::new(THREADS as usize));
    let handles: Vec<_> = (0..THREADS)
        .map(|thread| {
            let senders = senders.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for sequence in 0..PER_THREAD {
                    senders.emit_net_msg_received(message(thread << 24 | sequence));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let mut next_sequence = [0u32; THREADS as usize];
    for _ in 0..THREADS * PER_THREAD {
        let a = index_of(&first.recv().await.unwrap());
        let b = index_of(&second.recv().await.unwrap());
        assert_eq!(a, b, "two subscribers saw different orders");

        let thread = (a >> 24) as usize;
        let sequence = a & 0x00FF_FFFF;
        assert_eq!(next_sequence[thread], sequence, "a thread's events were reordered");
        next_sequence[thread] += 1;
    }

    assert!(tokio::time::timeout(Duration::from_millis(20), first.recv()).await.is_err());
}
