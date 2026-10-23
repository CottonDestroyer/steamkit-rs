//! `ServerRecord` tests.
//!
//! * The DNS factories resolve the host name, so the records here are built
//!   directly with `ServerRecord::new(EndPoint::dns(..))`, which gives the
//!   same value without touching the network.
//! * Hashes are compared with `DefaultHasher`, the hasher the standard
//!   collections use.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::steam::discovery::server_record::{EndPoint, ServerRecord};

/// The address behind the `host` name. Only the host name and the port take
/// part in equality, so any address works.
const HOST_ADDRESS: &str = "127.0.0.1:1";

/// The address behind the `host` name.
fn host_address() -> SocketAddr {
    HOST_ADDRESS
        .parse()
        .expect("HOST_ADDRESS is a valid socket address")
}

/// A server at `host` with the given port and protocols.
fn create_server(port: u16, protocol_types: ProtocolTypes) -> ServerRecord {
    ServerRecord::new(EndPoint::dns("host", port, host_address()), protocol_types)
}

/// A WebSocket server at `host:1`.
fn create_websocket_server() -> ServerRecord {
    ServerRecord::new(
        EndPoint::dns("host", 1, host_address()),
        ProtocolTypes::WEB_SOCKET,
    )
}

/// The hash code of a record, the way Rust's own collections compute it.
fn hash_of(record: &ServerRecord) -> u64 {
    let mut hasher = DefaultHasher::new();
    record.hash(&mut hasher);
    hasher.finish()
}

/// The factory produces a usable record.
#[test]
fn null_is_not_equal() {
    let record = create_websocket_server();

    assert!(record.is_dns(), "the record should be a DNS endpoint");
    assert_eq!("host", record.get_host());
    assert_eq!(1, record.get_port());
    assert_eq!(ProtocolTypes::WEB_SOCKET, record.protocol_types());
    assert_eq!(host_address(), record.end_point());
}

#[test]
fn different_protocols_are_not_equal() {
    let l = create_server(1, ProtocolTypes::TCP);
    let r = create_server(1, ProtocolTypes::WEB_SOCKET);

    assert!(l != r);
    assert!(r != l);
    assert!(!(l == r));
    assert!(!(r == l));
    assert!(!l.eq(&r));
    assert!(!r.eq(&l));
}

#[test]
fn different_end_points_are_not_equal() {
    let l = create_server(1, ProtocolTypes::TCP);
    let r = create_server(2, ProtocolTypes::TCP);

    assert!(l != r);
    assert!(r != l);
    assert!(!(l == r));
    assert!(!(r == l));
    assert!(!l.eq(&r));
    assert!(!r.eq(&l));
}

#[test]
fn different_end_points_and_protocols_are_not_equal() {
    let l = create_server(1, ProtocolTypes::TCP);
    let r = create_server(2, ProtocolTypes::WEB_SOCKET);

    assert!(l != r);
    assert!(r != l);
    assert!(!(l == r));
    assert!(!(r == l));
    assert!(!l.eq(&r));
    assert!(!r.eq(&l));
}

#[test]
fn same_end_points_and_protocols_are_equal() {
    let l = create_server(1, ProtocolTypes::TCP);
    let r = create_server(1, ProtocolTypes::TCP);

    assert!(!(l != r));
    assert!(!(r != l));
    assert!(l == r);
    assert!(r == l);
    assert!(l.eq(&r));
    assert!(r.eq(&l));

    assert_eq!(hash_of(&l), hash_of(&r));
}

#[test]
fn can_try_create_socket_server() {
    let record = ServerRecord::try_create_socket_server("127.0.0.1:1234");
    assert!(record.is_some(), "127.0.0.1:1234 should parse");

    let record = record.expect("127.0.0.1:1234 should parse");
    assert_eq!(
        SocketAddr::from(([127, 0, 0, 1], 1234)),
        record.end_point(),
        "the endpoint should be the loopback address on port 1234"
    );
    assert_eq!(
        ProtocolTypes::TCP | ProtocolTypes::UDP,
        record.protocol_types()
    );

    let record = ServerRecord::try_create_socket_server("192.168.0.1:5678");
    assert!(record.is_some(), "192.168.0.1:5678 should parse");

    let record = record.expect("192.168.0.1:5678 should parse");
    assert_eq!(
        SocketAddr::from(([192, 168, 0, 1], 5678)),
        record.end_point(),
        "the endpoint should be 192.168.0.1 on port 5678"
    );
    assert_eq!(
        ProtocolTypes::TCP | ProtocolTypes::UDP,
        record.protocol_types()
    );
}

#[test]
fn cannot_try_create_socket_server() {
    // no port at all
    assert!(ServerRecord::try_create_socket_server("127.0.0.1").is_none());

    // port out of range
    assert!(ServerRecord::try_create_socket_server("127.0.0.1:123456789").is_none());

    // negative port
    assert!(ServerRecord::try_create_socket_server("127.0.0.1:-1234").is_none());

    // port is not an integer
    assert!(ServerRecord::try_create_socket_server("127.0.0.1:notanint").is_none());

    // a DNS name rather than an IP address
    assert!(ServerRecord::try_create_socket_server("volvopls.valvesoftware.com:1234").is_none());
}
