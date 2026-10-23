//! Matchmaking lobby messages, whose lobby metadata is binary key values.

use std::io::Cursor;

use steamkit::types::key_value::KeyValue;
use steamkit_protos::steam::{
    CMsgClientMmsCreateLobby, CMsgClientMmsJoinLobbyResponse, CMsgClientMmsLobbyChatMsg,
    CMsgClientMmsLobbyData, CMsgClientMmsSendLobbyChatMsg, CMsgClientMmsSetLobbyData,
};

use crate::nethook::body::Body;
use crate::nethook::render::TreeNode;
use crate::nethook::specializations::{ExtraObject, Specialization, SpecializationContext};
use crate::nethook::well_known_app_ids as app;

/// The magic number of the binary key value format of Source 2.
const VBKV_MAGIC: u32 = 0x564B_4256;

/// Decodes the metadata of a matchmaking lobby message.
pub struct MmsLobbyMessageSpecialization;

impl Specialization for MmsLobbyMessageSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        _context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let data = match lobby_data(body) {
            Some(data) => data,
            None => return Vec::new(),
        };

        match read_binary_key_values(&data) {
            Some(node) => vec![ExtraObject::new("Lobby Metadata", node)],
            None => Vec::new(),
        }
    }
}

/// The metadata of a matchmaking lobby message.
fn lobby_data(body: &Body) -> Option<Vec<u8>> {
    if let Some(message) = body.decode_as::<CMsgClientMmsSetLobbyData>() {
        return message.metadata_kv;
    }

    if let Some(message) = body.decode_as::<CMsgClientMmsLobbyData>() {
        return message.metadata_kv;
    }

    if let Some(message) = body.decode_as::<CMsgClientMmsJoinLobbyResponse>() {
        return message.metadata_kv;
    }

    if let Some(message) = body.decode_as::<CMsgClientMmsCreateLobby>() {
        return message.metadata_kv;
    }

    if let Some(message) = body.decode_as::<CMsgClientMmsSendLobbyChatMsg>() {
        return chat_data(message.app_id.unwrap_or_default(), message.lobby_message);
    }

    if let Some(message) = body.decode_as::<CMsgClientMmsLobbyChatMsg>() {
        return chat_data(message.app_id.unwrap_or_default(), message.lobby_message);
    }

    None
}

/// The lobby message of a chat message.
///
/// The chat messages of Counter Strike 2 and Dota 2 start with the build
/// version of the engine, which is not part of the key values.
fn chat_data(app_id: u32, lobby_message: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let data = lobby_message?;

    if app_id == app::COUNTER_STRIKE_2 || app_id == app::DOTA_2 {
        if data.len() < 4 {
            return None;
        }
        return Some(data[4..].to_vec());
    }

    None
}

/// Reads the binary key values of a lobby metadata blob.
fn read_binary_key_values(data: &[u8]) -> Option<TreeNode> {
    let mut cursor = Cursor::new(data);

    if data.len() >= 4 {
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[..4]);
        if u32::from_le_bytes(magic) == VBKV_MAGIC {
            cursor.set_position(8);
        }
    }

    let mut key_value = KeyValue::new();
    if !key_value.try_read_as_binary(&mut cursor) {
        return None;
    }

    Some(key_value_node(&key_value))
}

/// Builds the node of a binary key value.
fn key_value_node(key_value: &KeyValue) -> TreeNode {
    let mut node = TreeNode::new(
        key_value.name().unwrap_or_default().to_owned(),
        key_value.value().map(|value| value.to_owned()),
    );

    for child in key_value.children() {
        node.children.push(key_value_node(child));
    }

    node
}
