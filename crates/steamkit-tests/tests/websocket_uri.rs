//! WebSocket URI tests.
//!
//! [`construct_uri`] takes a `SocketAddr` and [`construct_uri_for_host`] a
//! host name, so an endpoint of any other kind can't be expressed.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use steamkit::networking::steam3::web_socket_context::{
    construct_uri, construct_uri_for_host, WEBSOCKET_PATH, WEBSOCKET_SCHEME,
};

/// Splits a websocket URI into its scheme, host, port and path.
fn uri_parts(uri: &str) -> (String, String, String, String) {
    let (scheme, remainder) = uri
        .split_once("://")
        .expect("the URI must carry a scheme");
    let (authority, path) = remainder
        .split_once('/')
        .expect("the URI must carry a path");
    let (host, port) = authority
        .rsplit_once(':')
        .expect("the URI must carry a port");

    (
        scheme.to_string(),
        host.to_string(),
        port.to_string(),
        format!("/{path}"),
    )
}

#[test]
fn dns_end_point() {
    assert_eq!(
        "wss://example.com:1337/cmsocket/",
        construct_uri_for_host("example.com", 1337)
    );
}

#[test]
fn ip_end_point_v4() {
    let endpoint = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1337);
    assert_eq!("wss://127.0.0.1:1337/cmsocket/", construct_uri(endpoint));
}

#[test]
fn ip_end_point_v6() {
    let endpoint = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 1337);
    assert_eq!("wss://[::1]:1337/cmsocket/", construct_uri(endpoint));
}

#[test]
fn throws_wrong_end_point() {
    // An endpoint of an unknown kind can't be expressed, so there's nothing to
    // reject. Check what holds for every supported endpoint instead: a `wss`
    // URI with the path `/cmsocket/`.
    assert_eq!("wss", WEBSOCKET_SCHEME);
    assert_eq!("/cmsocket/", WEBSOCKET_PATH);

    let dns_uri = construct_uri_for_host("example.com", 1337);
    let (scheme, host, port, path) = uri_parts(&dns_uri);
    assert_eq!("wss", scheme);
    assert_eq!("example.com", host);
    assert_eq!("1337", port);
    assert_eq!("/cmsocket/", path);

    let ip_uri = construct_uri(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1337));
    let (scheme, host, port, path) = uri_parts(&ip_uri);
    assert_eq!("wss", scheme);
    assert_eq!("127.0.0.1", host);
    assert_eq!("1337", port);
    assert_eq!("/cmsocket/", path);
}
