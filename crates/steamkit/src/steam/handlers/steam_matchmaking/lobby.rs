//! Lobbies, lobby members and lobby list filters.
//!
//! The filters implement the [`Filter`] trait. Metadata maps are `BTreeMap`s
//! so the encoded key-value pairs come out in a stable order.

use std::collections::BTreeMap;
use std::io::Cursor;

use steamkit_protos::steam::{c_msg_client_mms_get_lobby_list, MmsKeyValuePair};

use crate::base::generated::steam_language::{
    ELobbyComparison, ELobbyDistanceFilter, ELobbyFilterType, ELobbyType,
};
use crate::error::SteamError;
use crate::types::key_value::KeyValue;
use crate::types::steam_id::SteamID;

/// The protobuf fields every filter shares.
fn base_filter(
    filter_type: ELobbyFilterType,
    key: &str,
    comparison: ELobbyComparison,
) -> c_msg_client_mms_get_lobby_list::Filter {
    c_msg_client_mms_get_lobby_list::Filter {
        key: Some(key.to_string()),
        value: None,
        comparision: Some(comparison.value()),
        filter_type: Some(filter_type.value()),
    }
}

/// A lobby list filter.
pub trait Filter {
    /// The type of the filter.
    fn filter_type(&self) -> ELobbyFilterType;

    /// The metadata key this filter is about. Empty for some filters, e.g. the
    /// distance filter.
    fn key(&self) -> &str;

    /// The comparison method used by this filter.
    fn comparison(&self) -> ELobbyComparison;

    /// The filter's protobuf form.
    fn serialize(&self) -> c_msg_client_mms_get_lobby_list::Filter;
}

/// Filters lobbies by distance, based on Steam's IP geolocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistanceFilter {
    value: ELobbyDistanceFilter,
}

impl DistanceFilter {
    /// Creates a new distance filter.
    pub fn new(value: ELobbyDistanceFilter) -> Self {
        Self { value }
    }

    /// The Steam distance filter value.
    pub fn value(&self) -> ELobbyDistanceFilter {
        self.value
    }
}

impl Filter for DistanceFilter {
    fn filter_type(&self) -> ELobbyFilterType {
        ELobbyFilterType::Distance
    }

    fn key(&self) -> &str {
        ""
    }

    fn comparison(&self) -> ELobbyComparison {
        ELobbyComparison::Equal
    }

    fn serialize(&self) -> c_msg_client_mms_get_lobby_list::Filter {
        let mut filter = base_filter(self.filter_type(), self.key(), self.comparison());
        filter.value = Some(self.value.value().to_string());
        filter
    }
}

/// Filters lobbies by how close a metadata value is to a given value. With
/// several near filters, earlier ones take precedence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearValueFilter {
    key: String,
    value: i32,
}

impl NearValueFilter {
    /// Creates a new near value filter.
    pub fn new(key: impl Into<String>, value: i32) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    /// The integer value that the lobbies' metadata value should be close to.
    pub fn value(&self) -> i32 {
        self.value
    }
}

impl Filter for NearValueFilter {
    fn filter_type(&self) -> ELobbyFilterType {
        ELobbyFilterType::NearValue
    }

    fn key(&self) -> &str {
        &self.key
    }

    fn comparison(&self) -> ELobbyComparison {
        ELobbyComparison::Equal
    }

    fn serialize(&self) -> c_msg_client_mms_get_lobby_list::Filter {
        let mut filter = base_filter(self.filter_type(), self.key(), self.comparison());
        filter.value = Some(self.value.to_string());
        filter
    }
}

/// Filters lobbies by comparing a metadata value to an integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumericalFilter {
    key: String,
    comparison: ELobbyComparison,
    value: i32,
}

impl NumericalFilter {
    /// Creates a new numerical filter.
    pub fn new(key: impl Into<String>, comparison: ELobbyComparison, value: i32) -> Self {
        Self {
            key: key.into(),
            comparison,
            value,
        }
    }

    /// The integer value to compare against.
    pub fn value(&self) -> i32 {
        self.value
    }
}

impl Filter for NumericalFilter {
    fn filter_type(&self) -> ELobbyFilterType {
        ELobbyFilterType::Numerical
    }

    fn key(&self) -> &str {
        &self.key
    }

    fn comparison(&self) -> ELobbyComparison {
        self.comparison
    }

    fn serialize(&self) -> c_msg_client_mms_get_lobby_list::Filter {
        let mut filter = base_filter(self.filter_type(), self.key(), self.comparison());
        filter.value = Some(self.value.to_string());
        filter
    }
}

/// Filters lobbies by the minimum number of free slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotsAvailableFilter {
    slots_available: i32,
}

impl SlotsAvailableFilter {
    /// Creates a new slots available filter.
    pub fn new(slots_available: i32) -> Self {
        Self { slots_available }
    }

    /// The minimum number of slots available in the lobby.
    pub fn slots_available(&self) -> i32 {
        self.slots_available
    }
}

impl Filter for SlotsAvailableFilter {
    fn filter_type(&self) -> ELobbyFilterType {
        ELobbyFilterType::SlotsAvailable
    }

    fn key(&self) -> &str {
        ""
    }

    fn comparison(&self) -> ELobbyComparison {
        ELobbyComparison::Equal
    }

    fn serialize(&self) -> c_msg_client_mms_get_lobby_list::Filter {
        let mut filter = base_filter(self.filter_type(), self.key(), self.comparison());
        filter.value = Some(self.slots_available.to_string());
        filter
    }
}

/// Filters lobbies by comparing a metadata value to a string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringFilter {
    key: String,
    comparison: ELobbyComparison,
    value: String,
}

impl StringFilter {
    /// Creates a new string filter.
    pub fn new(
        key: impl Into<String>,
        comparison: ELobbyComparison,
        value: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            comparison,
            value: value.into(),
        }
    }

    /// The string value to compare against.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl Filter for StringFilter {
    fn filter_type(&self) -> ELobbyFilterType {
        ELobbyFilterType::String
    }

    fn key(&self) -> &str {
        &self.key
    }

    fn comparison(&self) -> ELobbyComparison {
        self.comparison
    }

    fn serialize(&self) -> c_msg_client_mms_get_lobby_list::Filter {
        let mut filter = base_filter(self.filter_type(), self.key(), self.comparison());
        filter.value = Some(self.value.clone());
        filter
    }
}

/// A Steam user in a lobby.
#[derive(Clone, Debug, Default)]
pub struct Member {
    steam_id: SteamID,
    persona_name: String,
    metadata: BTreeMap<String, String>,
}

impl Member {
    /// Creates a lobby member.
    pub fn new(
        steam_id: SteamID,
        persona_name: impl Into<String>,
        metadata: Option<BTreeMap<String, String>>,
    ) -> Self {
        Self {
            steam_id,
            persona_name: persona_name.into(),
            metadata: metadata.unwrap_or_default(),
        }
    }

    /// The SteamID of the lobby member.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The Steam persona name of the lobby member.
    pub fn persona_name(&self) -> &str {
        &self.persona_name
    }

    /// The metadata attached to the lobby member.
    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }
}

/// Members are equal when their SteamIDs are.
impl PartialEq for Member {
    fn eq(&self, other: &Self) -> bool {
        self.steam_id == other.steam_id
    }
}

impl Eq for Member {}

impl std::hash::Hash for Member {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.steam_id.hash(state);
    }
}

/// A Steam lobby.
#[derive(Clone, Debug)]
pub struct Lobby {
    steam_id: SteamID,
    lobby_type: ELobbyType,
    lobby_flags: i32,
    owner_steam_id: Option<SteamID>,
    metadata: BTreeMap<String, String>,
    max_members: i32,
    num_members: i32,
    members: Vec<Member>,
    distance: Option<f32>,
    weight: Option<i64>,
}

impl Lobby {
    /// Creates a lobby.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        steam_id: SteamID,
        lobby_type: ELobbyType,
        lobby_flags: i32,
        owner_steam_id: Option<SteamID>,
        metadata: Option<BTreeMap<String, String>>,
        max_members: i32,
        num_members: i32,
        members: Option<Vec<Member>>,
        distance: Option<f32>,
        weight: Option<i64>,
    ) -> Self {
        Self {
            steam_id,
            lobby_type,
            lobby_flags,
            owner_steam_id,
            metadata: metadata.unwrap_or_default(),
            max_members,
            num_members,
            members: members.unwrap_or_default(),
            distance,
            weight,
        }
    }

    /// The SteamID of the lobby.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The type of the lobby.
    pub fn lobby_type(&self) -> ELobbyType {
        self.lobby_type
    }

    /// The lobby's flags.
    pub fn lobby_flags(&self) -> i32 {
        self.lobby_flags
    }

    /// The SteamID of the lobby's owner. Steam doesn't send owners for lobbies
    /// in a lobby list, so lobbies from `get_lobby_list` may not have one.
    pub fn owner_steam_id(&self) -> Option<SteamID> {
        self.owner_steam_id
    }

    /// The metadata of the lobby; string key-value pairs.
    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    /// The maximum number of members that can occupy the lobby.
    pub fn max_members(&self) -> i32 {
        self.max_members
    }

    /// The number of members that are currently occupying the lobby.
    pub fn num_members(&self) -> i32 {
        self.num_members
    }

    /// The lobby's members. Only filled in for the user's current lobby.
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// The distance of the lobby.
    pub fn distance(&self) -> Option<f32> {
        self.distance
    }

    /// The weight of the lobby.
    pub fn weight(&self) -> Option<i64> {
        self.weight
    }

    /// Encodes lobby metadata as protobuf key-value pairs.
    pub(crate) fn encode_metadata(
        metadata: Option<&BTreeMap<String, String>>,
    ) -> Vec<MmsKeyValuePair> {
        let Some(metadata) = metadata else {
            return Vec::new();
        };

        metadata
            .iter()
            .map(|(key, value)| MmsKeyValuePair {
                name: Some(key.clone()),
                value: Some(value.clone()),
            })
            .collect()
    }

    /// Decodes lobby metadata from the key-value pair list, falling back to the
    /// legacy binary [`KeyValue`] blob if the list is empty.
    pub(crate) fn decode_metadata(
        pairs: &[MmsKeyValuePair],
        legacy_buffer: Option<&[u8]>,
    ) -> Result<BTreeMap<String, String>, SteamError> {
        if pairs.is_empty() {
            return Self::decode_metadata_buffer(legacy_buffer);
        }

        let mut metadata = BTreeMap::new();

        for pair in pairs {
            let Some(name) = pair.name.as_deref() else {
                continue;
            };

            metadata.insert(name.to_string(), pair.value.clone().unwrap_or_default());
        }

        Ok(metadata)
    }

    /// Decodes the legacy binary lobby metadata blob.
    pub(crate) fn decode_metadata_buffer(
        buffer: Option<&[u8]>,
    ) -> Result<BTreeMap<String, String>, SteamError> {
        let Some(buffer) = buffer else {
            return Ok(BTreeMap::new());
        };

        if buffer.is_empty() {
            return Ok(BTreeMap::new());
        }

        let mut key_value = KeyValue::new();
        let mut cursor = Cursor::new(buffer);

        if !key_value.try_read_as_binary(&mut cursor) {
            return Err(SteamError::Malformed(
                "Lobby metadata is of an unexpected format".to_string(),
            ));
        }

        let mut metadata = BTreeMap::new();

        for value in key_value.children() {
            let (Some(name), Some(value)) = (value.name(), value.value()) else {
                continue;
            };

            metadata.insert(name.to_string(), value.to_string());
        }

        Ok(metadata)
    }
}
