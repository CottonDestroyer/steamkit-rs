//! Tests for `networking/steam3/udp_packet.rs`: the 36 byte header of a Steam
//! UDP datagram, the handshake payloads, and the packet around them.

use steamkit::base::generated::steam_language::EUdpPacketType;
use steamkit::networking::steam3::udp_packet::{
    ChallengeData, ConnectData, UdpHeader, UdpPacket, MAX_PAYLOAD,
};
use steamkit::SteamError;

fn sample_header() -> UdpHeader {
    UdpHeader {
        magic: UdpHeader::MAGIC,
        payload_size: 0x0102,
        packet_type: EUdpPacketType::Data,
        flags: 0x04,
        source_conn_id: 0x0506_0708,
        dest_conn_id: 0x090A_0B0C,
        seq_this: 0x0D0E_0F10,
        seq_ack: 0x1112_1314,
        packets_in_msg: 0x1516_1718,
        msg_start_seq: 0x191A_1B1C,
        msg_size: 0x1D1E_1F20,
    }
}

#[test]
fn the_header_has_the_documented_little_endian_layout() {
    let bytes = sample_header().serialize();

    assert_eq!(UdpHeader::SERIALIZED_SIZE, bytes.len());
    assert_eq!(b"VS01", &bytes[0..4], "the magic reads VS01");
    assert_eq!([0x02, 0x01], bytes[4..6]);
    assert_eq!(6, bytes[6], "Data");
    assert_eq!(0x04, bytes[7]);
    assert_eq!([0x08, 0x07, 0x06, 0x05], bytes[8..12]);
    assert_eq!([0x0C, 0x0B, 0x0A, 0x09], bytes[12..16]);
    assert_eq!([0x10, 0x0F, 0x0E, 0x0D], bytes[16..20]);
    assert_eq!([0x14, 0x13, 0x12, 0x11], bytes[20..24]);
    assert_eq!([0x18, 0x17, 0x16, 0x15], bytes[24..28]);
    assert_eq!([0x1C, 0x1B, 0x1A, 0x19], bytes[28..32]);
    assert_eq!([0x20, 0x1F, 0x1E, 0x1D], bytes[32..36]);
}

#[test]
fn the_header_round_trips() {
    let header = sample_header();
    let mut parsed = UdpHeader::new();
    parsed.deserialize(&header.serialize()).unwrap();
    assert_eq!(header, parsed);
}

#[test]
fn every_packet_type_round_trips() {
    for packet_type in [
        EUdpPacketType::Invalid,
        EUdpPacketType::ChallengeReq,
        EUdpPacketType::Challenge,
        EUdpPacketType::Connect,
        EUdpPacketType::Accept,
        EUdpPacketType::Disconnect,
        EUdpPacketType::Data,
        EUdpPacketType::Datagram,
        EUdpPacketType::Max,
    ] {
        let mut header = UdpHeader::new();
        header.packet_type = packet_type;
        let mut parsed = UdpHeader::new();
        parsed.deserialize(&header.serialize()).unwrap();
        assert_eq!(packet_type, parsed.packet_type);
    }
}

#[test]
fn an_unknown_packet_type_reads_as_invalid() {
    let mut bytes = sample_header().serialize();
    bytes[6] = 200;
    let mut parsed = UdpHeader::new();
    parsed.deserialize(&bytes).unwrap();
    assert_eq!(EUdpPacketType::Invalid, parsed.packet_type);
}

#[test]
fn the_default_header_has_the_expected_defaults() {
    let header = UdpHeader::default();
    assert_eq!(UdpHeader::MAGIC, header.magic);
    assert_eq!(512, header.source_conn_id);
    assert_eq!(EUdpPacketType::Invalid, header.packet_type);
    assert_eq!(0, header.payload_size);
}

#[test]
fn a_truncated_header_is_rejected() {
    let bytes = sample_header().serialize();
    for length in 0..UdpHeader::SERIALIZED_SIZE {
        let mut parsed = UdpHeader::new();
        assert!(
            matches!(parsed.deserialize(&bytes[..length]), Err(SteamError::Malformed(_))),
            "{length} bytes"
        );
    }
}

#[test]
fn a_packet_round_trips_through_its_wire_form() {
    let payload: Vec<u8> = (0..300).map(|index| index as u8).collect();
    let mut packet = UdpPacket::with_payload(EUdpPacketType::Data, payload.clone()).unwrap();
    packet.header.seq_this = 7;
    packet.header.seq_ack = 6;

    let parsed = UdpPacket::from_bytes(&packet.get_data());

    assert!(parsed.is_valid());
    assert_eq!(payload, parsed.payload());
    assert_eq!(EUdpPacketType::Data, parsed.header.packet_type);
    assert_eq!(7, parsed.header.seq_this);
    assert_eq!(6, parsed.header.seq_ack);
    assert_eq!(300, parsed.header.payload_size);
    assert_eq!(300, parsed.header.msg_size);
}

#[test]
fn setting_a_payload_updates_the_sizes_in_the_header() {
    let mut packet = UdpPacket::new(EUdpPacketType::Datagram);
    assert!(packet.payload().is_empty());

    packet.set_payload(vec![1, 2, 3]).unwrap();
    assert_eq!(3, packet.header.payload_size);
    assert_eq!(3, packet.header.msg_size);
    assert_eq!(UdpHeader::SERIALIZED_SIZE + 3, packet.get_data().len());
}

#[test]
fn the_payload_is_limited_to_max_payload_bytes() {
    assert!(UdpPacket::with_payload(EUdpPacketType::Data, vec![0; MAX_PAYLOAD]).is_ok());
    assert!(matches!(
        UdpPacket::with_payload(EUdpPacketType::Data, vec![0; MAX_PAYLOAD + 1]),
        Err(SteamError::InvalidArgument(_))
    ));
    assert!(matches!(
        UdpPacket::with_payload_slice(EUdpPacketType::Data, &[0; 2000], 0, MAX_PAYLOAD + 1),
        Err(SteamError::InvalidArgument(_))
    ));
}

#[test]
fn a_payload_slice_is_taken_from_the_offset_and_zero_padded_past_the_end() {
    let source: Vec<u8> = (0..10).collect();

    let packet = UdpPacket::with_payload_slice(EUdpPacketType::Data, &source, 4, 3).unwrap();
    assert_eq!(&[4, 5, 6], packet.payload());

    // Whatever is there gets read; the rest of the buffer stays zeroed.
    let packet = UdpPacket::with_payload_slice(EUdpPacketType::Data, &source, 8, 5).unwrap();
    assert_eq!(&[8, 9, 0, 0, 0], packet.payload());

    let packet = UdpPacket::with_payload_slice(EUdpPacketType::Data, &source, 50, 2).unwrap();
    assert_eq!(&[0, 0], packet.payload());
}

#[test]
fn a_datagram_with_a_bad_magic_is_invalid() {
    let mut bytes = UdpPacket::with_payload(EUdpPacketType::Data, vec![1, 2, 3]).unwrap().get_data();
    bytes[0] ^= 0xFF;

    let parsed = UdpPacket::from_bytes(&bytes);
    assert!(!parsed.is_valid());
    assert!(parsed.payload().is_empty());
}

#[test]
fn a_datagram_too_short_for_a_header_is_invalid() {
    let bytes = UdpPacket::new(EUdpPacketType::Data).get_data();
    assert!(!UdpPacket::from_bytes(&bytes[..10]).is_valid());
    assert!(!UdpPacket::from_bytes(&[]).is_valid());
}

#[test]
fn a_datagram_claiming_an_oversized_payload_is_invalid() {
    let mut header = sample_header();
    header.payload_size = (MAX_PAYLOAD + 1) as u16;
    let mut bytes = header.serialize();
    bytes.extend(vec![0u8; MAX_PAYLOAD + 1]);

    assert!(!UdpPacket::from_bytes(&bytes).is_valid());
}

#[test]
fn a_datagram_shorter_than_its_payload_size_is_zero_padded() {
    let mut header = sample_header();
    header.payload_size = 8;
    let mut bytes = header.serialize();
    bytes.extend_from_slice(&[1, 2, 3]);

    let parsed = UdpPacket::from_bytes(&bytes);
    assert!(parsed.is_valid());
    assert_eq!(&[1, 2, 3, 0, 0, 0, 0, 0], parsed.payload());
}

#[test]
fn challenge_data_round_trips() {
    let challenge = ChallengeData { challenge_value: 0x1234_5678, server_load: 0x9ABC_DEF0 };
    let bytes = challenge.serialize();
    assert_eq!(8, bytes.len());
    assert_eq!([0x78, 0x56, 0x34, 0x12], bytes[0..4]);

    let parsed = ChallengeData::deserialize(&bytes).unwrap();
    assert_eq!(challenge.challenge_value, parsed.challenge_value);
    assert_eq!(challenge.server_load, parsed.server_load);

    assert!(ChallengeData::deserialize(&bytes[..7]).is_err());
}

#[test]
fn connect_data_round_trips() {
    let connect = ConnectData { challenge_value: 0x0BAD_F00D ^ ConnectData::CHALLENGE_MASK };
    let parsed = ConnectData::deserialize(&connect.serialize()).unwrap();
    assert_eq!(connect.challenge_value, parsed.challenge_value);
    assert_eq!(ChallengeData::CHALLENGE_MASK, ConnectData::CHALLENGE_MASK);

    assert!(ConnectData::deserialize(&[1, 2, 3]).is_err());
}
