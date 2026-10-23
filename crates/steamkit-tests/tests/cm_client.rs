//! `CMClient::get_packet_msg` tests.
//!
//! # Packet type assertions
//!
//! `IPacketMsg` has no `as_any` and the concrete packet types keep their
//! fields private, so a `Box<dyn IPacketMsg>` can't be downcast to
//! `PacketMsg` / `PacketClientMsg` / `PacketClientMsgProtobuf`. Instead the
//! tests check every observable property and, where the payload allows it,
//! re-parse the payload as each candidate type to rule out the ones that
//! couldn't have produced the packet.

use steamkit::base::generated::steam_language::EMsg;
use steamkit::base::generated::steam_language_internal::{
    make_msg, ExtendedClientMsgHdr, MsgHdr, MsgHdrProtoBuf, SteamSerializable,
};
use steamkit::base::packet_base::{IPacketMsg, PacketClientMsg, PacketMsg};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::types::job_id::JobID;
use steamkit::SteamError;

/// Turns a library `Result` into its value, or fails the test with a
/// descriptive message.
fn or_fail<T>(result: Result<T, SteamError>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: {error}"),
    }
}

/// Unwraps the packet, or fails the test.
fn or_packet(packet: Option<Box<dyn IPacketMsg>>, what: &str) -> Box<dyn IPacketMsg> {
    match packet {
        Some(packet) => packet,
        None => panic!("{what}: GetPacketMsg returned no packet message"),
    }
}

/// Serialises a header.
fn serialize_header(hdr: &impl SteamSerializable) -> Vec<u8> {
    let mut data = Vec::new();
    hdr.write(&mut data);
    data
}

/// Asserts that `packet` carries `expected_msg`, which must be the only
/// non-protobuf interpretation of `data` that the library can produce.
fn assert_is_packet_msg(packet: &dyn IPacketMsg, expected_msg: EMsg, data: &[u8]) {
    assert!(!packet.is_proto(), "the packet must not be protobuf backed");
    assert_eq!(expected_msg, packet.msg_type());
    assert_eq!(data, packet.get_data());

    // The extended client header is 36 bytes long, so a bare `MsgHdr` payload
    // can never have produced a `PacketClientMsg`; and `PacketClientMsgProtobuf`
    // is the only type that reports `is_proto() == true`. What is left is
    // `PacketMsg`.
    assert!(
        PacketClientMsg::new(expected_msg, data.to_vec()).is_err(),
        "the payload must not be a valid extended client message header, \
         otherwise the packet could be a PacketClientMsg"
    );
}

/// Asserts that `packet` carries `expected_msg` and is backed by an extended
/// client message header.
fn assert_is_packet_client_msg(packet: &dyn IPacketMsg, expected_msg: EMsg, data: &[u8]) {
    assert!(!packet.is_proto(), "the packet must not be protobuf backed");
    assert_eq!(expected_msg, packet.msg_type());
    assert_eq!(data, packet.get_data());
    assert_eq!(JobID::INVALID, packet.target_job_id());
    assert_eq!(JobID::INVALID, packet.source_job_id());

    // The payload is a valid extended client message header, which is the only
    // non-protobuf header the non-protobuf branch of `GetPacketMsg` can build.
    let expected = or_fail(
        PacketClientMsg::new(expected_msg, data.to_vec()),
        "the payload must be a valid extended client message header",
    );
    assert_eq!(36, expected.body_offset());
}

#[test]
fn get_packet_msg_returns_packet_msg_for_crypto_handshake() {
    let client = SteamClient::new();
    let cm_client = client.cm_client();

    let messages = [
        EMsg::ChannelEncryptRequest,
        EMsg::ChannelEncryptResponse,
        EMsg::ChannelEncryptResult,
    ];

    for emsg in messages {
        let msg_hdr = MsgHdr {
            msg: emsg,
            ..MsgHdr::default()
        };

        let data = serialize_header(&msg_hdr);

        let packet_msg = or_packet(
            cm_client.get_packet_msg(data.clone()),
            "the crypto handshake message must parse",
        );

        assert_is_packet_msg(packet_msg.as_ref(), emsg, &data);

        // The concrete `PacketMsg` interpretation of the very same payload.
        let expected = or_fail(
            PacketMsg::new(emsg, data.clone()),
            "the payload must parse as a PacketMsg",
        );
        assert_eq!(expected.msg_type(), packet_msg.msg_type());
        assert_eq!(expected.target_job_id(), packet_msg.target_job_id());
        assert_eq!(expected.source_job_id(), packet_msg.source_job_id());
        assert_eq!(20, expected.body_offset());
    }
}

#[test]
fn get_packet_msg_returns_packet_client_msg_protobuf_for_messages_with_protomask() {
    let client = SteamClient::new();
    let cm_client = client.cm_client();

    let msg = make_msg(EMsg::ClientLogOnResponse, true);
    let msg_hdr = MsgHdrProtoBuf {
        msg: EMsg::ClientLogOnResponse,
        ..MsgHdrProtoBuf::default()
    };

    let data = serialize_header(&msg_hdr);

    // The serialised header carries the protobuf masked message type.
    assert_eq!(
        msg as i32,
        i32::from_le_bytes([data[0], data[1], data[2], data[3]])
    );

    let packet_msg = or_packet(
        cm_client.get_packet_msg(data.clone()),
        "the protomasked message must parse",
    );

    assert!(packet_msg.is_proto(), "the packet must be protobuf backed");
    assert_eq!(EMsg::ClientLogOnResponse, packet_msg.msg_type());
    assert_eq!(data, packet_msg.get_data());
    assert_eq!(JobID::INVALID, packet_msg.target_job_id());
    assert_eq!(JobID::INVALID, packet_msg.source_job_id());
}

#[test]
fn get_packet_msg_returns_packet_client_msg_for_other_messages() {
    let client = SteamClient::new();
    let cm_client = client.cm_client();

    let msg = make_msg(EMsg::ClientLogOnResponse, false);
    let msg_hdr = ExtendedClientMsgHdr {
        msg: EMsg::ClientLogOnResponse,
        ..ExtendedClientMsgHdr::default()
    };

    let data = serialize_header(&msg_hdr);

    // The serialised header carries the unmasked message type.
    assert_eq!(
        msg as i32,
        i32::from_le_bytes([data[0], data[1], data[2], data[3]])
    );

    let packet_msg = or_packet(
        cm_client.get_packet_msg(data.clone()),
        "the extended client message must parse",
    );

    assert_is_packet_client_msg(packet_msg.as_ref(), EMsg::ClientLogOnResponse, &data);
}

#[test]
fn get_packet_msg_fails_with_null() {
    let client = SteamClient::new();
    let cm_client = client.cm_client();

    let msg = make_msg(EMsg::ClientLogOnResponse, true);
    let msg_hdr = MsgHdrProtoBuf {
        msg: EMsg::ClientLogOnResponse,
        ..MsgHdrProtoBuf::default()
    };

    let mut data = serialize_header(&msg_hdr);
    // A header length of -1.
    data[4..8].copy_from_slice(&(-1i32).to_le_bytes());

    assert_eq!(
        msg as i32,
        i32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        "the message type itself is untouched, only the header length is poisoned"
    );

    assert!(
        cm_client.get_packet_msg(data).is_none(),
        "a negative protobuf header length must not parse"
    );
}

#[test]
fn get_packet_msg_fails_with_tiny_array() {
    let client = SteamClient::new();
    let cm_client = client.cm_client();

    assert!(
        cm_client.get_packet_msg(vec![0u8; 3]).is_none(),
        "an array that is too small to hold a message type must not parse"
    );
}
