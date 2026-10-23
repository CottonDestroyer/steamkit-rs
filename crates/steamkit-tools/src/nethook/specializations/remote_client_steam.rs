//! Remote play notifications, which carry either an encrypted Steam message or
//! a broadcast header and body.

use prost::Message as _;
use steamkit::util::crypto_helper::CryptoHelper;
use steamkit_protos::steam::CMsgRemoteClientBroadcastHeader;
use steamkit_protos::webui::{
    CRemoteClientSteamBroadcastNotification, CRemoteClientSteamToSteamNotification,
};

use crate::nethook::body::Body;
use crate::nethook::message_reading;
use crate::nethook::proto_registry;
use crate::nethook::proto_registry::ProtobufDecoder;
use crate::nethook::render::{body_node, debug_node, protobuf_node, TreeNode};
use crate::nethook::specializations::{ExtraObject, Specialization, SpecializationContext};

/// The magic number that starts a broadcast payload.
const BROADCAST_MAGIC: u32 = 0xA05F_4C21;

/// Broadcast body types, looked up by name.
const BROADCAST_BODY_TYPES: &[(&str, ProtobufDecoder)] = &[
    (
        "CMsgRemoteClientBroadcastDiscovery",
        proto_registry::decode_steam_CMsgRemoteClientBroadcastDiscovery,
    ),
    (
        "CMsgRemoteClientBroadcastStatus",
        proto_registry::decode_steam_CMsgRemoteClientBroadcastStatus,
    ),
    (
        "CMsgRemoteClientBroadcastClientIdDeconflict",
        proto_registry::decode_steam_CMsgRemoteClientBroadcastClientIdDeconflict,
    ),
    (
        "CMsgRemoteClientBroadcastClientPairingState",
        proto_registry::decode_steam_CMsgRemoteClientBroadcastClientPairingState,
    ),
    (
        "CMsgRemoteClientBroadcastClientPairingExclusivity",
        proto_registry::decode_steam_CMsgRemoteClientBroadcastClientPairingExclusivity,
    ),
];

/// Decrypts the Steam message a remote play notification carries.
pub struct RemoteClientSteamToSteamSpecialization;

impl Specialization for RemoteClientSteamToSteamSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let notification = match body.decode_as::<CRemoteClientSteamToSteamNotification>() {
            Some(notification) => notification,
            None => return Vec::new(),
        };

        let secret_id = notification.secretid.unwrap_or_default() as i32;
        let auth_secret = match context.dump.account_auth_secret(secret_id) {
            Some(secret) => secret,
            None => return Vec::new(),
        };

        let encrypted = notification.encrypted_payload.clone().unwrap_or_default();
        let decrypted = match CryptoHelper::symmetric_decrypt(&encrypted, auth_secret) {
            Ok(decrypted) => decrypted,
            Err(_) => return Vec::new(),
        };

        let message = match message_reading::read_message(&decrypted) {
            Ok(message) => message,
            Err(_) => return Vec::new(),
        };

        let mut objects = Vec::new();

        objects.push(ExtraObject::new(
            "Decrypted Header",
            debug_node(
                "Decrypted Header",
                &format!("{:?}", message.header),
                context.show_unset_fields,
            ),
        ));

        objects.push(ExtraObject::new(
            "Decrypted Body",
            body_node(&message.body, context.show_unset_fields),
        ));

        if !message.payload.is_empty() {
            objects.push(ExtraObject::new(
                "Decrypted Payload",
                TreeNode::new("Decrypted Payload", Some(hex::encode(&message.payload).to_uppercase())),
            ));
        }

        objects
    }
}

/// Decodes the broadcast a remote play notification carries.
pub struct RemoteClientSteamBroadcastSpecialization;

impl Specialization for RemoteClientSteamBroadcastSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let broadcast = match body.decode_as::<CRemoteClientSteamBroadcastNotification>() {
            Some(broadcast) => broadcast,
            None => return Vec::new(),
        };

        let payload = broadcast.payload.clone().unwrap_or_default();
        let mut offset = 0usize;

        if read_u32(&payload, &mut offset) != Some(0xFFFF_FFFF) {
            return Vec::new();
        }

        if read_u32(&payload, &mut offset) != Some(BROADCAST_MAGIC) {
            return Vec::new();
        }

        let header_length = match read_i32(&payload, &mut offset) {
            Some(length) if length >= 0 => length as usize,
            _ => return Vec::new(),
        };

        let header_bytes = match take(&payload, &mut offset, header_length) {
            Some(bytes) => bytes,
            None => return Vec::new(),
        };

        let header = match CMsgRemoteClientBroadcastHeader::decode(header_bytes) {
            Ok(header) => header,
            Err(_) => return Vec::new(),
        };

        let mut objects = vec![ExtraObject::new(
            "Broadcast Header",
            debug_node(
                "Broadcast Header",
                &format!("{header:?}"),
                context.show_unset_fields,
            ),
        )];

        let body_length = match read_i32(&payload, &mut offset) {
            Some(length) if length >= 0 => length as usize,
            _ => return objects,
        };

        let body_bytes = match take(&payload, &mut offset, body_length) {
            Some(bytes) => bytes,
            None => return objects,
        };

        let type_name = broadcast_body_type_name(header.msg_type.unwrap_or_default());
        let decoder = BROADCAST_BODY_TYPES
            .iter()
            .find(|(name, _)| *name == type_name)
            .map(|(_, decoder)| *decoder);

        let node = match decoder {
            Some(decoder) => match decoder(body_bytes) {
                Ok(message) => protobuf_node(&type_name, &*message, context.show_unset_fields),
                Err(_) => TreeNode::new("Broadcast Body", Some(hex::encode(body_bytes).to_uppercase())),
            },
            None => TreeNode::new("Broadcast Body", Some(hex::encode(body_bytes).to_uppercase())),
        };

        objects.push(ExtraObject::new("Broadcast Body", node));

        objects
    }
}

/// The name of the body type of a broadcast message.
///
/// `<name>` becomes `CMsgBroadcast<name>`.
fn broadcast_body_type_name(msg_type: i32) -> String {
    let name = match msg_type {
        0 => "k_ERemoteClientBroadcastMsgDiscovery",
        1 => "k_ERemoteClientBroadcastMsgStatus",
        2 => "k_ERemoteClientBroadcastMsgOffline",
        11 => "k_ERemoteClientBroadcastMsgClientIDDeconflict",
        15 => "k_ERemoteClientBroadcastMsgPairingState",
        16 => "k_ERemoteClientBroadcastMsgPairingExclusivity",
        _ => return String::new(),
    };

    name.replace("k_E", "CMsg").replace("BroadcastMsg", "Broadcast")
}

/// Reads a four byte number.
fn read_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let value = read_i32(bytes, offset)?;
    Some(value as u32)
}

/// Reads a four byte signed number.
fn read_i32(bytes: &[u8], offset: &mut usize) -> Option<i32> {
    if *offset + 4 > bytes.len() {
        return None;
    }

    let value = i32::from_le_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]);
    *offset += 4;
    Some(value)
}

/// Takes `count` bytes from the buffer.
fn take<'a>(bytes: &'a [u8], offset: &mut usize, count: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(count)?;
    if end > bytes.len() {
        return None;
    }

    let taken = &bytes[*offset..end];
    *offset = end;
    Some(taken)
}
