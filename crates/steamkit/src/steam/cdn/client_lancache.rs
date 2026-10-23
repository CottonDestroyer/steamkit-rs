//! Lancache support for the CDN [`Client`].

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::header::{HeaderMap, HeaderValue, HOST, USER_AGENT};
use reqwest::Url;

use crate::error::SteamError;

use super::client::Client;
use super::server::Server;

/// Whether to download from a Lancache instance on the LAN instead of Steam's CDN.
static USE_LANCACHE_SERVER: AtomicBool = AtomicBool::new(false);

/// The domain that is used to detect a Lancache instance.
const TRIGGER_DOMAIN: &str = "lancache.steamcontent.com";

/// The user agent of the Steam client. Lancache needs it to recognize and cache
/// Valve's CDN content.
const LANCACHE_USER_AGENT: &str = "Valve/Steam HTTP Client 1.0";

impl Client {
    /// Whether downloads go through a Lancache instance on the LAN instead of
    /// Steam's CDN.
    pub fn use_lancache_server() -> bool {
        USE_LANCACHE_SERVER.load(Ordering::Relaxed)
    }

    /// Tries to find a Lancache on the local network, through its poisoned DNS
    /// entry for `lancache.steamcontent.com`. If one is found, all downloads go
    /// through it instead of Steam's CDN.
    ///
    /// Based on <https://github.com/tpill90/lancache-prefill-common/blob/main/dotnet/LancacheIpResolver.cs>.
    pub async fn detect_lancache_server_async() -> Result<(), SteamError> {
        let addresses = tokio::net::lookup_host((TRIGGER_DOMAIN, 80u16))
            .await
            .map_err(|error| {
                SteamError::Connection(format!(
                    "Failed to resolve {TRIGGER_DOMAIN}: {error}"
                ))
            })?;

        // Lancache's address has to be a private (RFC 1918) one, so no private
        // addresses means no Lancache.
        let use_lancache = addresses
            .into_iter()
            .map(|address| address.ip())
            .any(|address| Self::is_private_address(&address));

        USE_LANCACHE_SERVER.store(use_lancache, Ordering::Relaxed);

        Ok(())
    }

    /// Whether an IP address is private, as defined by RFC 1918.
    pub fn is_private_address(to_test: &IpAddr) -> bool {
        match to_test {
            IpAddr::V4(address) => {
                if address.is_loopback() {
                    return true;
                }

                match address.octets() {
                    [10, _, _, _] => true,
                    [172, second, _, _] => (16..32).contains(&second),
                    [192, 168, _, _] => true,
                    _ => false,
                }
            }
            IpAddr::V6(address) => {
                // Unique local addresses (fc00::/7) and loopback (::1).
                let segments = address.segments();

                (segments[0] & 0xFE00) == 0xFC00
                    || address.is_unicast_link_local()
                    || address.is_loopback()
            }
        }
    }

    /// Builds a request that goes through the local Lancache.
    pub(crate) fn build_lancache_request(
        http_client: &reqwest::Client,
        server: &Server,
        command: &str,
        query: Option<&str>,
    ) -> Result<reqwest::RequestBuilder, SteamError> {
        let mut url = Url::parse(&format!(
            "http://{TRIGGER_DOMAIN}:80/{}",
            command.trim_start_matches('/')
        ))
        .map_err(|error| SteamError::InvalidArgument(format!("Invalid lancache url: {error}")))?;

        if let Some(query) = query {
            if !query.is_empty() {
                url.set_query(Some(query));
            }
        }

        let mut headers = HeaderMap::new();

        if let Some(host) = server.host.as_deref() {
            if !host.is_empty() {
                headers.insert(
                    HOST,
                    HeaderValue::from_str(host).map_err(|error| {
                        SteamError::InvalidArgument(format!("Invalid lancache host header: {error}"))
                    })?,
                );
            }
        }

        // The user agent has to match the Steam client's for Lancache to
        // recognize and cache the content.
        headers.insert(USER_AGENT, HeaderValue::from_static(LANCACHE_USER_AGENT));

        Ok(http_client.get(url).headers(headers))
    }
}
