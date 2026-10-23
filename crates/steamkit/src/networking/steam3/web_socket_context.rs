//! One WebSocket connection attempt: URI construction, message framing, the
//! read loop and the send path.
//!
//! The socket itself is behind the [`WebSocketSocket`] and [`WebSocketFactory`]
//! traits. [`TungsteniteWebSocketSocket`] is the real implementation; tests can
//! plug in their own.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Sink, SinkExt, Stream, StreamExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::Error as WebSocketError;
use tokio_tungstenite::tungstenite::http::header::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::error::SteamError;

use super::disconnected_event_args::DisconnectedEventArgs;
use super::i_connection::ConnectionEventSenders;
use super::net_msg_event_args::NetMsgEventArgs;

/// The path of the Steam websocket endpoint.
pub const WEBSOCKET_PATH: &str = "/cmsocket/";

/// The scheme of the Steam websocket endpoint.
pub const WEBSOCKET_SCHEME: &str = "wss";

/// The name of the header that carries the `User-Agent` of the upgrade request.
const USER_AGENT: HeaderName = HeaderName::from_static("user-agent");

/// The type of a websocket message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketMessageType {
    /// A UTF-8 text message.
    Text,
    /// A binary message.
    Binary,
    /// The close frame.
    Close,
}

/// The outcome of a single websocket receive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSocketReceive {
    /// The number of bytes carried by the message.
    pub data: Bytes,
    /// Whether this was the last chunk of the message.
    pub end_of_message: bool,
    /// The type of the message.
    pub message_type: WebSocketMessageType,
}

/// A WebSocket socket.
#[async_trait]
pub trait WebSocketSocket: Send + Sync {
    /// Sends a binary message.
    async fn send_binary(&self, data: Bytes) -> Result<(), SteamError>;

    /// Receives the next chunk of a message.
    async fn receive(&self) -> Result<WebSocketReceive, SteamError>;

    /// Closes the socket.
    async fn close(&self) -> Result<(), SteamError>;

    /// Whether the socket is still open.
    fn is_open(&self) -> bool;
}

/// Creates websocket sockets.
#[async_trait]
pub trait WebSocketFactory: Send + Sync {
    /// Connects to `uri`, giving up after `timeout`.
    async fn connect(
        &self,
        uri: &str,
        timeout: Duration,
    ) -> Result<Arc<dyn WebSocketSocket>, SteamError>;
}

/// A single WebSocket connection attempt, with its own read loop.
#[derive(Debug)]
pub struct WebSocketContext {
    end_point: SocketAddr,
    uri: String,
    events: ConnectionEventSenders,
    cancel: CancellationToken,
    disconnect_notified: AtomicBool,
}

impl WebSocketContext {
    /// Creates a context for the given endpoint.
    pub fn new(end_point: SocketAddr, events: ConnectionEventSenders) -> Self {
        Self::new_with_uri(end_point, construct_uri(end_point), events)
    }

    /// Creates a context for the given endpoint and URI. The URI is passed in
    /// because it needs the host name, which `end_point` doesn't have.
    pub fn new_with_uri(
        end_point: SocketAddr,
        uri: String,
        events: ConnectionEventSenders,
    ) -> Self {
        Self {
            end_point,
            uri,
            events,
            cancel: CancellationToken::new(),
            disconnect_notified: AtomicBool::new(false),
        }
    }

    /// The endpoint this context is connected to.
    pub fn end_point(&self) -> SocketAddr {
        self.end_point
    }

    /// The websocket URI this context connects to.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Whether the context has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Cancels the run loop.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Raises the `Disconnected` event exactly once.
    pub fn notify_disconnected(&self, user_initiated: bool) {
        if !self.disconnect_notified.swap(true, Ordering::SeqCst) {
            self.events
                .emit_disconnected(DisconnectedEventArgs::new(user_initiated));
        }
    }

    /// Starts the read loop.
    pub fn start(self: Arc<Self>, socket: Arc<dyn WebSocketSocket>) {
        tokio::spawn(async move {
            self.run_loop(socket).await;
        });
    }

    async fn run_loop(self: Arc<Self>, socket: Arc<dyn WebSocketSocket>) {
        self.events.emit_connected();

        loop {
            if self.cancel.is_cancelled() {
                break;
            }

            let received = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => break,
                result = socket.receive() => result,
            };

            let Ok(received) = received else {
                break;
            };

            match received.message_type {
                WebSocketMessageType::Binary => {
                    let mut message = received.data;

                    while !received.end_of_message {
                        let Ok(next) = socket.receive().await else {
                            self.notify_disconnected(false);
                            return;
                        };

                        if next.message_type != WebSocketMessageType::Binary {
                            break;
                        }

                        message = extend(message, next.data);

                        if next.end_of_message {
                            break;
                        }
                    }

                    if !message.is_empty() {
                        self.events.emit_net_msg_received(NetMsgEventArgs {
                            data: message.to_vec(),
                            end_point: self.end_point,
                        });
                    }
                }
                WebSocketMessageType::Text => {
                    tracing::debug!(
                        "WebSocketContext: Received websocket text message: \"{}\"",
                        String::from_utf8_lossy(&received.data)
                    );
                }
                WebSocketMessageType::Close => {
                    self.notify_disconnected(false);
                    return;
                }
            }
        }

        let _ = socket.close().await;
        self.notify_disconnected(false);
    }
}

/// Appends the payload of a message chunk to the message that is being read.
fn extend(message: Bytes, chunk: Bytes) -> Bytes {
    let mut buffer = Vec::with_capacity(message.len() + chunk.len());
    buffer.extend_from_slice(&message);
    buffer.extend_from_slice(&chunk);
    buffer.into()
}

/// Builds the WebSocket URI for an endpoint.
pub fn construct_uri(end_point: SocketAddr) -> String {
    construct_uri_for_host(&format_host(end_point.ip()), end_point.port())
}

/// Builds the WebSocket URI for a host name and port.
pub fn construct_uri_for_host(host: &str, port: u16) -> String {
    format!("{WEBSOCKET_SCHEME}://{host}:{port}{WEBSOCKET_PATH}")
}

fn format_host(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

type TungsteniteSink =
    Box<dyn Sink<Message, Error = WebSocketError> + Unpin + Send>;

type TungsteniteStream =
    Box<dyn Stream<Item = Result<Message, WebSocketError>> + Unpin + Send>;

/// A WebSocket socket backed by `tokio-tungstenite`.
pub struct TungsteniteWebSocketSocket {
    sink: tokio::sync::Mutex<Option<TungsteniteSink>>,
    stream: tokio::sync::Mutex<Option<TungsteniteStream>>,
    open: AtomicBool,
}

impl std::fmt::Debug for TungsteniteWebSocketSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TungsteniteWebSocketSocket")
            .field("open", &self.open.load(Ordering::SeqCst))
            .finish()
    }
}

impl TungsteniteWebSocketSocket {
    fn new(socket: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) -> Self {
        let (sink, stream) = socket.split();

        Self {
            sink: tokio::sync::Mutex::new(Some(Box::new(sink))),
            stream: tokio::sync::Mutex::new(Some(Box::new(stream))),
            open: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl WebSocketSocket for TungsteniteWebSocketSocket {
    async fn send_binary(&self, data: Bytes) -> Result<(), SteamError> {
        let mut guard = self.sink.lock().await;

        let Some(sink) = guard.as_mut() else {
            return Err(SteamError::Connection("websocket is closed".to_string()));
        };

        sink.send(Message::Binary(data.to_vec().into()))
            .await
            .map_err(|err| SteamError::Connection(format!("websocket send failed: {err}")))
    }

    async fn receive(&self) -> Result<WebSocketReceive, SteamError> {
        let mut guard = self.stream.lock().await;

        let Some(stream) = guard.as_mut() else {
            return Err(SteamError::Connection("websocket is closed".to_string()));
        };

        match stream.next().await {
            Some(Ok(Message::Binary(payload))) => Ok(WebSocketReceive {
                data: payload.into(),
                end_of_message: true,
                message_type: WebSocketMessageType::Binary,
            }),
            Some(Ok(Message::Text(text))) => Ok(WebSocketReceive {
                data: text.into_bytes().into(),
                end_of_message: true,
                message_type: WebSocketMessageType::Text,
            }),
            Some(Ok(Message::Close(_))) => {
                self.open.store(false, Ordering::SeqCst);

                Ok(WebSocketReceive {
                    data: Bytes::new(),
                    end_of_message: true,
                    message_type: WebSocketMessageType::Close,
                })
            }
            Some(Ok(_)) => Ok(WebSocketReceive {
                data: Bytes::new(),
                end_of_message: true,
                message_type: WebSocketMessageType::Binary,
            }),
            Some(Err(err)) => Err(SteamError::Connection(format!(
                "websocket read failed: {err}"
            ))),
            None => {
                self.open.store(false, Ordering::SeqCst);

                Err(SteamError::Connection(
                    "websocket stream ended".to_string(),
                ))
            }
        }
    }

    async fn close(&self) -> Result<(), SteamError> {
        self.open.store(false, Ordering::SeqCst);

        let sink = self.sink.lock().await.take();
        if let Some(mut sink) = sink {
            let _ = sink.close().await;
        }

        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }
}

/// Creates `tokio-tungstenite` backed websocket sockets.
#[derive(Clone, Debug, Default)]
pub struct TungsteniteWebSocketFactory {
    user_agent: Option<String>,
}

impl TungsteniteWebSocketFactory {
    /// Creates a factory that sends no `User-Agent` header.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a factory that sends the given `User-Agent` with the upgrade
    /// request.
    pub fn with_user_agent(user_agent: impl Into<String>) -> Self {
        Self {
            user_agent: Some(user_agent.into()),
        }
    }
}

#[async_trait]
impl WebSocketFactory for TungsteniteWebSocketFactory {
    async fn connect(
        &self,
        uri: &str,
        timeout: Duration,
    ) -> Result<Arc<dyn WebSocketSocket>, SteamError> {
        let request = match &self.user_agent {
            Some(user_agent) => {
                let mut request: Request<()> = uri
                    .into_client_request()
                    .map_err(|error| SteamError::Connection(format!("invalid websocket URI: {error}")))?;

                request.headers_mut().insert(
                    USER_AGENT,
                    HeaderValue::from_str(user_agent).map_err(|error| {
                        SteamError::Connection(format!("invalid user agent: {error}"))
                    })?,
                );

                request
            }
            None => uri
                .into_client_request()
                .map_err(|error| SteamError::Connection(format!("invalid websocket URI: {error}")))?,
        };

        let connect = tokio_tungstenite::connect_async(request);

        let (socket, _response) = match tokio::time::timeout(timeout, connect).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(err)) => {
                return Err(SteamError::Connection(format!(
                    "websocket handshake failed: {err}"
                )))
            }
            Err(_) => return Err(SteamError::Timeout(timeout)),
        };

        Ok(Arc::new(TungsteniteWebSocketSocket::new(socket)))
    }
}
