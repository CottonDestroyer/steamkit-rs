//! `CDNClient` tests.
//!
//! Two tests need every CDN request answered with HTTP 418. The HTTP client
//! is `reqwest`, which has no seam for a fake transport, so those two are
//! `#[ignore]`d. They fail loudly if run, since the client really does try to
//! reach `localhost:80`.

mod common;

use std::net::IpAddr;

use steamkit::error::SteamError;
use steamkit::steam::cdn::client::Client;
use steamkit::steam::cdn::server::Server;
use steamkit::types::depot_manifest::ChunkData;

/// A plain HTTP server at `localhost:80`.
fn localhost_server() -> Server {
    // `from_dns_end_point` uses HTTP unless the port is 443.
    Server::from_dns_end_point("localhost", 80)
}

/// Parses an IP address, panicking on bad input.
fn parse_ip(address: &str) -> IpAddr {
    match address.parse::<IpAddr>() {
        Ok(address) => address,
        Err(error) => panic!("failed to parse IP address {address}: {error}"),
    }
}

/// Checks that a request failed with HTTP 418.
///
/// Non-success statuses come back as `SteamError::Other` with the status code
/// in the message.
fn assert_teapot_response(error: &SteamError) {
    match error {
        SteamError::Other(message) => assert!(
            message.contains("418"),
            "expected the HTTP 418 status code in the error, got: {message}"
        ),
        other => panic!("expected an HTTP 418 response error, got: {other}"),
    }
}

#[tokio::test]
#[ignore = "needs a mock HTTP transport that answers every request with HTTP 418, which reqwest has no seam for"]
async fn throws_steam_kit_web_exception_on_unsuccessful_web_response_for_manifest() {
    let steam_client = common::connected_steam_client();
    let client = Client::from_steam_client(&steam_client);
    let server = localhost_server();

    let result = client
        .download_manifest_async(0, 0, 0, &server, None, None, None)
        .await;

    match result {
        Ok(_) => panic!("expected the CDN manifest download to fail"),
        Err(error) => assert_teapot_response(&error),
    }
}

#[tokio::test]
#[ignore = "needs a mock HTTP transport that answers every request with HTTP 418, which reqwest has no seam for"]
async fn throws_steam_kit_web_exception_on_unsuccessful_web_response_for_chunk() {
    let steam_client = common::connected_steam_client();
    let client = Client::from_steam_client(&steam_client);
    let server = localhost_server();

    let chunk = ChunkData::with_values(vec![0xFF], 0, 0, 0, 0);
    let mut destination: [u8; 0] = [];

    let result = client
        .download_depot_chunk_async(0, &chunk, &server, &mut destination, None, None, None)
        .await;

    match result {
        Ok(_) => panic!("expected the CDN chunk download to fail"),
        Err(error) => assert_teapot_response(&error),
    }
}

#[tokio::test]
async fn throws_when_no_chunk_id_is_set() {
    let steam_client = common::connected_steam_client();
    let client = Client::from_steam_client(&steam_client);
    let server = localhost_server();

    // Every field left at its default.
    let chunk = ChunkData::new();
    let mut destination: [u8; 0] = [];

    let result = client
        .download_depot_chunk_async(0, &chunk, &server, &mut destination, None, None, None)
        .await;

    // Arguments are checked before the request is built, so this needs no
    // network.
    match result {
        Err(SteamError::InvalidArgument(message)) => assert!(
            message.contains("ChunkID"),
            "expected the error to name the chunk id, got: {message}"
        ),
        other => panic!("expected an invalid argument error, got: {:?}", other.err()),
    }
}

#[tokio::test]
async fn throws_when_destination_buffer_smaller() {
    let steam_client = common::connected_steam_client();
    let client = Client::from_steam_client(&steam_client);
    let server = localhost_server();

    let chunk = ChunkData::with_values(vec![0xFF], 0, 0, 32, 64);
    let mut destination = [0u8; 4];

    let result = client
        .download_depot_chunk_async(0, &chunk, &server, &mut destination, None, None, None)
        .await;

    // Without a depot key the destination must hold the *compressed* length.
    match result {
        Err(SteamError::InvalidArgument(message)) => assert!(
            message.contains("CompressedLength"),
            "expected the error to name the compressed length, got: {message}"
        ),
        other => panic!("expected an invalid argument error, got: {:?}", other.err()),
    }
}

#[tokio::test]
async fn throws_when_destination_buffer_smaller_with_depot_key() {
    let steam_client = common::connected_steam_client();
    let client = Client::from_steam_client(&steam_client);
    let server = localhost_server();

    let chunk = ChunkData::with_values(vec![0xFF], 0, 0, 32, 64);
    let mut destination = [0u8; 4];

    let result = client
        .download_depot_chunk_async(0, &chunk, &server, &mut destination, Some(&[]), None, None)
        .await;

    // With a depot key the destination must hold the *uncompressed* length.
    match result {
        Err(SteamError::InvalidArgument(message)) => assert!(
            message.contains("UncompressedLength"),
            "expected the error to name the uncompressed length, got: {message}"
        ),
        other => panic!("expected an invalid argument error, got: {:?}", other.err()),
    }
}

#[test]
fn is_private_address_ipv4_10_0_0_1() {
    assert!(Client::is_private_address(&parse_ip("10.0.0.1")));
}

#[test]
fn is_private_address_ipv4_172_16_0_1() {
    assert!(Client::is_private_address(&parse_ip("172.16.0.1")));
}

#[test]
fn is_private_address_ipv4_192_168_0_1() {
    assert!(Client::is_private_address(&parse_ip("192.168.0.1")));
}

#[test]
fn is_private_address_ipv4_8_8_8_8() {
    assert!(!Client::is_private_address(&parse_ip("8.8.8.8")));
}

#[test]
fn is_private_address_ipv4_127_0_0_1() {
    assert!(Client::is_private_address(&parse_ip("127.0.0.1")));
}

#[test]
fn is_private_address_ipv6_fc00_1() {
    assert!(Client::is_private_address(&parse_ip("fc00::1")));
}

#[test]
fn is_private_address_ipv6_fe80_1() {
    assert!(Client::is_private_address(&parse_ip("fe80::1")));
}

#[test]
fn is_private_address_ipv6_2001_db8_1() {
    assert!(!Client::is_private_address(&parse_ip("2001:db8::1")));
}

#[test]
fn is_private_address_ipv6_loopback() {
    assert!(Client::is_private_address(&parse_ip("::1")));
}
