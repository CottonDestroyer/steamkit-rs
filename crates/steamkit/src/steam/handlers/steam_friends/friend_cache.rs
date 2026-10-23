//! What `SteamFriends` knows about the local user, other users and clans.

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::base::generated::steam_language::{
    EClanRelationship, EFriendRelationship, EPersonaState,
};
use crate::types::steam_id::SteamID;

/// Cached information about a user account.
#[derive(Clone, Debug)]
pub(crate) struct User {
    /// The SteamID of the account.
    pub(crate) steam_id: SteamID,

    /// The persona name of the account, if it is known.
    pub(crate) name: Option<String>,

    /// The SHA-1 avatar hash of the account, if it is known.
    pub(crate) avatar_hash: Option<Vec<u8>>,

    /// The relationship of the account to the local user.
    pub(crate) relationship: EFriendRelationship,

    /// The persona state of the account.
    pub(crate) persona_state: EPersonaState,

    /// The raw persona state flags of the account.
    pub(crate) persona_state_flags: u32,

    /// The app ID of the game the account is playing.
    pub(crate) game_app_id: u32,

    /// The game ID of the game the account is playing.
    pub(crate) game_id: u64,

    /// The name of the game the account is playing.
    pub(crate) game_name: Option<String>,
}

impl User {
    /// Creates an empty cache entry for the given SteamID.
    pub(crate) fn new(steam_id: SteamID) -> Self {
        Self {
            steam_id,
            name: None,
            avatar_hash: None,
            relationship: EFriendRelationship::None,
            persona_state: EPersonaState::Offline,
            persona_state_flags: 0,
            game_app_id: 0,
            game_id: 0,
            game_name: None,
        }
    }
}

/// Cached information about a clan account.
#[derive(Clone, Debug)]
pub(crate) struct Clan {
    /// The SteamID of the clan.
    pub(crate) steam_id: SteamID,

    /// The name of the clan, if it is known.
    pub(crate) name: Option<String>,

    /// The SHA-1 avatar hash of the clan, if it is known.
    pub(crate) avatar_hash: Option<Vec<u8>>,

    /// The relationship of the clan to the local user.
    pub(crate) relationship: EClanRelationship,
}

impl Clan {
    /// Creates an empty cache entry for the given SteamID.
    pub(crate) fn new(steam_id: SteamID) -> Self {
        Self {
            steam_id,
            name: None,
            avatar_hash: None,
            relationship: EClanRelationship::None,
        }
    }
}

/// The accounts this client knows about.
pub(crate) struct AccountCache {
    local_user: RwLock<User>,
    users: DashMap<SteamID, User>,
    clans: DashMap<SteamID, Clan>,
}

impl AccountCache {
    /// Creates an empty cache.
    pub(crate) fn new() -> Self {
        Self {
            local_user: RwLock::new(User::new(SteamID::new())),
            users: DashMap::new(),
            clans: DashMap::new(),
        }
    }

    /// Whether the given SteamID is the local user.
    pub(crate) fn is_local_user(&self, steam_id: SteamID) -> bool {
        self.local_user.read().steam_id == steam_id
    }

    /// A snapshot of the cached local user.
    pub(crate) fn local_user(&self) -> User {
        self.local_user.read().clone()
    }

    /// Sets the persona name of the local user.
    pub(crate) fn set_local_user_name(&self, name: String) {
        self.local_user.write().name = Some(name);
    }

    /// Sets the SteamID of the local user.
    pub(crate) fn set_local_user_steam_id(&self, steam_id: SteamID) {
        self.local_user.write().steam_id = steam_id;
    }

    /// The persona state of the local user.
    pub(crate) fn local_user_persona_state(&self) -> EPersonaState {
        self.local_user.read().persona_state
    }

    /// Sets the persona state of the local user.
    pub(crate) fn set_local_user_persona_state(&self, state: EPersonaState) {
        self.local_user.write().persona_state = state;
    }

    /// A copy of the cached user; creates the entry if it's unknown.
    pub(crate) fn user(&self, steam_id: SteamID) -> User {
        if self.is_local_user(steam_id) {
            return self.local_user();
        }

        self.users
            .entry(steam_id)
            .or_insert_with(|| User::new(steam_id))
            .clone()
    }

    /// Modifies the cached user; creates the entry if it's unknown.
    pub(crate) fn with_user<R>(&self, steam_id: SteamID, f: impl FnOnce(&mut User) -> R) -> R {
        if self.is_local_user(steam_id) {
            let mut local_user = self.local_user.write();
            return f(&mut local_user);
        }

        let mut entry = self
            .users
            .entry(steam_id)
            .or_insert_with(|| User::new(steam_id));
        f(entry.value_mut())
    }

    /// A copy of the cached clan; creates the entry if it's unknown.
    pub(crate) fn clan(&self, steam_id: SteamID) -> Clan {
        self.clans
            .entry(steam_id)
            .or_insert_with(|| Clan::new(steam_id))
            .clone()
    }

    /// Modifies the cached clan; creates the entry if it's unknown.
    pub(crate) fn with_clan<R>(&self, steam_id: SteamID, f: impl FnOnce(&mut Clan) -> R) -> R {
        let mut entry = self
            .clans
            .entry(steam_id)
            .or_insert_with(|| Clan::new(steam_id));
        f(entry.value_mut())
    }
}
