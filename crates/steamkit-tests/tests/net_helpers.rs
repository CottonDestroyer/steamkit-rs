//! `NetHelpers` tests.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use steamkit::util::net_helpers::NetHelpers;
use steamkit_protos::steam::c_msg_ip_address;

/// The IPv4 value of a `CMsgIpAddress`.
fn v4(message: &steamkit_protos::steam::CMsgIpAddress) -> u32 {
    match message.ip.as_ref() {
        Some(c_msg_ip_address::Ip::V4(value)) => *value,
        other => panic!("the message must carry an IPv4 address, got {other:?}"),
    }
}

/// The IPv6 bytes of a `CMsgIpAddress`.
fn v6(message: &steamkit_protos::steam::CMsgIpAddress) -> Vec<u8> {
    match message.ip.as_ref() {
        Some(c_msg_ip_address::Ip::V6(value)) => value.clone(),
        other => panic!("the message must carry an IPv6 address, got {other:?}"),
    }
}

#[test]
fn get_msg_ip_address() {
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    assert_eq!(2130706433u32, v4(&NetHelpers::get_msg_ip_address(&loopback)));

    let loopback_v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
    assert_eq!(
        vec![
            0u8, 0, 0, 0, //
            0, 0, 0, 0, //
            0, 0, 0, 0, //
            0, 0, 0, 1
        ],
        v6(&NetHelpers::get_msg_ip_address(&loopback_v6))
    );
}

#[test]
fn get_ip_address_from_msg() {
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let message = NetHelpers::get_msg_ip_address(&loopback);
    assert_eq!(
        Some(loopback),
        NetHelpers::get_ip_address_from_msg(&message)
    );

    let loopback_v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
    let message = NetHelpers::get_msg_ip_address(&loopback_v6);
    assert_eq!(
        Some(loopback_v6),
        NetHelpers::get_ip_address_from_msg(&message)
    );
}

#[test]
fn get_ip_address() {
    assert_eq!(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        NetHelpers::get_ip_address(2130706433)
    );
    assert_eq!(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 1)),
        NetHelpers::get_ip_address(1)
    );
    assert_eq!(
        IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
        NetHelpers::get_ip_address(u32::MAX)
    );
    assert_eq!(
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        NetHelpers::get_ip_address(0)
    );
}

#[test]
fn get_ip_address_as_uint() {
    assert_eq!(
        2130706433u32,
        NetHelpers::get_ip_address_as_uint(&IpAddr::V4(Ipv4Addr::LOCALHOST))
            .expect("an IPv4 address must convert")
    );
    assert_eq!(
        1u32,
        NetHelpers::get_ip_address_as_uint(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 1)))
            .expect("an IPv4 address must convert")
    );
    assert_eq!(
        u32::MAX,
        NetHelpers::get_ip_address_as_uint(&IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)))
            .expect("an IPv4 address must convert")
    );
    assert_eq!(
        3232235521u32,
        NetHelpers::get_ip_address_as_uint(&IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1)))
            .expect("an IPv4 address must convert")
    );
    assert_eq!(
        167772161u32,
        NetHelpers::get_ip_address_as_uint(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
            .expect("an IPv4 address must convert")
    );
    assert_eq!(
        2886729729u32,
        NetHelpers::get_ip_address_as_uint(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)))
            .expect("an IPv4 address must convert")
    );
}

#[test]
fn obfuscate_private_ip() {
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut message = NetHelpers::get_msg_ip_address(&loopback);
    NetHelpers::obfuscate_private_ip(&mut message);
    assert_eq!(3316510732u32, v4(&message));

    let loopback_v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
    let mut message = NetHelpers::get_msg_ip_address(&loopback_v6);
    NetHelpers::obfuscate_private_ip(&mut message);
    assert_eq!(
        vec![
            0x0Du8, 0xF0, 0xAD, 0xBA, //
            0x0D, 0xF0, 0xAD, 0xBA, //
            0x0D, 0xF0, 0xAD, 0xBA, //
            0x0D, 0xF0, 0xAD, 1 ^ 0xBA
        ],
        v6(&message)
    );
}

#[test]
fn try_parse_ip_end_point() {
    let parsed_ip = NetHelpers::try_parse_ip_end_point("127.0.0.1:1337")
        .expect("a valid IPv4 endpoint must parse");
    assert_eq!(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1337),
        parsed_ip
    );

    let parsed_ipv6 = NetHelpers::try_parse_ip_end_point("[::1]:1337")
        .expect("a valid IPv6 endpoint must parse");
    assert_eq!(
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 1337),
        parsed_ipv6
    );
}
