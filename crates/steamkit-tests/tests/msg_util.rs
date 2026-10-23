//! Tests for `util/msg_util.rs` (the protobuf flag on message numbers) and
//! `util/key_dictionary.rs` (the universe public keys).

use steamkit::base::generated::steam_language::{EMsg, EUniverse};
use steamkit::networking::steam3::envelope_encrypted_connection::public_key_for_universe;
use steamkit::util::key_dictionary::{get_public_key, KeyDictionary};
use steamkit::util::msg_util::MsgUtil;

const PROTO_MASK: u32 = 0x8000_0000;

#[test]
fn make_msg_sets_the_protobuf_flag_only_when_asked() {
    assert_eq!(EMsg::ClientLogon as u32, MsgUtil::make_msg(EMsg::ClientLogon, false));
    assert_eq!(EMsg::ClientLogon as u32 | PROTO_MASK, MsgUtil::make_msg(EMsg::ClientLogon, true));
}

#[test]
fn get_msg_strips_the_protobuf_flag() {
    for msg in [EMsg::Multi, EMsg::ClientLogon, EMsg::ClientHello, EMsg::ChannelEncryptRequest] {
        assert_eq!(Some(msg), MsgUtil::get_msg(MsgUtil::make_msg(msg, true)));
        assert_eq!(Some(msg), MsgUtil::get_msg(MsgUtil::make_msg(msg, false)));
        assert_eq!(Some(msg), MsgUtil::get_msg_from(msg));
    }
}

#[test]
fn get_msg_of_an_unknown_number_is_none() {
    assert_eq!(None, MsgUtil::get_msg(0x7FFF_FFF0));
}

#[test]
fn is_proto_buf_tests_the_flag() {
    assert!(MsgUtil::is_proto_buf(MsgUtil::make_msg(EMsg::ClientLogon, true)));
    assert!(!MsgUtil::is_proto_buf(MsgUtil::make_msg(EMsg::ClientLogon, false)));
    assert!(!MsgUtil::is_proto_buf_from(EMsg::ClientLogon));
}

#[test]
fn game_coordinator_messages_keep_numbers_outside_the_emsg_enum() {
    let gc_msg = 4004;
    assert_eq!(gc_msg | PROTO_MASK, MsgUtil::make_gc_msg(gc_msg, true));
    assert_eq!(gc_msg, MsgUtil::make_gc_msg(gc_msg, false));
    assert_eq!(gc_msg, MsgUtil::get_gc_msg(gc_msg | PROTO_MASK));
}

#[test]
fn the_free_functions_match_the_associated_functions() {
    use steamkit::util::msg_util;

    let raw = MsgUtil::make_msg(EMsg::ClientLogon, true);
    assert_eq!(raw, msg_util::make_msg(EMsg::ClientLogon, true));
    assert_eq!(MsgUtil::get_msg(raw), msg_util::get_msg(raw));
    assert_eq!(MsgUtil::is_proto_buf(raw), msg_util::is_proto_buf(raw));
    assert_eq!(MsgUtil::make_gc_msg(9, true), msg_util::make_gc_msg(9, true));
    assert_eq!(MsgUtil::get_gc_msg(raw), msg_util::get_gc_msg(raw));
}

#[test]
fn every_real_universe_has_a_public_key() {
    for universe in [EUniverse::Public, EUniverse::Beta, EUniverse::Internal, EUniverse::Dev] {
        let key = get_public_key(universe).unwrap_or_else(|| panic!("{universe:?} has no key"));
        // A DER `SubjectPublicKeyInfo` starts with a SEQUENCE.
        assert_eq!(0x30, key[0], "{universe:?}");
        assert_eq!(Some(key), KeyDictionary::get_public_key(universe));
    }
}

#[test]
fn the_invalid_universe_has_no_public_key() {
    assert_eq!(None, get_public_key(EUniverse::Invalid));
}

#[test]
fn the_handshake_uses_the_same_keys_as_the_key_dictionary() {
    for universe in [EUniverse::Invalid, EUniverse::Public, EUniverse::Beta, EUniverse::Internal, EUniverse::Dev] {
        assert_eq!(get_public_key(universe), public_key_for_universe(universe), "{universe:?}");
    }
}

#[test]
fn the_universe_keys_are_distinct() {
    let keys: Vec<&[u8]> = [EUniverse::Public, EUniverse::Beta, EUniverse::Internal, EUniverse::Dev]
        .into_iter()
        .map(|universe| get_public_key(universe).unwrap())
        .collect();
    for (index, key) in keys.iter().enumerate() {
        for other in &keys[index + 1..] {
            assert_ne!(key, other);
        }
    }
}
