//! Works out the body type of a captured message.
//!
//! * Protobuf messages: an override from [`message_type_overrides`] if there
//!   is one, otherwise the first Steam protobuf type whose name ends with the
//!   message name (see [`crate::nethook::proto_registry`]).
//! * Non-protobuf messages: the wire format struct with the matching message
//!   number.
//! * Game coordinator messages: the message name is reduced to a suffix and
//!   every type of the game that ends with it is a candidate.

use steamkit::base::generated::steam_language::EMsg;
use steamkit::util::msg_util::MsgUtil;

use crate::nethook::emsg_extensions;
use crate::nethook::message_type_overrides;
use crate::nethook::proto_registry::{self, ProtobufDecoder};
use crate::nethook::serializable_registry::{self, SerializableDecoder};

/// A protobuf body type.
#[derive(Clone, Copy, Debug)]
pub struct ProtobufType {
    /// The name of the protobuf type.
    pub name: &'static str,
    /// The decoder of the type.
    pub decoder: ProtobufDecoder,
}

/// A body type of the SteamKit wire format that is not protobuf backed.
#[derive(Clone, Copy, Debug)]
pub struct SerializableType {
    /// The name of the message type.
    pub name: &'static str,
    /// The decoder of the message type.
    pub decoder: SerializableDecoder,
}

/// The protobuf body type of a message number.
pub fn protobuf_message_body_type(emsg: EMsg) -> Option<ProtobufType> {
    if let Some(known) = message_type_overrides::body_map(emsg) {
        return Some(known);
    }

    proto_registry::protobuf_body_decoder(emsg)
        .map(|(name, decoder)| ProtobufType { name, decoder })
}

/// The body type of a message number that is not protobuf backed.
pub fn non_protobuf_message_body_type(emsg: EMsg) -> Option<SerializableType> {
    serializable_registry::non_protobuf_message_body_type(emsg)
        .map(|(name, decoder)| SerializableType { name, decoder })
}

/// The candidate game coordinator body types of a message number.
///
/// Every type of the game whose name ends with the reduced game coordinator
/// message name. The caller tries them in order and uses the first one that
/// decodes.
pub fn gc_message_body_type_candidates(raw_emsg: u32, gc_app_id: u32) -> Vec<ProtobufType> {
    let gc_emsg = MsgUtil::get_gc_msg(raw_emsg);

    if let Some(known) = message_type_overrides::gc_body_map(gc_app_id, gc_emsg) {
        return vec![known];
    }

    let message_name = emsg_extensions::gc_message_name(raw_emsg, gc_app_id);
    let suffix = gc_type_suffix(&message_name);
    if suffix.is_empty() {
        return Vec::new();
    }

    let suffix = suffix.to_lowercase();

    let mut candidates = Vec::new();
    for (name, decoder) in proto_registry::BASE_GC_TYPES
        .iter()
        .chain(gc_types_of(gc_app_id))
    {
        if name.to_lowercase().ends_with(&suffix) {
            candidates.push(ProtobufType {
                name: *name,
                decoder: *decoder,
            });
        }
    }

    candidates
}

/// The message types of a game coordinator namespace.
fn gc_types_of(gc_app_id: u32) -> &'static [(&'static str, ProtobufDecoder)] {
    use crate::nethook::well_known_app_ids as app;

    match gc_app_id {
        app::TEAM_FORTRESS_2 => proto_registry::TF2_GC_TYPES,
        app::DOTA_2 => proto_registry::DOTA2_GC_TYPES,
        app::COUNTER_STRIKE_2 => proto_registry::CSGO_GC_TYPES,
        app::DEADLOCK => proto_registry::DEADLOCK_GC_TYPES,
        _ => &[],
    }
}

/// The type name suffix for a game coordinator message name.
///
/// Strips `k_`, `ESOMsg`, leading `_`, `EMsg`, and then every leading `G` and
/// `C`. So `k_EMsgGCClientHello` becomes `lientHello`; it over-strips, but
/// the suffix still matches `CMsgClientHello`.
pub fn gc_type_suffix(gc_message_name: &str) -> String {
    let mut name = gc_message_name.replace("k_", "");
    name = name.replace("ESOMsg", "");
    name = name.trim_start_matches('_').to_owned();
    name = name.replace("EMsg", "");
    name = name
        .trim_start_matches(|character| character == 'G' || character == 'C')
        .to_owned();
    name
}
