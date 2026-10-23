//! Names for game coordinator message numbers.

use steamkit::util::msg_util::MsgUtil;

use crate::nethook::well_known_app_ids as app;

/// The bit that flags a message as being protobuf backed.
const PROTO_MASK: u32 = 0x8000_0000;

/// Looks the name of a game coordinator message up.
///
/// Walks the enums of the game in a fixed order and returns the name from the
/// first one that has the number. The names are the ones from the proto
/// files, for example `k_EMsgGCClientHello`.
pub fn gc_message_name(raw_emsg: u32, app_id: u32) -> String {
    let value = (raw_emsg & !PROTO_MASK) as i32;

    let name = match app_id {
        app::TEAM_FORTRESS_2 => tf2_message_name(value),
        app::DOTA_2 => dota2_message_name(value),
        app::COUNTER_STRIKE_2 => csgo_message_name(value),
        app::DEADLOCK => deadlock_message_name(value),
        _ => None,
    };

    name.map(str::to_owned).unwrap_or_else(|| value.to_string())
}

/// The name of a Team Fortress 2 game coordinator message.
fn tf2_message_name(value: i32) -> Option<&'static str> {
    use steamkit_protos::tf2::{
        EgcBaseClientMsg, EgcBaseMsg, EgcItemMsg, EgcSystemMsg, EsoMsg, EtfgcMsg,
    };

    EtfgcMsg::try_from(value)
        .ok()
        .map(|member| member.as_str_name())
        .or_else(|| EgcBaseMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EsoMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcSystemMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcItemMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcBaseClientMsg::try_from(value).ok().map(|member| member.as_str_name()))
}

/// The name of a Dota 2 game coordinator message.
fn dota2_message_name(value: i32) -> Option<&'static str> {
    use steamkit_protos::dota2::{EdotagcMsg, EgcBaseClientMsg, EgcBaseMsg, EgcItemMsg, EsoMsg};

    EdotagcMsg::try_from(value)
        .ok()
        .map(|member| member.as_str_name())
        .or_else(|| EgcBaseMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EsoMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcItemMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcBaseClientMsg::try_from(value).ok().map(|member| member.as_str_name()))
}

/// The name of a Counter Strike 2 game coordinator message.
fn csgo_message_name(value: i32) -> Option<&'static str> {
    use steamkit_protos::csgo::{
        ECsgoGcMsg, EgcBaseClientMsg, EgcBaseMsg, EgcItemMsg, EgcSystemMsg, EsoMsg,
    };

    ECsgoGcMsg::try_from(value)
        .ok()
        .map(|member| member.as_str_name())
        .or_else(|| EgcBaseMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EsoMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcItemMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcSystemMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcBaseClientMsg::try_from(value).ok().map(|member| member.as_str_name()))
}

/// The name of a Deadlock game coordinator message.
fn deadlock_message_name(value: i32) -> Option<&'static str> {
    use steamkit_protos::deadlock::{
        EgcBaseClientMsg, EgcBaseMsg, EgcCitadelClientMessages, EgcItemMsg, EsoMsg,
    };

    EgcCitadelClientMessages::try_from(value)
        .ok()
        .map(|member| member.as_str_name())
        .or_else(|| EgcBaseMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EsoMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcItemMsg::try_from(value).ok().map(|member| member.as_str_name()))
        .or_else(|| EgcBaseClientMsg::try_from(value).ok().map(|member| member.as_str_name()))
}

/// The game coordinator message number of a raw message number.
pub fn gc_message_number(raw_emsg: u32) -> u32 {
    MsgUtil::get_gc_msg(raw_emsg)
}
