//! The local cache of lobbies Steam told us about, by app id and then by lobby
//! SteamID.
//!
//! The inner maps are `Arc<DashMap<..>>`s, so a lookup never holds two shard
//! locks at once.

use std::sync::Arc;

use dashmap::DashMap;

use crate::types::steam_id::SteamID;

use super::lobby::{Lobby, Member};

/// The lobbies known locally.
#[derive(Debug, Default)]
pub struct LobbyCache {
    lobbies: DashMap<u32, Arc<DashMap<SteamID, Lobby>>>,
}

impl LobbyCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            lobbies: DashMap::new(),
        }
    }

    /// The cached lobby, if it's known.
    pub fn get_lobby(&self, app_id: u32, lobby_steam_id: SteamID) -> Option<Lobby> {
        self.app_lobbies(app_id)
            .get(&lobby_steam_id)
            .map(|lobby| lobby.clone())
    }

    /// Stores a lobby in the cache.
    pub fn cache_lobby(&self, app_id: u32, lobby: Lobby) {
        self.app_lobbies(app_id).insert(lobby.steam_id(), lobby);
    }

    /// Adds a member to a lobby. Returns `None` if the member was already in it.
    pub fn add_lobby_member(
        &self,
        app_id: u32,
        lobby: &Lobby,
        member_id: SteamID,
        persona_name: &str,
    ) -> Option<Member> {
        if lobby
            .members()
            .iter()
            .any(|member| member.steam_id() == member_id)
        {
            // Already in the lobby.
            return None;
        }

        let added_member = Member::new(member_id, persona_name, None);

        let mut members = Vec::with_capacity(lobby.members().len() + 1);
        members.extend_from_slice(lobby.members());
        members.push(added_member.clone());

        self.update_lobby_members(app_id, lobby, Some(members));

        Some(added_member)
    }

    /// Removes a member from a lobby. Returns `None` if the member wasn't in it.
    /// When the last member leaves, the lobby is deleted, like Steam does.
    pub fn remove_lobby_member(
        &self,
        app_id: u32,
        lobby: &Lobby,
        member_id: SteamID,
    ) -> Option<Member> {
        let removed_member = lobby
            .members()
            .iter()
            .find(|member| member.steam_id() == member_id)
            .cloned()?;

        let members: Vec<Member> = lobby
            .members()
            .iter()
            .filter(|member| member.steam_id() != removed_member.steam_id())
            .cloned()
            .collect();

        if !members.is_empty() {
            self.update_lobby_members(app_id, lobby, Some(members));
        } else {
            // Steam deletes lobbies without members.
            self.delete_lobby(app_id, lobby.steam_id());
        }

        Some(removed_member)
    }

    /// Removes every member from a lobby.
    pub fn clear_lobby_members(&self, app_id: u32, lobby_steam_id: SteamID) {
        let Some(lobby) = self.get_lobby(app_id, lobby_steam_id) else {
            return;
        };

        self.update_lobby_members_with_owner(app_id, &lobby, lobby.owner_steam_id(), None);
    }

    /// Changes the owner of a lobby.
    pub fn update_lobby_owner(&self, app_id: u32, lobby_steam_id: SteamID, owner_steam_id: SteamID) {
        let Some(lobby) = self.get_lobby(app_id, lobby_steam_id) else {
            return;
        };

        let members = lobby.members().to_vec();
        self.update_lobby_members_with_owner(app_id, &lobby, Some(owner_steam_id), Some(members));
    }

    /// Replaces the member list of a lobby, keeping its owner.
    pub fn update_lobby_members(
        &self,
        app_id: u32,
        lobby: &Lobby,
        members: Option<Vec<Member>>,
    ) {
        self.update_lobby_members_with_owner(app_id, lobby, lobby.owner_steam_id(), members);
    }

    /// Removes every cached lobby.
    pub fn clear(&self) {
        self.lobbies.clear();
    }

    /// Stores a new version of a lobby.
    fn update_lobby_members_with_owner(
        &self,
        app_id: u32,
        lobby: &Lobby,
        owner: Option<SteamID>,
        members: Option<Vec<Member>>,
    ) {
        self.cache_lobby(
            app_id,
            Lobby::new(
                lobby.steam_id(),
                lobby.lobby_type(),
                lobby.lobby_flags(),
                owner,
                Some(lobby.metadata().clone()),
                lobby.max_members(),
                lobby.num_members(),
                members,
                lobby.distance(),
                lobby.weight(),
            ),
        );
    }

    /// The lobby map of an app, created if needed.
    fn app_lobbies(&self, app_id: u32) -> Arc<DashMap<SteamID, Lobby>> {
        self.lobbies
            .entry(app_id)
            .or_insert_with(|| Arc::new(DashMap::new()))
            .clone()
    }

    /// Removes a lobby from the cache.
    fn delete_lobby(&self, app_id: u32, lobby_steam_id: SteamID) -> Option<Lobby> {
        let app_lobbies = self.lobbies.get(&app_id)?;
        app_lobbies.remove(&lobby_steam_id).map(|(_, lobby)| lobby)
    }
}
