//! SteamIDs, plus the `BitVector64` helper that `GlobalID` and `GameID` use too.

use std::fmt;

use crate::base::generated::steam_language::{EAccountType, EUniverse};
use crate::error::SteamError;

/// A 64-bit value with masked field access.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct BitVector64 {
    data: u64,
}

impl BitVector64 {
    pub(crate) fn new(value: u64) -> Self {
        BitVector64 { data: value }
    }

    pub(crate) fn data(&self) -> u64 {
        self.data
    }

    pub(crate) fn set_data(&mut self, value: u64) {
        self.data = value;
    }

    /// `Data >> bitoffset & valuemask`
    pub(crate) fn get(&self, bitoffset: u32, valuemask: u64) -> u64 {
        self.data.wrapping_shr(bitoffset) & valuemask
    }

    /// `Data = (Data & ~(valuemask << bitoffset)) | ((value & valuemask) << bitoffset)`
    pub(crate) fn set(&mut self, bitoffset: u32, valuemask: u64, value: u64) {
        let shifted_mask = valuemask.wrapping_shl(bitoffset);
        self.data = (self.data & !shifted_mask) | (value & valuemask).wrapping_shl(bitoffset);
    }
}

/// Flags a chat [`SteamID`] can have, packed into its instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ChatInstanceFlags {
    /// This flag is set for clan based chat `SteamID`s.
    Clan = (SteamID::ACCOUNT_INSTANCE_MASK + 1) >> 1,
    /// This flag is set for lobby based chat `SteamID`s.
    Lobby = (SteamID::ACCOUNT_INSTANCE_MASK + 1) >> 2,
    /// This flag is set for matchmaking lobby based chat `SteamID`s.
    MMSLobby = (SteamID::ACCOUNT_INSTANCE_MASK + 1) >> 3,
}

impl ChatInstanceFlags {
    /// The numeric value of this flag.
    pub const fn value(self) -> u32 {
        self as u32
    }

    /// Whether all bits of `other` are set.
    pub const fn has_flag(self, other: ChatInstanceFlags) -> bool {
        (self as u32) & (other as u32) == (other as u32)
    }
}

impl std::ops::BitOr for ChatInstanceFlags {
    type Output = u32;

    fn bitor(self, rhs: ChatInstanceFlags) -> u32 {
        (self as u32) | (rhs as u32)
    }
}

/// The character for account types that don't have their own.
const UNKNOWN_ACCOUNT_TYPE_CHAR: char = 'i';

/// A 64-bit id for various objects on the Steam network.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SteamID(u64);

impl SteamID {
    /// The account instance meaning "all instances".
    pub const ALL_INSTANCES: u32 = 0;
    /// The account instance value for a desktop `SteamID`.
    pub const DESKTOP_INSTANCE: u32 = 1;
    /// The account instance value for a console `SteamID`.
    pub const CONSOLE_INSTANCE: u32 = 2;
    /// The account instance for mobile or web based `SteamID`s.
    pub const WEB_INSTANCE: u32 = 4;

    /// Mask of the account id.
    pub const ACCOUNT_ID_MASK: u32 = 0xFFFF_FFFF;
    /// Mask of the account instance (where the chat flags live).
    pub const ACCOUNT_INSTANCE_MASK: u32 = 0x000F_FFFF;

    /// Creates a zero SteamID.
    pub fn new() -> SteamID {
        SteamID(0)
    }

    /// Creates a SteamID from its 64-bit value.
    pub fn from_id(id: u64) -> SteamID {
        SteamID(id)
    }

    /// Creates a SteamID from its parts. The instance is
    /// [`SteamID::DESKTOP_INSTANCE`], except for clan and game server accounts,
    /// which use [`SteamID::ALL_INSTANCES`].
    pub fn from_parts(account_type: EAccountType, universe: EUniverse, account_id: u32) -> SteamID {
        let mut sid = SteamID(0);
        sid.set(account_id, universe, account_type);
        sid
    }

    /// Creates a SteamID from its parts, with an explicit instance.
    pub fn from_parts_instanced(
        account_type: EAccountType,
        universe: EUniverse,
        account_id: u32,
        instance: u32,
    ) -> SteamID {
        let mut sid = SteamID(0);
        sid.instanced_set(account_id, instance, universe, account_type);
        sid
    }

    /// Sets the parts of this SteamID.
    pub fn set(&mut self, account_id: u32, universe: EUniverse, account_type: EAccountType) {
        self.set_account_id(account_id);
        self.set_account_universe(universe);
        self.set_account_type(account_type);

        if account_type == EAccountType::Clan || account_type == EAccountType::GameServer {
            self.set_account_instance(Self::ALL_INSTANCES);
        } else {
            self.set_account_instance(Self::DESKTOP_INSTANCE);
        }
    }

    /// Sets the parts of this SteamID, with an explicit instance.
    pub fn instanced_set(
        &mut self,
        account_id: u32,
        instance: u32,
        universe: EUniverse,
        account_type: EAccountType,
    ) {
        self.set_account_id(account_id);
        self.set_account_universe(universe);
        self.set_account_type(account_type);
        self.set_account_instance(instance);
    }

    /// Sets this SteamID from a Steam2 "STEAM_" string and a universe. Returns
    /// `false` if the string isn't valid.
    pub fn set_from_string(&mut self, steam_id: &str, universe: EUniverse) -> bool {
        let Some((_universe_digit, auth_server, account_id)) = parse_steam2_id(steam_id) else {
            return false;
        };

        self.set_account_universe(universe);
        self.set_account_instance(1);
        self.set_account_type(EAccountType::Individual);
        self.set_account_id(account_id.wrapping_shl(1) | auth_server);

        true
    }

    /// Sets this SteamID from a Steam3 "[X:1:2:3]" string. Returns `false` if
    /// the string isn't valid.
    pub fn set_from_steam3_string(&mut self, steam_id: &str) -> bool {
        let Some(parsed) = parse_steam3_id(steam_id) else {
            return false;
        };

        let mut instance = match parsed.instance {
            Some(instance) => instance,
            None => match parsed.type_char {
                'g' | 'T' | 'c' | 'L' => 0,
                _ => 1,
            },
        };

        match parsed.type_char {
            'c' => {
                instance |= ChatInstanceFlags::Clan.value();
                self.set_account_type(EAccountType::Chat);
            }
            'L' => {
                instance |= ChatInstanceFlags::Lobby.value();
                self.set_account_type(EAccountType::Chat);
            }
            'i' => {
                self.set_account_type(EAccountType::Invalid);
            }
            other => {
                let account_type = account_type_from_char(other);
                self.set_account_type(account_type);
            }
        }

        self.set_account_universe(universe_from_value(parsed.universe));
        self.set_account_instance(instance);
        self.set_account_id(parsed.account_id);

        true
    }

    /// Sets this SteamID from its 64-bit value.
    pub fn set_from_uint64(&mut self, ul_steam_id: u64) {
        self.0 = ul_steam_id;
    }

    /// The 64-bit value.
    pub fn convert_to_u64(&self) -> u64 {
        self.0
    }

    /// A key for grouping accounts that differ only in their instance.
    pub fn get_static_account_key(&self) -> u64 {
        ((self.account_universe_raw() as u64) << 56)
            + ((self.account_type_raw() as u64) << 52)
            + (self.account_id() as u64)
    }

    /// The raw 64-bit value, instance bits included.
    pub fn value(&self) -> u64 {
        self.0
    }

    /// The raw 4-bit account type. Kept raw because the generated enum can't
    /// hold the invalid values 11-15.
    fn account_type_raw(&self) -> u32 {
        ((self.0 >> 52) & 0xF) as u32
    }

    /// The raw 8-bit universe. Kept raw because the generated enum can't hold
    /// the invalid values 5-255.
    fn account_universe_raw(&self) -> u32 {
        ((self.0 >> 56) & 0xFF) as u32
    }

    /// Whether this is a blank anonymous account.
    pub fn is_blank_anon_account(&self) -> bool {
        self.account_id() == 0 && self.is_anon_account() && self.account_instance() == 0
    }

    /// Whether this is a game server account.
    pub fn is_game_server_account(&self) -> bool {
        let account_type = self.account_type_raw();
        account_type == EAccountType::GameServer.value() as u32
            || account_type == EAccountType::AnonGameServer.value() as u32
    }

    /// Whether this is a persistent game server account.
    pub fn is_persistent_game_server_account(&self) -> bool {
        self.account_type_raw() == EAccountType::GameServer.value() as u32
    }

    /// Whether this is an anonymous game server account.
    pub fn is_anon_game_server_account(&self) -> bool {
        self.account_type_raw() == EAccountType::AnonGameServer.value() as u32
    }

    /// Whether this is a content server account.
    pub fn is_content_server_account(&self) -> bool {
        self.account_type_raw() == EAccountType::ContentServer.value() as u32
    }

    /// Whether this is a clan account.
    pub fn is_clan_account(&self) -> bool {
        self.account_type_raw() == EAccountType::Clan.value() as u32
    }

    /// Whether this is a chat account.
    pub fn is_chat_account(&self) -> bool {
        self.account_type_raw() == EAccountType::Chat.value() as u32
    }

    /// Whether this is a lobby.
    pub fn is_lobby(&self) -> bool {
        self.is_chat_account() && (self.account_instance() & ChatInstanceFlags::Lobby.value()) > 0
    }

    /// Whether this is an individual account.
    pub fn is_individual_account(&self) -> bool {
        let account_type = self.account_type_raw();
        account_type == EAccountType::Individual.value() as u32
            || account_type == EAccountType::ConsoleUser.value() as u32
    }

    /// Whether this is an anonymous account.
    pub fn is_anon_account(&self) -> bool {
        let account_type = self.account_type_raw();
        account_type == EAccountType::AnonUser.value() as u32
            || account_type == EAccountType::AnonGameServer.value() as u32
    }

    /// Whether this is an anonymous user account.
    pub fn is_anon_user_account(&self) -> bool {
        self.account_type_raw() == EAccountType::AnonUser.value() as u32
    }

    /// Whether this is a console user account.
    pub fn is_console_user_account(&self) -> bool {
        self.account_type_raw() == EAccountType::ConsoleUser.value() as u32
    }

    /// Whether this is a valid SteamID.
    pub fn is_valid(&self) -> bool {
        let account_type = self.account_type_raw();
        if account_type <= EAccountType::Invalid.value() as u32
            || account_type > EAccountType::AnonUser.value() as u32
        {
            return false;
        }

        let universe = self.account_universe_raw();
        if universe <= EUniverse::Invalid.value() as u32 || universe > EUniverse::Dev.value() as u32 {
            return false;
        }

        if account_type == EAccountType::Individual.value() as u32
            && (self.account_id() == 0 || self.account_instance() > Self::WEB_INSTANCE)
        {
            return false;
        }

        if account_type == EAccountType::Clan.value() as u32
            && (self.account_id() == 0 || self.account_instance() != 0)
        {
            return false;
        }

        if account_type == EAccountType::GameServer.value() as u32 && self.account_id() == 0 {
            return false;
        }

        true
    }

    /// The account id.
    pub fn account_id(&self) -> u32 {
        (self.0 & Self::ACCOUNT_ID_MASK as u64) as u32
    }

    /// Sets the account id.
    pub fn set_account_id(&mut self, value: u32) {
        let mask = Self::ACCOUNT_ID_MASK as u64;
        self.0 = (self.0 & !mask) | (value as u64 & mask);
    }

    /// The account instance.
    pub fn account_instance(&self) -> u32 {
        ((self.0 >> 32) & Self::ACCOUNT_INSTANCE_MASK as u64) as u32
    }

    /// Sets the account instance.
    pub fn set_account_instance(&mut self, value: u32) {
        let mask = (Self::ACCOUNT_INSTANCE_MASK as u64) << 32;
        self.0 = (self.0 & !mask) | ((value as u64) & Self::ACCOUNT_INSTANCE_MASK as u64) << 32;
    }

    /// The account type. Values outside the generated enum (11-15) come back as
    /// [`EAccountType::Invalid`]; [`SteamID::render`] still renders them
    /// correctly.
    pub fn account_type(&self) -> EAccountType {
        account_type_from_value(self.account_type_raw())
    }

    /// Sets the account type.
    pub fn set_account_type(&mut self, value: EAccountType) {
        let raw = (value.value() as u64) & 0xF;
        self.0 = (self.0 & !(0xF_u64 << 52)) | (raw << 52);
    }

    /// The universe. Values outside the generated enum (5-255) come back as
    /// [`EUniverse::Invalid`].
    pub fn account_universe(&self) -> EUniverse {
        universe_from_value(self.account_universe_raw())
    }

    /// Sets the universe.
    pub fn set_account_universe(&mut self, value: EUniverse) {
        let raw = (value.value() as u64) & 0xFF;
        self.0 = (self.0 & !(0xFF_u64 << 56)) | (raw << 56);
    }

    /// Renders this SteamID as Steam3 ("[U:1:123]") or Steam2 ("STEAM_0:1:61").
    pub fn render(&self, steam3: bool) -> String {
        if steam3 {
            self.render_steam3()
        } else {
            self.render_steam2()
        }
    }

    /// Converts this clan id to a chat id, as a raw 64-bit value. Ids that
    /// aren't clan accounts are converted anyway; use
    /// [`SteamID::try_to_chat_id`] for a checked version.
    pub fn to_chat_id(&self) -> u64 {
        let mut chat_id = self.0;
        chat_id = (chat_id & !((Self::ACCOUNT_INSTANCE_MASK as u64) << 32))
            | (ChatInstanceFlags::Clan.value() as u64) << 32;
        chat_id = (chat_id & !(0xF_u64 << 52)) | ((EAccountType::Chat.value() as u64 & 0xF) << 52);
        chat_id
    }

    /// Converts this clan id to a chat id, failing if it isn't a clan account.
    pub fn try_to_chat_id(&self) -> Result<SteamID, SteamError> {
        if !self.is_clan_account() {
            return Err(SteamError::invalid_argument(
                "Only Clan IDs can be converted to Chat IDs.",
            ));
        }

        Ok(SteamID(self.to_chat_id()))
    }

    /// Converts this chat id to a clan id, if it is a clan chat.
    pub fn try_get_clan_id(&self) -> Option<SteamID> {
        if self.is_chat_account() && self.account_instance() == ChatInstanceFlags::Clan.value() {
            let mut group_id = SteamID(self.0);
            group_id.set_account_type(EAccountType::Clan);
            group_id.set_account_instance(0);
            Some(group_id)
        } else {
            None
        }
    }

    /// Renders the Steam2 form.
    fn render_steam2(&self) -> String {
        let account_type_raw = self.account_type_raw();

        if account_type_raw == EAccountType::Invalid.value() as u32
            || account_type_raw == EAccountType::Individual.value() as u32
        {
            let universe_digit = if self.account_universe_raw() <= EUniverse::Public.value() as u32
            {
                "0".to_string()
            } else {
                self.account_universe_raw().to_string()
            };
            format!(
                "STEAM_{}:{}:{}",
                universe_digit,
                self.account_id() & 1,
                self.account_id() >> 1
            )
        } else {
            // Only individual (and invalid) accounts have a Steam2 form; anything
            // else renders as the plain 64-bit number.
            self.0.to_string()
        }
    }

    /// Renders the Steam3 form.
    fn render_steam3(&self) -> String {
        let account_type_raw = self.account_type_raw();
        let mut account_type_char = account_type_char(account_type_raw);

        if account_type_raw == EAccountType::Chat.value() as u32 {
            let instance = self.account_instance();
            if instance & ChatInstanceFlags::Clan.value() != 0 {
                account_type_char = 'c';
            } else if instance & ChatInstanceFlags::Lobby.value() != 0 {
                account_type_char = 'L';
            }
        }

        let render_instance = match account_type_raw {
            raw if raw == EAccountType::AnonGameServer.value() as u32
                || raw == EAccountType::Multiseat.value() as u32 =>
            {
                true
            }
            raw if raw == EAccountType::Individual.value() as u32 => {
                self.account_instance() != Self::DESKTOP_INSTANCE
            }
            _ => false,
        };

        if render_instance {
            format!(
                "[{}:{}:{}:{}]",
                account_type_char,
                self.account_universe_raw(),
                self.account_id(),
                self.account_instance()
            )
        } else {
            format!(
                "[{}:{}:{}]",
                account_type_char,
                self.account_universe_raw(),
                self.account_id()
            )
        }
    }
}

impl fmt::Display for SteamID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(true))
    }
}

impl fmt::Debug for SteamID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<SteamID> for u64 {
    fn from(sid: SteamID) -> u64 {
        sid.0
    }
}

impl From<u64> for SteamID {
    fn from(id: u64) -> SteamID {
        SteamID(id)
    }
}

/// Maps a raw account type to the generated `EAccountType` enum.
fn account_type_from_value(raw: u32) -> EAccountType {
    match raw {
        0 => EAccountType::Invalid,
        1 => EAccountType::Individual,
        2 => EAccountType::Multiseat,
        3 => EAccountType::GameServer,
        4 => EAccountType::AnonGameServer,
        5 => EAccountType::Pending,
        6 => EAccountType::ContentServer,
        7 => EAccountType::Clan,
        8 => EAccountType::Chat,
        9 => EAccountType::ConsoleUser,
        10 => EAccountType::AnonUser,
        _ => EAccountType::Invalid,
    }
}

/// Maps a raw universe to the generated `EUniverse` enum.
fn universe_from_value(raw: u32) -> EUniverse {
    match raw {
        0 => EUniverse::Invalid,
        1 => EUniverse::Public,
        2 => EUniverse::Beta,
        3 => EUniverse::Internal,
        4 => EUniverse::Dev,
        _ => EUniverse::Invalid,
    }
}

/// The account type for a Steam3 type character.
fn account_type_from_char(ch: char) -> EAccountType {
    match ch {
        'A' => EAccountType::AnonGameServer,
        'G' => EAccountType::GameServer,
        'M' => EAccountType::Multiseat,
        'P' => EAccountType::Pending,
        'C' => EAccountType::ContentServer,
        'g' => EAccountType::Clan,
        'T' => EAccountType::Chat,
        'I' => EAccountType::Invalid,
        'U' => EAccountType::Individual,
        'a' => EAccountType::AnonUser,
        _ => EAccountType::Invalid,
    }
}

/// The Steam3 type character of an account type. `ConsoleUser` has none, so it
/// gets the "unknown" character.
fn account_type_char(raw: u32) -> char {
    match raw {
        0 => 'I',
        1 => 'U',
        2 => 'M',
        3 => 'G',
        4 => 'A',
        5 => 'P',
        6 => 'C',
        7 => 'g',
        8 => 'T',
        10 => 'a',
        _ => UNKNOWN_ACCOUNT_TYPE_CHAR,
    }
}

/// The parts of a parsed Steam3 id.
struct Steam3Parts {
    type_char: char,
    universe: u32,
    account_id: u32,
    instance: Option<u32>,
}

/// Parses a Steam2 id like "STEAM_0:1:12345". Matches
/// `STEAM_([0-4]):([0-1]):([0-9]+)`, case-insensitive and unanchored, so the
/// leftmost occurrence in the input is used.
fn parse_steam2_id(text: &str) -> Option<(u32, u32, u32)> {
    let bytes = text.as_bytes();
    let mut search_from = 0usize;

    while let Some(offset) = find_ignore_case(bytes, b"STEAM_", search_from) {
        if let Some(parsed) = parse_steam2_at(bytes, offset + 6) {
            return Some(parsed);
        }
        search_from = offset + 1;
    }

    None
}

fn parse_steam2_at(bytes: &[u8], mut index: usize) -> Option<(u32, u32, u32)> {
    if index >= bytes.len() || !bytes[index].is_ascii_digit() || bytes[index] > b'4' {
        return None;
    }
    let universe = (bytes[index] - b'0') as u32;
    index += 1;

    if index >= bytes.len() || bytes[index] != b':' {
        return None;
    }
    index += 1;

    if index >= bytes.len() || !bytes[index].is_ascii_digit() || bytes[index] > b'1' {
        return None;
    }
    let auth_server = (bytes[index] - b'0') as u32;
    index += 1;

    if index >= bytes.len() || bytes[index] != b':' {
        return None;
    }
    index += 1;

    let digits_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == digits_start {
        return None;
    }

    let account_id = parse_u32(&bytes[digits_start..index])?;

    Some((universe, auth_server, account_id))
}

/// Parses a Steam3 id like "[U:1:1234]". Matches
/// `\[([AGMPCgcLTIUai]):([0-4]):([0-9]+)(:([0-9]+))?\]`, or failing that the
/// older form with the instance in parentheses,
/// `\[([AGMPCgcLTIUai]):([0-4]):([0-9]+)(\(([0-9]+)\))?\]`.
fn parse_steam3_id(text: &str) -> Option<Steam3Parts> {
    parse_steam3_with(text, false).or_else(|| parse_steam3_with(text, true))
}

fn parse_steam3_with(text: &str, fallback: bool) -> Option<Steam3Parts> {
    let bytes = text.as_bytes();
    let mut search_from = 0usize;

    while let Some(offset) = bytes[search_from..]
        .iter()
        .position(|&b| b == b'[')
        .map(|pos| pos + search_from)
    {
        if let Some(parsed) = parse_steam3_at(bytes, offset, fallback) {
            return Some(parsed);
        }
        search_from = offset + 1;
    }

    None
}

fn parse_steam3_at(bytes: &[u8], mut index: usize, fallback: bool) -> Option<Steam3Parts> {
    if index + 1 >= bytes.len() {
        return None;
    }

    let type_char = bytes[index + 1] as char;
    if !matches!(
        type_char,
        'A' | 'G' | 'M' | 'P' | 'C' | 'g' | 'c' | 'L' | 'T' | 'I' | 'U' | 'a' | 'i'
    ) {
        return None;
    }
    index += 2;

    if index >= bytes.len() || bytes[index] != b':' {
        return None;
    }
    index += 1;

    if index >= bytes.len() || !bytes[index].is_ascii_digit() || bytes[index] > b'4' {
        return None;
    }
    let universe = (bytes[index] - b'0') as u32;
    index += 1;

    if index >= bytes.len() || bytes[index] != b':' {
        return None;
    }
    index += 1;

    let digits_start = index;
    let mut digits_end = index;
    while digits_end < bytes.len() && bytes[digits_end].is_ascii_digit() {
        digits_end += 1;
    }
    if digits_end == digits_start {
        return None;
    }

    // Like a greedy regex that backtracks when the optional instance doesn't
    // match: try shorter and shorter account ids.
    let mut account_end = digits_end;
    while account_end > digits_start {
        let Some(account_id) = parse_u32(&bytes[digits_start..account_end]) else {
            return None;
        };
        match parse_steam3_tail(bytes, account_end, fallback) {
            Some(instance) => {
                return Some(Steam3Parts {
                    type_char,
                    universe,
                    account_id,
                    instance,
                })
            }
            None => account_end -= 1,
        }
    }

    None
}

/// Parses the optional instance part plus the closing bracket.
fn parse_steam3_tail(bytes: &[u8], index: usize, fallback: bool) -> Option<Option<u32>> {
    let separator = if fallback { b'(' } else { b':' };

    if index < bytes.len() && bytes[index] == separator {
        let mut cursor = index + 1;
        let digits_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor > digits_start {
            if let Some(instance) = parse_u32(&bytes[digits_start..cursor]) {
                if fallback {
                    // The fallback form is `(\d+)]`, so the parenthesis and
                    // the bracket are two separate characters.
                    if cursor < bytes.len() && bytes[cursor] == b')' {
                        cursor += 1;
                        if cursor < bytes.len() && bytes[cursor] == b']' {
                            return Some(Some(instance));
                        }
                    }
                } else if cursor < bytes.len() && bytes[cursor] == b']' {
                    // The regular form is `:\d+]`, so the bracket ends the
                    // match.
                    return Some(Some(instance));
                }
            }
        }
    }

    if index < bytes.len() && bytes[index] == b']' {
        return Some(None);
    }

    None
}

/// Parses ASCII digits into a `u32`, failing on overflow.
fn parse_u32(digits: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(digits).ok()?;
    let value: u64 = text.parse().ok()?;
    if value > u32::MAX as u64 {
        return None;
    }
    Some(value as u32)
}

/// Finds the leftmost case insensitive occurrence of `needle`.
fn find_ignore_case(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    let last = haystack.len() - needle.len();
    let mut index = from;
    while index <= last {
        if haystack[index..index + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Some(index);
        }
        index += 1;
    }

    None
}
