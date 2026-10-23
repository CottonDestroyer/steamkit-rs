//! `SteamID` tests.
//!
//! Some tests need account types and universes outside the enums (for
//! example account type 15). The generated enums can't hold those, so the ids
//! are built from their raw 64 bit form instead.

use std::hash::{Hash, Hasher};

use steamkit::base::generated::steam_language::{EAccountType, EUniverse};
use steamkit::types::steam_id::{ChatInstanceFlags, SteamID};

/// The raw 64 bit id most tests start from.
const BASE_ID: u64 = 76561198074261126;

/// Whether `flag` is set in the chat instance flags of an account instance.
fn has_chat_instance_flag(instance: u32, flag: ChatInstanceFlags) -> bool {
    instance & flag.value() == flag.value()
}

/// [`BASE_ID`] with the account type and universe fields replaced by the
/// given raw values.
fn steam_id_with_raw(account_type: u64, universe: u64) -> SteamID {
    let account_id = BASE_ID & 0xFFFF_FFFF;
    let instance = (BASE_ID >> 32) & 0x000F_FFFF;

    SteamID::from_id((universe << 56) | (account_type << 52) | (instance << 32) | account_id)
}

/// A default `SteamID` is invalid.
#[test]
fn empty_constructor_invalid() {
    let sid = SteamID::new();

    assert!(!sid.is_valid());
}

#[test]
fn full_constructor_valid() {
    let sid = SteamID::from_parts_instanced(
        EAccountType::Chat,
        EUniverse::Beta,
        1234,
        SteamID::CONSOLE_INSTANCE,
    );

    assert_eq!(1234u32, sid.account_id());
    assert_eq!(SteamID::CONSOLE_INSTANCE, sid.account_instance());
    assert_eq!(EUniverse::Beta, sid.account_universe());
    assert_eq!(EAccountType::Chat, sid.account_type());

    let sid = SteamID::from_parts(EAccountType::Pending, EUniverse::Invalid, 4321);

    assert_eq!(4321u32, sid.account_id());
    assert_eq!(SteamID::DESKTOP_INSTANCE, sid.account_instance());
    assert_eq!(EUniverse::Invalid, sid.account_universe());
    assert_eq!(EAccountType::Pending, sid.account_type());
}

#[test]
fn long_constructor_and_setter_getter_valid() {
    let mut sid = SteamID::from_id(103582791432294076);

    assert_eq!(2772668u32, sid.account_id());
    assert_eq!(SteamID::ALL_INSTANCES, sid.account_instance());
    assert_eq!(EUniverse::Public, sid.account_universe());
    assert_eq!(EAccountType::Clan, sid.account_type());

    sid.set_from_uint64(157626004137848889);

    assert_eq!(12345u32, sid.account_id());
    assert_eq!(SteamID::WEB_INSTANCE, sid.account_instance());
    assert_eq!(EUniverse::Beta, sid.account_universe());
    assert_eq!(EAccountType::GameServer, sid.account_type());

    assert_eq!(157626004137848889u64, sid.convert_to_u64());
}

#[test]
fn steam2_correct_parse() {
    let mut sid_even = SteamID::new();
    assert!(sid_even.set_from_string("STEAM_0:0:4491990", EUniverse::Public));

    assert_eq!(8983980u32, sid_even.account_id());
    assert_eq!(SteamID::DESKTOP_INSTANCE, sid_even.account_instance());
    assert_eq!(EUniverse::Public, sid_even.account_universe());

    let mut sid_odd = SteamID::new();
    assert!(sid_odd.set_from_string("STEAM_0:1:4491990", EUniverse::Public));

    assert_eq!(8983981u32, sid_odd.account_id());
    assert_eq!(SteamID::DESKTOP_INSTANCE, sid_odd.account_instance());
    assert_eq!(EUniverse::Public, sid_odd.account_universe());
}

#[test]
fn set_from_steam3_string_correct_parse() {
    let mut sid_user = SteamID::new();
    assert!(sid_user.set_from_steam3_string("[U:1:123]"));
    assert_eq!(123u32, sid_user.account_id());
    assert_eq!(EUniverse::Public, sid_user.account_universe());
    assert_eq!(1u32, sid_user.account_instance());
    assert_eq!(EAccountType::Individual, sid_user.account_type());

    let mut sid_anon_gs_user = SteamID::new();
    assert!(sid_anon_gs_user.set_from_steam3_string("[A:1:123:456]"));
    assert_eq!(123u32, sid_anon_gs_user.account_id());
    assert_eq!(EUniverse::Public, sid_anon_gs_user.account_universe());
    assert_eq!(456u32, sid_anon_gs_user.account_instance());
    assert_eq!(EAccountType::AnonGameServer, sid_anon_gs_user.account_type());

    let mut sid_lobby = SteamID::new();
    assert!(sid_lobby.set_from_steam3_string("[L:1:123]"));
    assert_eq!(123u32, sid_lobby.account_id());
    assert_eq!(EUniverse::Public, sid_lobby.account_universe());
    assert!(has_chat_instance_flag(
        sid_lobby.account_instance(),
        ChatInstanceFlags::Lobby
    ));
    assert_eq!(EAccountType::Chat, sid_lobby.account_type());

    let mut sid_clan_chat = SteamID::new();
    assert!(sid_clan_chat.set_from_steam3_string("[c:1:123]"));
    assert_eq!(123u32, sid_clan_chat.account_id());
    assert_eq!(EUniverse::Public, sid_clan_chat.account_universe());
    assert!(has_chat_instance_flag(
        sid_clan_chat.account_instance(),
        ChatInstanceFlags::Clan
    ));
    assert_eq!(EAccountType::Chat, sid_clan_chat.account_type());

    let mut sid_multiseat = SteamID::new();
    assert!(sid_multiseat.set_from_steam3_string("[M:1:123:456]"));
    assert_eq!(123u32, sid_multiseat.account_id());
    assert_eq!(EUniverse::Public, sid_multiseat.account_universe());
    assert_eq!(456u32, sid_multiseat.account_instance());
    assert_eq!(EAccountType::Multiseat, sid_multiseat.account_type());

    let mut sid_lowercase_i = SteamID::new();
    assert!(sid_lowercase_i.set_from_steam3_string("[i:2:456]"));
    assert_eq!(456u32, sid_lowercase_i.account_id());
    assert_eq!(EUniverse::Beta, sid_lowercase_i.account_universe());
    assert_eq!(1u32, sid_lowercase_i.account_instance());
    assert_eq!(EAccountType::Invalid, sid_lowercase_i.account_type());
}

#[test]
fn set_from_old_style_steam3_string_correct_parse() {
    let mut sid_multiseat = SteamID::new();
    assert!(sid_multiseat.set_from_steam3_string("[M:1:123(456)]"));
    assert_eq!(123u32, sid_multiseat.account_id());
    assert_eq!(EUniverse::Public, sid_multiseat.account_universe());
    assert_eq!(456u32, sid_multiseat.account_instance());
    assert_eq!(EAccountType::Multiseat, sid_multiseat.account_type());

    let mut sid_anon_gs_user = SteamID::new();
    assert!(sid_anon_gs_user.set_from_steam3_string("[A:1:123(456)]"));
    assert_eq!(123u32, sid_anon_gs_user.account_id());
    assert_eq!(EUniverse::Public, sid_anon_gs_user.account_universe());
    assert_eq!(456u32, sid_anon_gs_user.account_instance());
    assert_eq!(EAccountType::AnonGameServer, sid_anon_gs_user.account_type());
}

#[test]
fn steam3_string_symmetric() {
    let steam_ids = [
        "[U:1:123]",
        "[U:1:123:2]",
        "[G:1:626]",
        "[A:2:165:1234]",
        "[M:2:165:1234]",
    ];

    for steam_id in steam_ids {
        let mut sid = SteamID::new();
        assert!(sid.set_from_steam3_string(steam_id), "should parse {steam_id}");
        assert_eq!(steam_id, sid.render(true));
    }
}

#[test]
fn set_from_string_handles_invalid() {
    let mut sid = SteamID::new();

    assert!(!sid.set_from_string("", EUniverse::Public));

    assert!(!sid.set_from_string("NOT A STEAMID!", EUniverse::Public));

    assert!(!sid.set_from_string(
        "STEAM_0:1:999999999999999999999999999999",
        EUniverse::Public
    ));

    assert!(!sid.set_from_steam3_string("STEAM_5:0:123"));
}

#[test]
fn set_from_steam3_string_handles_invalid() {
    let mut sid = SteamID::new();

    assert!(!sid.set_from_steam3_string(""));

    assert!(!sid.set_from_steam3_string("NOT A STEAMID!"));

    assert!(!sid.set_from_steam3_string(
        "STEAM_0:1:999999999999999999999999999999"
    ));

    assert!(!sid.set_from_steam3_string("STEAM_0:1:4491990"));

    assert!(!sid.set_from_steam3_string("[A:1:2:345)]"));

    assert!(!sid.set_from_steam3_string("[A:1:2(345]"));

    assert!(!sid.set_from_steam3_string("[U:5:123]"));
}

#[test]
fn set_valid_and_handles_clan() {
    let mut sid = SteamID::new();

    sid.set(1234, EUniverse::Internal, EAccountType::ContentServer);

    assert_eq!(1234u32, sid.account_id());
    assert_eq!(EUniverse::Internal, sid.account_universe());
    assert_eq!(EAccountType::ContentServer, sid.account_type());
    assert_eq!(SteamID::DESKTOP_INSTANCE, sid.account_instance());

    sid.set(4321, EUniverse::Public, EAccountType::Clan);

    assert_eq!(4321u32, sid.account_id());
    assert_eq!(EUniverse::Public, sid.account_universe());
    assert_eq!(EAccountType::Clan, sid.account_type());
    assert_eq!(0u32, sid.account_instance());
}

/// The last case is a game server, which has no Steam2 form, so it renders as
/// the numeric id.
#[test]
fn steam2_render_is_valid() {
    let mut sid = SteamID::from_id(76561197969249708);

    assert_eq!("STEAM_0:0:4491990", sid.render(false));

    sid.set_account_universe(EUniverse::Beta);
    assert_eq!("STEAM_2:0:4491990", sid.render(false));

    sid.set_account_type(EAccountType::GameServer);
    assert_eq!("157625991261918636", sid.render(false));
}

#[test]
fn renders_steam3_by_default() {
    let sid = SteamID::from_id(76561197969249708);

    assert_eq!("[U:1:8983980]", sid.render(true));
    assert_eq!("[U:1:8983980]", sid.to_string());
}

#[test]
fn steam_ids_equality() {
    let sid = SteamID::from_id(76561197969249708);
    let sid2 = SteamID::from_id(76561197969249708);

    assert!(sid == sid2);
    assert_eq!(sid, sid2);

    let sid3 = SteamID::from_id(12345);

    assert!(sid != sid3);

    let sid4: u64 = u64::from(sid);
    assert!(sid4 == sid.convert_to_u64());
}

/// A `SteamID` hashes the same as its `u64` value.
#[test]
fn steam_id_hash_code_uses_long_hash_code() {
    let sid = SteamID::from_id(172376458626834);
    let long_value: u64 = 172376458626834;

    let mut sid_hasher = std::collections::hash_map::DefaultHasher::new();
    sid.hash(&mut sid_hasher);

    let mut long_hasher = std::collections::hash_map::DefaultHasher::new();
    long_value.hash(&mut long_hasher);

    assert_eq!(sid_hasher.finish(), long_hasher.finish());
}

#[test]
fn initializes_instances_correctly() {
    let mut sid = SteamID::new();

    assert!(sid.set_from_steam3_string("[g:1:1234]"));
    assert_eq!(EUniverse::Public, sid.account_universe());
    assert_eq!(EAccountType::Clan, sid.account_type());
    assert_eq!(0u32, sid.account_instance());
    assert_eq!(1234u32, sid.account_id());

    assert!(sid.set_from_steam3_string("[T:1:1234]"));
    assert_eq!(EUniverse::Public, sid.account_universe());
    assert_eq!(EAccountType::Chat, sid.account_type());
    assert_eq!(0u32, sid.account_instance());
    assert_eq!(1234u32, sid.account_id());

    assert!(sid.set_from_steam3_string("[c:1:1234]"));
    assert_eq!(EUniverse::Public, sid.account_universe());
    assert_eq!(EAccountType::Chat, sid.account_type());
    assert_eq!(ChatInstanceFlags::Clan.value(), sid.account_instance());
    assert_eq!(1234u32, sid.account_id());

    assert!(sid.set_from_steam3_string("[L:1:1234]"));
    assert_eq!(EUniverse::Public, sid.account_universe());
    assert_eq!(EAccountType::Chat, sid.account_type());
    assert_eq!(ChatInstanceFlags::Lobby.value(), sid.account_instance());
    assert_eq!(1234u32, sid.account_id());
}

/// Account type 15 doesn't fit the generated enum, so the id is built from
/// its raw form: universe 2, account type 15, desktop instance, account id 123.
#[test]
fn renders_out_of_range_account_type_as_lowercase_i() {
    let sid = SteamID::from_id((2u64 << 56) | (15u64 << 52) | (1u64 << 32) | 123u64);

    assert_eq!("[i:2:123]", sid.render(true));
}

#[test]
fn to_chat_id_converts_well_known_id() {
    let clan_id = SteamID::from_parts(EAccountType::Clan, EUniverse::Public, 4);
    let expected_chat_id = 110338190870577156u64;

    let chat_id = match clan_id.try_to_chat_id() {
        Ok(chat_id) => chat_id,
        Err(error) => panic!("clan id should convert to a chat id: {error}"),
    };

    assert_eq!(expected_chat_id, chat_id.convert_to_u64());
}

#[test]
fn to_chat_id_does_not_modify_self() {
    let mut clan_id = SteamID::from_parts(EAccountType::Clan, EUniverse::Public, 4);

    let _ = clan_id.try_to_chat_id();

    assert_eq!(EUniverse::Public, clan_id.account_universe());
    assert_eq!(EAccountType::Clan, clan_id.account_type());
    assert_eq!(0u32, clan_id.account_instance());
    assert_eq!(4u32, clan_id.account_id());
}

#[test]
fn to_chat_id_only_supports_clans_anon_game_server() {
    let id = SteamID::from_parts(EAccountType::AnonGameServer, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_anon_user() {
    let id = SteamID::from_parts(EAccountType::AnonUser, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_chat() {
    let id = SteamID::from_parts(EAccountType::Chat, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_console_user() {
    let id = SteamID::from_parts(EAccountType::ConsoleUser, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_content_server() {
    let id = SteamID::from_parts(EAccountType::ContentServer, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_game_server() {
    let id = SteamID::from_parts(EAccountType::GameServer, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_individual() {
    let id = SteamID::from_parts(EAccountType::Individual, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_multiseat() {
    let id = SteamID::from_parts(EAccountType::Multiseat, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn to_chat_id_only_supports_clans_pending() {
    let id = SteamID::from_parts(EAccountType::Pending, EUniverse::Public, 1);

    assert!(
        id.try_to_chat_id().is_err(),
        "only clan ids can be converted to chat ids"
    );
}

#[test]
fn try_get_clan_id_converts_well_known_id() {
    let clan_id = SteamID::from_parts_instanced(
        EAccountType::Chat,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    let group_id = match clan_id.try_get_clan_id() {
        Some(group_id) => group_id,
        None => panic!("clan chat id should convert to a clan id"),
    };

    assert_eq!(103582791429521412u64, group_id.convert_to_u64());
}

#[test]
fn try_get_clan_id_does_not_modify_self() {
    let mut clan_id = SteamID::from_parts_instanced(
        EAccountType::Chat,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(clan_id.try_get_clan_id().is_some());

    assert_eq!(EUniverse::Public, clan_id.account_universe());
    assert_eq!(EAccountType::Chat, clan_id.account_type());
    assert_eq!(ChatInstanceFlags::Clan.value(), clan_id.account_instance());
    assert_eq!(4u32, clan_id.account_id());
}

#[test]
fn try_get_clan_id_returns_false_for_ad_hoc_chat_room() {
    let chat_id = SteamID::from_id(108093571196988453);

    assert!(
        chat_id.try_get_clan_id().is_none(),
        "an ad hoc chat room has no clan id"
    );
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_anon_game_server() {
    let id = SteamID::from_parts_instanced(
        EAccountType::AnonGameServer,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_anon_user() {
    let id = SteamID::from_parts_instanced(
        EAccountType::AnonUser,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_console_user() {
    let id = SteamID::from_parts_instanced(
        EAccountType::ConsoleUser,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_content_server() {
    let id = SteamID::from_parts_instanced(
        EAccountType::ContentServer,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_game_server() {
    let id = SteamID::from_parts_instanced(
        EAccountType::GameServer,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_individual() {
    let id = SteamID::from_parts_instanced(
        EAccountType::Individual,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_multiseat() {
    let id = SteamID::from_parts_instanced(
        EAccountType::Multiseat,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn try_get_clan_id_only_supports_clan_chat_rooms_pending() {
    let id = SteamID::from_parts_instanced(
        EAccountType::Pending,
        EUniverse::Public,
        4,
        ChatInstanceFlags::Clan.value(),
    );

    assert!(id.try_get_clan_id().is_none());
}

#[test]
fn known_account_types_are_valid_individual() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::Individual);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_multiseat() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::Multiseat);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_game_server() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::GameServer);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_anon_game_server() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::AnonGameServer);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_pending() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::Pending);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_content_server() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::ContentServer);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_clan() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::Clan);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_chat() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::Chat);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_console_user() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::ConsoleUser);

    assert!(sid.is_valid());
}

#[test]
fn known_account_types_are_valid_anon_user() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_instance(0); // for Clan to pass
    sid.set_account_type(EAccountType::AnonUser);

    assert!(sid.is_valid());
}

#[test]
fn unknown_account_types_are_invalid_invalid() {
    let sid = steam_id_with_raw(0, 1);

    assert!(!sid.is_valid());
}

#[test]
fn unknown_account_types_are_invalid_11() {
    let sid = steam_id_with_raw(11, 1);

    assert!(!sid.is_valid());
}

#[test]
fn unknown_account_types_are_invalid_12() {
    let sid = steam_id_with_raw(12, 1);

    assert!(!sid.is_valid());
}

#[test]
fn unknown_account_types_are_invalid_13() {
    let sid = steam_id_with_raw(13, 1);

    assert!(!sid.is_valid());
}

#[test]
fn known_account_universes_are_valid_public() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_universe(EUniverse::Public);

    assert!(sid.is_valid());
}

#[test]
fn known_account_universes_are_valid_beta() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_universe(EUniverse::Beta);

    assert!(sid.is_valid());
}

#[test]
fn known_account_universes_are_valid_internal() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_universe(EUniverse::Internal);

    assert!(sid.is_valid());
}

#[test]
fn known_account_universes_are_valid_dev() {
    let mut sid = SteamID::from_id(BASE_ID);
    sid.set_account_universe(EUniverse::Dev);

    assert!(sid.is_valid());
}

#[test]
fn unknown_account_universes_are_invalid_invalid() {
    let sid = steam_id_with_raw(1, 0);

    assert!(!sid.is_valid());
}

#[test]
fn unknown_account_universes_are_invalid_5() {
    let sid = steam_id_with_raw(1, 5);

    assert!(!sid.is_valid());
}

#[test]
fn unknown_account_universes_are_invalid_6() {
    let sid = steam_id_with_raw(1, 6);

    assert!(!sid.is_valid());
}

#[test]
fn unknown_account_universes_are_invalid_7() {
    let sid = steam_id_with_raw(1, 7);

    assert!(!sid.is_valid());
}

/// If this enum has changed, update `SteamID::is_valid`.
#[test]
fn e_universe_enum_has_not_changed() {
    assert_eq!(5, EUniverse::ALL.len());
}

/// If this enum has changed, update `SteamID::is_valid`.
#[test]
fn e_account_type_enum_has_not_changed() {
    assert_eq!(11, EAccountType::ALL.len());
}
