//! Game ids: the 64-bit id of an app, mod, shortcut or P2P file.

use std::fmt;

use crate::types::steam_id::BitVector64;

/// The kind of game a [`GameID`] refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GameType {
    /// A Steam application.
    #[default]
    App = 0,
    /// A game modification.
    GameMod = 1,
    /// A shortcut to a program.
    Shortcut = 2,
    /// A peer-to-peer file.
    P2P = 3,
}

impl GameType {
    /// The numeric value of this game type.
    pub const fn value(self) -> u32 {
        self as u32
    }

    /// Maps a raw value to a game type. Unknown values become [`GameType::App`].
    pub const fn from_raw(raw: u32) -> GameType {
        match raw {
            1 => GameType::GameMod,
            2 => GameType::Shortcut,
            3 => GameType::P2P,
            _ => GameType::App,
        }
    }
}

/// A 64-bit id of an app, mod, shortcut or P2P file on the Steam network.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GameID {
    gameid: BitVector64,
}

impl GameID {
    /// Creates a zero game id.
    pub fn new() -> GameID {
        GameID::from_value(0)
    }

    /// Creates a game id from its 64-bit value.
    pub fn from_value(id: u64) -> GameID {
        GameID {
            gameid: BitVector64::new(id),
        }
    }

    /// Creates a game id for an app.
    pub fn from_app_id(n_app_id: i32) -> GameID {
        GameID::from_value(n_app_id as u64)
    }

    /// Creates a game id for a mod.
    pub fn from_mod(n_app_id: u32, mod_path: &str) -> GameID {
        let mut game_id = GameID::from_value(0);
        game_id.set_app_id(n_app_id);
        game_id.set_app_type(GameType::GameMod);
        game_id.set_mod_id(crc32_of(mod_path.as_bytes()));
        game_id
    }

    /// Creates a game id for an application shortcut.
    pub fn from_shortcut(exe_path: &str, app_name: &str) -> GameID {
        let mut combined = String::with_capacity(exe_path.len() + app_name.len());
        combined.push_str(exe_path);
        combined.push_str(app_name);

        let mut game_id = GameID::from_value(0);
        game_id.set_app_id(0);
        game_id.set_app_type(GameType::Shortcut);
        game_id.set_mod_id(crc32_of(combined.as_bytes()));
        game_id
    }

    /// Sets the whole id from its 64-bit value.
    pub fn set(&mut self, game_id: u64) {
        self.gameid.set_data(game_id);
    }

    /// The 64-bit value.
    pub fn to_u64(&self) -> u64 {
        self.gameid.data()
    }

    /// The app id.
    pub fn app_id(&self) -> u32 {
        self.gameid.get(0, 0xFF_FFFF) as u32
    }

    /// Sets the app id.
    pub fn set_app_id(&mut self, value: u32) {
        self.gameid.set(0, 0xFF_FFFF, value as u64);
    }

    /// The type of the game.
    pub fn app_type(&self) -> GameType {
        GameType::from_raw(self.gameid.get(24, 0xFF) as u32)
    }

    /// Sets the type of the game.
    pub fn set_app_type(&mut self, value: GameType) {
        self.gameid.set(24, 0xFF, value.value() as u64);
    }

    /// The mod id.
    pub fn mod_id(&self) -> u32 {
        self.gameid.get(32, 0xFFFF_FFFF) as u32
    }

    /// Sets the mod id. Also sets bit 63.
    pub fn set_mod_id(&mut self, value: u32) {
        self.gameid.set(32, 0xFFFF_FFFF, value as u64);
        self.gameid.set(63, 0xFF, 1);
    }

    /// Whether this is a mod.
    pub fn is_mod(&self) -> bool {
        self.app_type() == GameType::GameMod
    }

    /// Whether this is a shortcut.
    pub fn is_shortcut(&self) -> bool {
        self.app_type() == GameType::Shortcut
    }

    /// Whether this is a P2P file.
    pub fn is_p2p_file(&self) -> bool {
        self.app_type() == GameType::P2P
    }

    /// Whether this is a Steam app.
    pub fn is_steam_app(&self) -> bool {
        self.app_type() == GameType::App
    }
}

impl fmt::Display for GameID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_u64())
    }
}

impl fmt::Debug for GameID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<GameID> for u64 {
    fn from(gid: GameID) -> u64 {
        gid.to_u64()
    }
}

impl From<u64> for GameID {
    fn from(id: u64) -> GameID {
        GameID::from_value(id)
    }
}

impl From<GameID> for String {
    fn from(gid: GameID) -> String {
        gid.to_u64().to_string()
    }
}

/// CRC-32 (IEEE 802.3).
fn crc32_of(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}
