//! What's needed to connect to a CM server: an endpoint and the protocols it
//! supports.
//!
//! Two records are equal only if both the endpoint and the protocols match.
//!
//! An endpoint is either an IP address and port, or a host name and port. For
//! host names the name is kept, since a WebSocket connection needs it for TLS,
//! along with the address it resolved to. Connections take a `SocketAddr`, so
//! host names are resolved when the record is created; that's why the
//! constructors taking a host name are `async`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, SocketAddr};

use crate::error::SteamError;
use crate::networking::steam3::protocol_type::ProtocolTypes;

/// The endpoint of a server.
#[derive(Clone, Debug)]
pub enum EndPoint {
    /// An IP address and port.
    Ip(SocketAddr),
    /// A host name and port, plus the address it resolved to.
    Dns {
        /// The host name of the endpoint.
        host: String,
        /// The port of the endpoint.
        port: u16,
        /// The address the host name resolved to.
        address: SocketAddr,
    },
}

impl EndPoint {
    /// Creates a host name endpoint from the name, the port and the address it
    /// resolved to.
    pub fn dns(host: impl Into<String>, port: u16, address: SocketAddr) -> Self {
        EndPoint::Dns {
            host: host.into(),
            port,
            address,
        }
    }

    /// The socket address of the endpoint.
    pub fn socket_addr(&self) -> SocketAddr {
        match self {
            EndPoint::Ip(address) => *address,
            EndPoint::Dns { address, .. } => *address,
        }
    }

    /// The host: an IP address or a host name.
    pub fn host(&self) -> String {
        match self {
            EndPoint::Ip(address) => address.ip().to_string(),
            EndPoint::Dns { host, .. } => host.clone(),
        }
    }

    /// The port.
    pub fn port(&self) -> u16 {
        match self {
            EndPoint::Ip(address) => address.port(),
            EndPoint::Dns { port, .. } => *port,
        }
    }
}

impl PartialEq for EndPoint {
    /// Equal if both are the same kind and describe the same address. Host
    /// names are compared case-insensitively.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (EndPoint::Ip(left), EndPoint::Ip(right)) => left == right,
            (
                EndPoint::Dns {
                    host: left_host,
                    port: left_port,
                    ..
                },
                EndPoint::Dns {
                    host: right_host,
                    port: right_port,
                    ..
                },
            ) => left_port == right_port && left_host.eq_ignore_ascii_case(right_host),
            _ => false,
        }
    }
}

impl Eq for EndPoint {}

impl Hash for EndPoint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            EndPoint::Ip(address) => {
                0u8.hash(state);
                address.hash(state);
            }
            EndPoint::Dns { host, port, .. } => {
                1u8.hash(state);
                port.hash(state);
                host.to_ascii_lowercase().hash(state);
            }
        }
    }
}

impl fmt::Display for EndPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EndPoint::Ip(address) => write!(f, "{address}"),
            EndPoint::Dns { host, port, .. } => write!(f, "{host}:{port}"),
        }
    }
}

impl From<SocketAddr> for EndPoint {
    fn from(address: SocketAddr) -> Self {
        EndPoint::Ip(address)
    }
}

/// What's needed to connect to a CM server.
#[derive(Clone, Debug)]
pub struct ServerRecord {
    end_point: EndPoint,
    protocol_types: ProtocolTypes,
}

impl ServerRecord {
    /// Creates a server record for the given endpoint and protocol types.
    pub fn new(end_point: EndPoint, protocol_types: ProtocolTypes) -> Self {
        ServerRecord {
            end_point,
            protocol_types,
        }
    }

    /// The address to connect to. For the host name, if there is one, use
    /// [`ServerRecord::endpoint`].
    pub fn end_point(&self) -> SocketAddr {
        self.end_point.socket_addr()
    }

    /// The full endpoint, including the host name if there is one.
    pub fn endpoint(&self) -> &EndPoint {
        &self.end_point
    }

    /// Whether the endpoint is a host name rather than an IP address.
    pub fn is_dns(&self) -> bool {
        matches!(self.end_point, EndPoint::Dns { .. })
    }

    /// The protocols that can be used to talk to this server.
    pub fn protocol_types(&self) -> ProtocolTypes {
        self.protocol_types
    }

    /// The host of the endpoint: an IP address or a host name.
    pub fn get_host(&self) -> String {
        self.end_point.host()
    }

    /// The port of the endpoint.
    pub fn get_port(&self) -> u16 {
        self.end_point.port()
    }

    /// Creates a record for the given host and port. A host name is resolved
    /// to an address.
    pub async fn create_server(
        host: &str,
        port: u16,
        protocol_types: ProtocolTypes,
    ) -> Result<Self, SteamError> {
        match host.parse::<IpAddr>() {
            Ok(address) => Ok(ServerRecord::new(
                EndPoint::Ip(SocketAddr::new(address, port)),
                protocol_types,
            )),
            Err(_) => {
                let address = resolve_host(host, port).await?;
                Ok(ServerRecord::new(
                    EndPoint::dns(host, port, address),
                    protocol_types,
                ))
            }
        }
    }

    /// Creates a TCP/UDP server record for an IP endpoint.
    pub fn create_socket_server(end_point: SocketAddr) -> Self {
        ServerRecord::new(EndPoint::Ip(end_point), tcp_udp_protocol_types())
    }

    /// Creates a TCP/UDP server record from an address string. Returns `None`
    /// if it can't be parsed.
    pub fn try_create_socket_server(address: &str) -> Option<Self> {
        let end_point = address.parse::<SocketAddr>().ok()?;
        if end_point.port() == 0 {
            return None;
        }

        Some(ServerRecord::new(
            EndPoint::Ip(end_point),
            tcp_udp_protocol_types(),
        ))
    }

    /// Creates a TCP/UDP server record from a "host:port" address. The port
    /// defaults to 443.
    pub async fn create_dns_socket_server(address: &str) -> Result<Self, SteamError> {
        ServerRecord::create_server_from_dns(address, tcp_udp_protocol_types()).await
    }

    /// Creates a WebSocket server record from a "host:port" address. The port
    /// defaults to 443.
    pub async fn create_websocket_server(address: &str) -> Result<Self, SteamError> {
        ServerRecord::create_server_from_dns(address, ProtocolTypes::WEB_SOCKET).await
    }

    /// Creates a server record from a "host:port" address.
    async fn create_server_from_dns(
        address: &str,
        protocol_types: ProtocolTypes,
    ) -> Result<Self, SteamError> {
        const DEFAULT_PORT: u16 = 443;

        let (host, port) = match address.find(':') {
            Some(index) => {
                let host = &address[..index];
                let port = &address[index + 1..];
                let port = port.parse::<u16>().map_err(|_| {
                    SteamError::InvalidArgument(
                        "Port number must be a valid integer value.".to_string(),
                    )
                })?;
                (host, port)
            }
            None => (address, DEFAULT_PORT),
        };

        let resolved = resolve_host(host, port).await?;

        Ok(ServerRecord::new(
            EndPoint::dns(host, port, resolved),
            protocol_types,
        ))
    }
}

impl PartialEq for ServerRecord {
    /// Equal when both the endpoints and the protocol types match.
    fn eq(&self, other: &Self) -> bool {
        self.end_point == other.end_point && self.protocol_types == other.protocol_types
    }
}

impl Eq for ServerRecord {}

impl Hash for ServerRecord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.end_point.hash(state);
        self.protocol_types.hash(state);
    }
}

impl fmt::Display for ServerRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.end_point, self.protocol_types)
    }
}

/// Resolves a host name to a socket address.
async fn resolve_host(host: &str, port: u16) -> Result<SocketAddr, SteamError> {
    let mut addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| SteamError::Connection(format!("Failed to resolve {host}: {error}")))?;

    addresses.next().ok_or_else(|| {
        SteamError::Connection(format!("Failed to resolve {host}: no addresses returned"))
    })
}

/// `Tcp | Udp`.
pub(crate) fn tcp_udp_protocol_types() -> ProtocolTypes {
    ProtocolTypes(ProtocolTypes::TCP.raw() | ProtocolTypes::UDP.raw())
}
