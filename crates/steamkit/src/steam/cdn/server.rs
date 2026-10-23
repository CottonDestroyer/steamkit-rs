//! A Steam content (Steampipe) server.

use std::fmt;
use std::net::SocketAddr;

/// The protocol used to connect to a content server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionProtocol {
    /// The server doesn't advertise HTTPS, so connect over HTTP.
    #[default]
    Http,
    /// The server supports HTTPS.
    Https,
}

/// A Steam content (Steampipe) server.
#[derive(Clone, Debug, Default)]
pub struct Server {
    /// The protocol used to connect to this server.
    pub(crate) protocol: ConnectionProtocol,
    /// The hostname of the server.
    pub(crate) host: Option<String>,
    /// The virtual hostname of the server.
    pub(crate) v_host: Option<String>,
    /// The port of the server.
    pub(crate) port: i32,
    /// The type of the server (`type` is a keyword, hence the name).
    pub(crate) server_type: Option<String>,
    /// The SourceID this server belongs to.
    pub(crate) source_id: i32,
    /// The CellID this server belongs to.
    pub(crate) cell_id: u32,
    /// The load value associated with this server.
    pub(crate) load: i32,
    /// The weighted load.
    pub(crate) weighted_load: f32,
    /// The number of entries this server is worth.
    pub(crate) num_entries: i32,
    /// Whether this server is for Steam China only.
    pub(crate) steam_china_only: bool,
    /// Whether this server is a download proxy.
    pub(crate) use_as_proxy: bool,
    /// The transformation template applied to request paths.
    pub(crate) proxy_request_path_template: Option<String>,
    /// The list of app ids this server can be used with.
    pub(crate) allowed_app_ids: Vec<u32>,
    /// The list of bypass proxies.
    pub(crate) bypass_proxies_of_type: Vec<String>,
}

impl Server {
    /// Creates a server from a socket address. Port 443 means HTTPS, anything
    /// else HTTP.
    pub fn from_socket_addr(end_point: SocketAddr) -> Self {
        let host = end_point.ip().to_string();

        Server {
            protocol: if end_point.port() == 443 {
                ConnectionProtocol::Https
            } else {
                ConnectionProtocol::Http
            },
            host: Some(host.clone()),
            v_host: Some(host),
            port: i32::from(end_point.port()),
            ..Server::default()
        }
    }

    /// Creates a server from a host name and port. Port 443 means HTTPS,
    /// anything else HTTP.
    pub fn from_dns_end_point(host: &str, port: u16) -> Self {
        Server {
            protocol: if port == 443 {
                ConnectionProtocol::Https
            } else {
                ConnectionProtocol::Http
            },
            host: Some(host.to_string()),
            v_host: Some(host.to_string()),
            port: i32::from(port),
            ..Server::default()
        }
    }

    /// The connection protocol the server supports.
    pub fn protocol(&self) -> ConnectionProtocol {
        self.protocol
    }

    /// The host name of the server.
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// The virtual host name of the server.
    pub fn v_host(&self) -> Option<&str> {
        self.v_host.as_deref()
    }

    /// The port of the server.
    pub fn port(&self) -> i32 {
        self.port
    }

    /// The type of the server.
    pub fn server_type(&self) -> Option<&str> {
        self.server_type.as_deref()
    }

    /// The SourceID this server belongs to.
    pub fn source_id(&self) -> i32 {
        self.source_id
    }

    /// The CellID this server belongs to.
    pub fn cell_id(&self) -> u32 {
        self.cell_id
    }

    /// The load value of this server.
    pub fn load(&self) -> i32 {
        self.load
    }

    /// The weighted load.
    pub fn weighted_load(&self) -> f32 {
        self.weighted_load
    }

    /// The number of entries this server is worth.
    pub fn num_entries(&self) -> i32 {
        self.num_entries
    }

    /// Whether this server is for Steam China only.
    pub fn steam_china_only(&self) -> bool {
        self.steam_china_only
    }

    /// Whether this server is a download proxy.
    pub fn use_as_proxy(&self) -> bool {
        self.use_as_proxy
    }

    /// The template applied to request paths.
    pub fn proxy_request_path_template(&self) -> Option<&str> {
        self.proxy_request_path_template.as_deref()
    }

    /// The app ids this server can be used for.
    pub fn allowed_app_ids(&self) -> &[u32] {
        &self.allowed_app_ids
    }

    /// The list of bypass proxies.
    pub fn bypass_proxies_of_type(&self) -> &[String] {
        &self.bypass_proxies_of_type
    }
}

impl From<SocketAddr> for Server {
    /// Same as [`Server::from_socket_addr`].
    fn from(end_point: SocketAddr) -> Self {
        Server::from_socket_addr(end_point)
    }
}

impl fmt::Display for Server {
    /// Formats the server as `host:port (type)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} ({})",
            self.host.as_deref().unwrap_or_default(),
            self.port,
            self.server_type.as_deref().unwrap_or_default()
        )
    }
}
