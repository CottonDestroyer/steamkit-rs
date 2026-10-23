//! Steam's reliable UDP transport: sequenced messages, piggy-backed acks and
//! retransmission.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};

use crate::base::generated::steam_language::EUdpPacketType;
use crate::error::SteamError;

use super::disconnected_event_args::DisconnectedEventArgs;
use super::i_connection::{ConnectionEvent, ConnectionEventSenders, ConnectionEvents, IConnection};
use super::net_msg_event_args::NetMsgEventArgs;
use super::protocol_type::ProtocolTypes;
use super::udp_packet::{ChallengeData, ConnectData, UdpPacket, MAX_PAYLOAD};

/// Seconds to wait before resending packets.
const RESEND_DELAY: Duration = Duration::from_secs(3);

/// Seconds to wait before considering the connection dead.
const TIMEOUT_DELAY: Duration = Duration::from_secs(60);

/// Maximum number of packets to resend when `RESEND_DELAY` is exceeded.
const RESEND_COUNT: usize = 3;

/// Maximum number of packets that we can be waiting on at a time.
const AHEAD_COUNT: u32 = 5;

/// How long the net loop waits for the first datagram of an iteration.
const OUTER_POLL: Duration = Duration::from_millis(150);

/// How long the net loop waits for additional datagrams while draining.
const INNER_POLL: Duration = Duration::from_millis(10);

/// Size of the receive buffer.
const RECEIVE_BUFFER_SIZE: usize = 2048;

/// Source connection id, shared by every UDP connection in the process. It
/// moves on by 256 on every disconnect, since Steam reuses the socket.
static SOURCE_CONN_ID: AtomicU32 = AtomicU32::new(512);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UdpState {
    Disconnected,
    ChallengeReqSent,
    ConnectSent,
    Connected,
    Disconnecting,
}

#[derive(Debug)]
struct UdpStateData {
    state: UdpState,
    remote_conn_id: u32,
    out_seq: u32,
    out_seq_sent: u32,
    out_seq_acked: u32,
    in_seq: u32,
    in_seq_acked: u32,
    in_seq_handled: u32,
    out_packets: Vec<UdpPacket>,
    in_packets: HashMap<u32, UdpPacket>,
    time_out: Instant,
    next_resend: Instant,
}

impl UdpStateData {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            state: UdpState::Disconnected,
            remote_conn_id: 0,
            out_seq: 1,
            out_seq_sent: 0,
            out_seq_acked: 0,
            in_seq: 0,
            in_seq_acked: 0,
            in_seq_handled: 0,
            out_packets: Vec::new(),
            in_packets: HashMap::new(),
            time_out: now + TIMEOUT_DELAY,
            next_resend: now + RESEND_DELAY,
        }
    }
}

#[derive(Debug)]
struct UdpShared {
    socket: Mutex<Option<Arc<UdpSocket>>>,
    current_end_point: Mutex<Option<SocketAddr>>,
    local_ip: Mutex<Option<IpAddr>>,
    state: tokio::sync::Mutex<UdpStateData>,
    disconnect_notified: AtomicBool,
    running: AtomicBool,
}

impl UdpShared {
    fn new() -> Self {
        Self {
            socket: Mutex::new(None),
            current_end_point: Mutex::new(None),
            local_ip: Mutex::new(None),
            state: tokio::sync::Mutex::new(UdpStateData::new()),
            disconnect_notified: AtomicBool::new(false),
            running: AtomicBool::new(false),
        }
    }
}

/// A Steam UDP connection.
#[derive(Debug)]
pub struct UdpConnection {
    events: ConnectionEventSenders,
    shared: Arc<UdpShared>,
}

impl UdpConnection {
    /// Creates a new, unconnected UDP connection.
    pub fn new() -> Self {
        Self {
            events: ConnectionEventSenders::with_default_capacity(),
            shared: Arc::new(UdpShared::new()),
        }
    }

    /// Subscribes to the `NetMsgReceived` event.
    pub fn subscribe_net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs> {
        self.events.subscribe_net_msg_received()
    }

    /// Subscribes to the `Connected` event.
    pub fn subscribe_connected(&self) -> broadcast::Receiver<()> {
        self.events.subscribe_connected()
    }

    /// Subscribes to the `Disconnected` event.
    pub fn subscribe_disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs> {
        self.events.subscribe_disconnected()
    }
}

impl Default for UdpConnection {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IConnection for UdpConnection {
    fn current_end_point(&self) -> Option<SocketAddr> {
        lock(&self.shared.current_end_point).clone()
    }

    fn protocol_types(&self) -> ProtocolTypes {
        ProtocolTypes::UDP
    }

    async fn connect(&self, end_point: SocketAddr, _timeout: Duration) -> Result<(), SteamError> {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let local_ip = socket.local_addr().ok().map(|addr| addr.ip());

        {
            let mut data = self.shared.state.lock().await;
            *data = UdpStateData::new();
        }

        *lock(&self.shared.current_end_point) = Some(end_point);
        *lock(&self.shared.local_ip) = local_ip;
        *lock(&self.shared.socket) = Some(socket);

        self.shared.running.store(true, Ordering::SeqCst);
        self.shared.disconnect_notified.store(false, Ordering::SeqCst);

        let shared = self.shared.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            udp_net_loop(shared, events, end_point).await;
        });

        Ok(())
    }

    async fn disconnect(&self, user_initiated: bool) {
        if !self.shared.running.load(Ordering::SeqCst) {
            return;
        }

        let should_send_disconnect = {
            let mut data = self.shared.state.lock().await;
            if data.state == UdpState::Disconnected {
                false
            } else {
                data.state = UdpState::Disconnecting;
                true
            }
        };

        if should_send_disconnect {
            send_sequenced(
                &self.shared,
                vec![UdpPacket::new(EUdpPacketType::Disconnect)],
            )
            .await;
        }

        SOURCE_CONN_ID.fetch_add(256, Ordering::SeqCst);

        if !self.shared.disconnect_notified.swap(true, Ordering::SeqCst) {
            self.events
                .emit_disconnected(DisconnectedEventArgs::new(user_initiated));
        }
    }

    async fn send(&self, data: Bytes) -> Result<(), SteamError> {
        {
            let state = self.shared.state.lock().await;
            if state.state != UdpState::Connected {
                return Ok(());
            }
        }

        let total = data.len();
        let packet_count = total / MAX_PAYLOAD + 1;
        let mut packets = Vec::with_capacity(packet_count);

        for index in 0..packet_count {
            let offset = index * MAX_PAYLOAD;
            let length = std::cmp::min(MAX_PAYLOAD, total.saturating_sub(offset));

            let mut packet = UdpPacket::new(EUdpPacketType::Data);
            if packet.set_payload_slice(&data, offset, length).is_err() {
                continue;
            }
            packet.header.msg_size = total as u32;
            packets.push(packet);
        }

        send_sequenced(&self.shared, packets).await;

        Ok(())
    }

    fn local_ip(&self) -> Option<IpAddr> {
        lock(&self.shared.local_ip).clone()
    }
}

impl ConnectionEvents for UdpConnection {
    fn events(&self) -> mpsc::UnboundedReceiver<ConnectionEvent> {
        self.events.subscribe_events()
    }

    fn net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs> {
        self.events.subscribe_net_msg_received()
    }

    fn connected(&self) -> broadcast::Receiver<()> {
        self.events.subscribe_connected()
    }

    fn disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs> {
        self.events.subscribe_disconnected()
    }
}

async fn udp_net_loop(shared: Arc<UdpShared>, events: ConnectionEventSenders, end_point: SocketAddr) {
    {
        let mut data = shared.state.lock().await;
        data.state = UdpState::ChallengeReqSent;
        data.time_out = Instant::now() + TIMEOUT_DELAY;
        data.next_resend = Instant::now() + RESEND_DELAY;
    }

    send_packet(&shared, UdpPacket::new(EUdpPacketType::ChallengeReq)).await;

    let mut buffer = vec![0u8; RECEIVE_BUFFER_SIZE];
    let mut user_requested_disconnect = false;

    loop {
        {
            let data = shared.state.lock().await;
            if data.state == UdpState::Disconnected {
                break;
            }
        }

        let mut socket_failed = false;
        let mut received_any = false;

        match recv_with_timeout(&shared, &mut buffer, OUTER_POLL).await {
            RecvOutcome::Timeout => {
                let expired = {
                    let data = shared.state.lock().await;
                    Instant::now() > data.time_out
                };
                if expired {
                    tracing::debug!("UdpConnection: Connection timed out");
                    let mut data = shared.state.lock().await;
                    data.state = UdpState::Disconnected;
                    break;
                }
            }
            RecvOutcome::SocketError => {
                socket_failed = true;
            }
            RecvOutcome::Packet(length, from) => {
                received_any = true;
                handle_received(&shared, &events, &buffer[..length], from, &end_point).await;
            }
        }

        if received_any {
            loop {
                match recv_with_timeout(&shared, &mut buffer, INNER_POLL).await {
                    RecvOutcome::Timeout => break,
                    RecvOutcome::SocketError => {
                        socket_failed = true;
                        break;
                    }
                    RecvOutcome::Packet(length, from) => {
                        handle_received(&shared, &events, &buffer[..length], from, &end_point).await;
                    }
                }
            }
        }

        if socket_failed {
            tracing::debug!("UdpConnection: Critical socket failure");
            let mut data = shared.state.lock().await;
            data.state = UdpState::Disconnected;
            break;
        }

        {
            let should_send = {
                let data = shared.state.lock().await;
                data.state != UdpState::Disconnected
            };
            if should_send {
                send_pending_messages(&shared).await;
            }
        }

        {
            let needs_ack = {
                let data = shared.state.lock().await;
                data.in_seq != data.in_seq_acked
            };
            if needs_ack {
                send_packet(&shared, UdpPacket::new(EUdpPacketType::Datagram)).await;
            }
        }

        {
            let graceful = {
                let data = shared.state.lock().await;
                data.state == UdpState::Disconnecting && data.out_packets.is_empty()
            };
            if graceful {
                tracing::debug!("UdpConnection: Graceful disconnect completed");
                let mut data = shared.state.lock().await;
                data.state = UdpState::Disconnected;
                user_requested_disconnect = true;
                break;
            }
        }
    }

    shared.running.store(false, Ordering::SeqCst);
    {
        let mut socket = lock(&shared.socket);
        *socket = None;
    }

    tracing::debug!("UdpConnection: Calling OnDisconnected");
    if !shared.disconnect_notified.swap(true, Ordering::SeqCst) {
        events.emit_disconnected(DisconnectedEventArgs::new(user_requested_disconnect));
    }
}

enum RecvOutcome {
    Timeout,
    SocketError,
    Packet(usize, SocketAddr),
}

async fn recv_with_timeout(
    shared: &Arc<UdpShared>,
    buffer: &mut [u8],
    timeout: Duration,
) -> RecvOutcome {
    let socket = lock(&shared.socket).clone();

    let Some(socket) = socket else {
        return RecvOutcome::SocketError;
    };

    match tokio::time::timeout(timeout, socket.recv_from(buffer)).await {
        Err(_) => RecvOutcome::Timeout,
        Ok(Err(_)) => RecvOutcome::SocketError,
        Ok(Ok((length, from))) => RecvOutcome::Packet(length, from),
    }
}

async fn handle_received(
    shared: &Arc<UdpShared>,
    events: &ConnectionEventSenders,
    data: &[u8],
    from: SocketAddr,
    expected: &SocketAddr,
) {
    if from != *expected {
        return;
    }

    {
        let mut state = shared.state.lock().await;
        state.time_out = Instant::now() + TIMEOUT_DELAY;
    }

    let packet = UdpPacket::from_bytes(data);
    receive_packet(shared, events, packet, expected).await;
}

async fn receive_packet(
    shared: &Arc<UdpShared>,
    events: &ConnectionEventSenders,
    packet: UdpPacket,
    end_point: &SocketAddr,
) {
    if !packet.is_valid() {
        return;
    }

    {
        let state = shared.state.lock().await;
        if state.remote_conn_id > 0 && packet.header.source_conn_id != state.remote_conn_id {
            return;
        }
    }

    let in_seq = {
        let state = shared.state.lock().await;
        state.in_seq
    };

    if packet.header.packet_type == EUdpPacketType::Data && packet.header.seq_this < in_seq {
        send_packet(shared, UdpPacket::new(EUdpPacketType::Datagram)).await;
        return;
    }

    {
        let mut state = shared.state.lock().await;
        if state.out_seq_acked < packet.header.seq_ack {
            state.out_seq_acked = packet.header.seq_ack;

            if state.out_seq_sent < state.out_seq_acked {
                state.out_seq_sent = state.out_seq_acked;
            }

            let acked = state.out_seq_acked;
            state.out_packets.retain(|p| p.header.seq_this > acked);
            state.next_resend = Instant::now() + RESEND_DELAY;
        }
    }

    {
        let mut state = shared.state.lock().await;
        if packet.header.seq_this == state.in_seq + 1 {
            loop {
                state.in_seq += 1;
                if !state.in_packets.contains_key(&(state.in_seq + 1)) {
                    break;
                }
            }
        }
    }

    match packet.header.packet_type {
        EUdpPacketType::Challenge => receive_challenge(shared, packet).await,
        EUdpPacketType::Accept => receive_accept(shared, events, packet).await,
        EUdpPacketType::Data => receive_data(shared, events, packet, end_point).await,
        EUdpPacketType::Disconnect => {
            tracing::debug!("UdpConnection: Disconnected by server");
            let mut state = shared.state.lock().await;
            state.state = UdpState::Disconnected;
        }
        EUdpPacketType::Datagram => {}
        other => {
            tracing::debug!("UdpConnection: Received unexpected packet type {other:?}");
        }
    }
}

async fn receive_challenge(shared: &Arc<UdpShared>, packet: UdpPacket) {
    {
        let mut state = shared.state.lock().await;
        if state.state != UdpState::ChallengeReqSent {
            return;
        }
        state.state = UdpState::ConnectSent;
    }

    let Ok(challenge) = ChallengeData::deserialize(packet.payload()) else {
        return;
    };

    let connect = ConnectData {
        challenge_value: challenge.challenge_value ^ ConnectData::CHALLENGE_MASK,
    };

    let Ok(connect_packet) = UdpPacket::with_payload(EUdpPacketType::Connect, connect.serialize())
    else {
        return;
    };

    send_sequenced(shared, vec![connect_packet]).await;

    let mut state = shared.state.lock().await;
    state.in_seq_handled = packet.header.seq_this;
}

async fn receive_accept(
    shared: &Arc<UdpShared>,
    events: &ConnectionEventSenders,
    packet: UdpPacket,
) {
    {
        let mut state = shared.state.lock().await;
        if state.state != UdpState::ConnectSent {
            return;
        }
        state.state = UdpState::Connected;
        state.remote_conn_id = packet.header.source_conn_id;
        state.in_seq_handled = packet.header.seq_this;
    }

    tracing::debug!("UdpConnection: Connection established");
    events.emit_connected();
}

async fn receive_data(
    shared: &Arc<UdpShared>,
    events: &ConnectionEventSenders,
    packet: UdpPacket,
    end_point: &SocketAddr,
) {
    let seq_this = packet.header.seq_this;

    {
        let mut state = shared.state.lock().await;

        if state.state != UdpState::Connected && state.state != UdpState::Disconnecting {
            return;
        }

        if seq_this <= state.in_seq_handled {
            return;
        }

        if state.in_packets.contains_key(&seq_this) {
            return;
        }

        state.in_packets.insert(seq_this, packet);
    }

    while dispatch_message(shared, events, end_point).await {}
}

async fn dispatch_message(
    shared: &Arc<UdpShared>,
    events: &ConnectionEventSenders,
    end_point: &SocketAddr,
) -> bool {
    let num_packets = {
        let state = shared.state.lock().await;
        ready_message_parts(&state)
    };

    if num_packets == 0 {
        return false;
    }

    let mut payload = Vec::new();
    {
        let mut state = shared.state.lock().await;
        for _ in 0..num_packets {
            state.in_seq_handled += 1;
            let seq = state.in_seq_handled;
            if let Some(packet) = state.in_packets.remove(&seq) {
                payload.extend_from_slice(packet.payload());
            }
        }
    }

    events.emit_net_msg_received(NetMsgEventArgs {
        data: payload,
        end_point: *end_point,
    });

    true
}

fn ready_message_parts(state: &UdpStateData) -> u32 {
    let Some(first) = state.in_packets.get(&(state.in_seq_handled + 1)) else {
        return 0;
    };

    for index in 1..first.header.packets_in_msg {
        if !state.in_packets.contains_key(&(state.in_seq_handled + 1 + index)) {
            return 0;
        }
    }

    first.header.packets_in_msg
}

async fn send_packet(shared: &Arc<UdpShared>, mut packet: UdpPacket) {
    let end_point = lock(&shared.current_end_point).clone();
    let Some(end_point) = end_point else {
        return;
    };

    let socket = lock(&shared.socket).clone();
    let Some(socket) = socket else {
        return;
    };

    {
        let mut state = shared.state.lock().await;
        packet.header.source_conn_id = SOURCE_CONN_ID.load(Ordering::SeqCst);
        packet.header.dest_conn_id = state.remote_conn_id;
        packet.header.seq_ack = state.in_seq;
        state.in_seq_acked = state.in_seq;
    }

    let data = packet.get_data();

    if socket.send_to(&data, end_point).await.is_err() {
        tracing::debug!("UdpConnection: Critical socket failure");
        let mut state = shared.state.lock().await;
        state.state = UdpState::Disconnected;
        return;
    }

    let mut state = shared.state.lock().await;

    if state.out_seq_sent == state.out_seq_acked {
        state.next_resend = Instant::now() + RESEND_DELAY;
    }

    if packet.header.seq_this > 0 {
        state.out_seq_sent = state.out_seq_sent.max(packet.header.seq_this);
    }
}

async fn send_sequenced(shared: &Arc<UdpShared>, mut packets: Vec<UdpPacket>) {
    let mut state = shared.state.lock().await;
    let msg_start = state.out_seq;
    let packets_in_msg = packets.len() as u32;

    for mut packet in packets.drain(..) {
        packet.header.seq_this = state.out_seq;
        packet.header.msg_start_seq = msg_start;
        packet.header.packets_in_msg = packets_in_msg;
        state.out_packets.push(packet);
        state.out_seq += 1;
    }
}

async fn send_pending_messages(shared: &Arc<UdpShared>) {
    let to_send: Vec<UdpPacket> = {
        let mut state = shared.state.lock().await;

        if Instant::now() > state.next_resend && state.out_seq_sent > state.out_seq_acked {
            if state.state == UdpState::Disconnecting {
                state.out_packets.clear();
            }

            tracing::debug!("UdpConnection: Sequenced packet resend required");

            let count = RESEND_COUNT.min(state.out_packets.len());
            let packets = state.out_packets[..count].to_vec();
            state.next_resend = Instant::now() + RESEND_DELAY;
            packets
        } else if state.out_seq_sent < state.out_seq_acked + AHEAD_COUNT {
            let start = state.out_seq_sent.saturating_sub(state.out_seq_acked) as usize;
            let end = (AHEAD_COUNT as usize).min(state.out_packets.len());
            let mut packets = Vec::new();
            let mut index = start;
            while index < end {
                packets.push(state.out_packets[index].clone());
                index += 1;
            }
            packets
        } else {
            Vec::new()
        }
    };

    for packet in to_send {
        send_packet(shared, packet).await;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
