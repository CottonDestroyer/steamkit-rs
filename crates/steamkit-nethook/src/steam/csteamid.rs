//! Valve's `CSteamID`: a 32 bit account id, a 20 bit account instance, a 4 bit
//! account type and an 8 bit universe, packed into a `u64`.

/// The Steam universes; each universe is a self-contained Steam instance.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum EUniverse {
    /// An invalid universe.
    k_EUniverseInvalid = 0,
    /// The public universe.
    k_EUniversePublic = 1,
    /// The beta universe.
    k_EUniverseBeta = 2,
    /// The internal universe.
    k_EUniverseInternal = 3,
    /// The developer universe.
    k_EUniverseDev = 4,
    /// The release candidate universe.
    k_EUniverseRC = 5,
}

/// The largest valid universe.
pub const K_EUNIVERSE_MAX: u8 = 6;

/// The Steam account types.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum EAccountType {
    /// An invalid account type.
    k_EAccountTypeInvalid = 0,
    /// A single user account.
    k_EAccountTypeIndividual = 1,
    /// A multiseat (e.g. cybercafe) account.
    k_EAccountTypeMultiseat = 2,
    /// A game server account.
    k_EAccountTypeGameServer = 3,
    /// An anonymous game server account.
    k_EAccountTypeAnonGameServer = 4,
    /// A pending account.
    k_EAccountTypePending = 5,
    /// A content server account.
    k_EAccountTypeContentServer = 6,
    /// A clan (group) account.
    k_EAccountTypeClan = 7,
    /// A chat account.
    k_EAccountTypeChat = 8,
    /// A fake Steam id used by superpeers to seed content.
    k_EAccountTypeP2PSuperSeeder = 9,
    /// An anonymous user account.
    k_EAccountTypeAnonUser = 10,
}

/// The largest valid account type.
pub const K_EACCOUNT_TYPE_MAX: u8 = 11;

/// The mask covering the account id field.
pub const K_UN_STEAM_ACCOUNT_ID_MASK: u32 = 0xFFFF_FFFF;

/// The mask covering the account instance field.
pub const K_UN_STEAM_ACCOUNT_INSTANCE_MASK: u32 = 0x000F_FFFF;

/// The mask covering the chat account instance field; the top 8 bits are flags.
pub const K_E_CHAT_ACCOUNT_INSTANCE_MASK: u32 = 0x0000_0FFF;

/// The chat instance flag that marks a clan account.
pub const K_E_CHAT_INSTANCE_FLAG_CLAN: u32 = (K_UN_STEAM_ACCOUNT_INSTANCE_MASK + 1) >> 1;

/// The chat instance flag that marks a lobby.
pub const K_E_CHAT_INSTANCE_FLAG_LOBBY: u32 = (K_UN_STEAM_ACCOUNT_INSTANCE_MASK + 1) >> 2;

/// The chat instance flag that marks a matchmaking lobby.
pub const K_E_CHAT_INSTANCE_FLAG_MMS_LOBBY: u32 = (K_UN_STEAM_ACCOUNT_INSTANCE_MASK + 1) >> 3;

/// A Steam id, packed into 64 bits.
///
/// Stores the raw 64 bit value and derives the fields on demand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[allow(non_snake_case)]
pub struct CSteamID {
    /// All 64 bits of the id.
    pub m_unAll64Bits: u64,
}

impl CSteamID {
    /// Creates an empty (nil) Steam id.
    pub const fn new() -> CSteamID {
        CSteamID { m_unAll64Bits: 0 }
    }

    /// Creates a Steam id from its 64 bit representation.
    pub const fn from_uint64(value: u64) -> CSteamID {
        CSteamID { m_unAll64Bits: value }
    }

    /// Creates a Steam id from its parts.
    pub fn from_parts(
        account_id: u32,
        account_instance: u32,
        universe: EUniverse,
        account_type: EAccountType,
    ) -> CSteamID {
        let mut id = CSteamID::new();
        id.instanced_set(account_id, account_instance, universe, account_type);
        id
    }

    /// Sets the account id, universe and account type, defaulting the instance to 1
    /// (or 0 for clan accounts).
    pub fn set(&mut self, account_id: u32, universe: EUniverse, account_type: EAccountType) {
        self.set_account_id(account_id);
        self.set_universe(universe);
        self.set_account_type(account_type);

        let instance = if account_type == EAccountType::k_EAccountTypeClan {
            0
        } else {
            1
        };
        self.set_account_instance(instance);
    }

    /// Sets the account id, instance, universe and account type.
    pub fn instanced_set(
        &mut self,
        account_id: u32,
        account_instance: u32,
        universe: EUniverse,
        account_type: EAccountType,
    ) {
        self.set_account_id(account_id);
        self.set_universe(universe);
        self.set_account_type(account_type);
        self.set_account_instance(account_instance);
    }

    /// Sets the id from the low 52 bits plus the universe and account type.
    pub fn full_set(&mut self, identifier: u64, universe: EUniverse, account_type: EAccountType) {
        self.set_account_id((identifier & 0xFFFF_FFFF) as u32);
        self.set_account_instance(((identifier >> 32) & 0xF_FFFF) as u32);
        self.set_universe(universe);
        self.set_account_type(account_type);
    }

    /// Overwrites the whole id with `value`.
    pub fn set_from_uint64(&mut self, value: u64) {
        self.m_unAll64Bits = value;
    }

    /// Returns the 64 bit representation of the id.
    pub const fn convert_to_uint64(self) -> u64 {
        self.m_unAll64Bits
    }

    /// Returns the account id.
    pub const fn get_account_id(self) -> u32 {
        (self.m_unAll64Bits & 0xFFFF_FFFF) as u32
    }

    /// Overwrites the account id.
    pub fn set_account_id(&mut self, account_id: u32) {
        self.m_unAll64Bits = (self.m_unAll64Bits & !0xFFFF_FFFF) | account_id as u64;
    }

    /// Returns the account instance.
    pub const fn get_account_instance(self) -> u32 {
        ((self.m_unAll64Bits >> 32) & 0xF_FFFF) as u32
    }

    /// Overwrites the account instance.
    pub fn set_account_instance(&mut self, instance: u32) {
        let mask = !(0xF_FFFFu64 << 32);
        self.m_unAll64Bits = (self.m_unAll64Bits & mask) | ((instance as u64 & 0xF_FFFF) << 32);
    }

    /// Returns the universe.
    pub const fn get_universe(self) -> u8 {
        ((self.m_unAll64Bits >> 56) & 0xFF) as u8
    }

    /// Overwrites the universe.
    pub fn set_universe(&mut self, universe: EUniverse) {
        let mask = !(0xFFu64 << 56);
        self.m_unAll64Bits = (self.m_unAll64Bits & mask) | ((universe as u64 & 0xFF) << 56);
    }

    /// Returns the account type.
    pub const fn get_account_type(self) -> u8 {
        ((self.m_unAll64Bits >> 52) & 0xF) as u8
    }

    /// Overwrites the account type.
    pub fn set_account_type(&mut self, account_type: EAccountType) {
        let mask = !(0xFu64 << 52);
        self.m_unAll64Bits = (self.m_unAll64Bits & mask) | ((account_type as u64 & 0xF) << 52);
    }

    /// Returns `true` when the account type is an individual user account.
    pub const fn is_individual_account(self) -> bool {
        self.get_account_type() == EAccountType::k_EAccountTypeIndividual as u8
    }

    /// Returns `true` when the account type is a game server account.
    pub const fn is_game_server_account(self) -> bool {
        let account_type = self.get_account_type();
        account_type == EAccountType::k_EAccountTypeGameServer as u8
            || account_type == EAccountType::k_EAccountTypeAnonGameServer as u8
    }

    /// Returns `true` when the account type is a content server account.
    pub const fn is_content_server_account(self) -> bool {
        self.get_account_type() == EAccountType::k_EAccountTypeContentServer as u8
    }

    /// Returns `true` when the account type is a clan account.
    pub const fn is_clan_account(self) -> bool {
        self.get_account_type() == EAccountType::k_EAccountTypeClan as u8
    }

    /// Returns `true` when the account type is a chat account.
    pub const fn is_chat_account(self) -> bool {
        self.get_account_type() == EAccountType::k_EAccountTypeChat as u8
    }

    /// Returns `true` when the account type is an anonymous account.
    pub const fn is_anon_account(self) -> bool {
        let account_type = self.get_account_type();
        account_type == EAccountType::k_EAccountTypeAnonUser as u8
            || account_type == EAccountType::k_EAccountTypeAnonGameServer as u8
    }

    /// Returns `true` when the account type is an anonymous user account.
    pub const fn is_anon_user_account(self) -> bool {
        self.get_account_type() == EAccountType::k_EAccountTypeAnonUser as u8
    }

    /// Returns `true` when the id describes a chat lobby.
    pub const fn is_lobby(self) -> bool {
        self.is_chat_account() && (self.get_account_instance() & K_E_CHAT_INSTANCE_FLAG_LOBBY) != 0
    }

    /// Returns `true` when the id describes a blank anonymous account.
    pub const fn is_blank_anon_account(self) -> bool {
        self.get_account_id() == 0 && self.is_anon_account() && self.get_account_instance() == 0
    }

    /// Returns `true` when the id is structurally valid.
    pub const fn is_valid(self) -> bool {
        let account_type = self.get_account_type();
        if account_type <= EAccountType::k_EAccountTypeInvalid as u8
            || account_type >= K_EACCOUNT_TYPE_MAX
        {
            return false;
        }

        let universe = self.get_universe();
        if universe <= EUniverse::k_EUniverseInvalid as u8 || universe >= K_EUNIVERSE_MAX {
            return false;
        }

        if account_type == EAccountType::k_EAccountTypeIndividual as u8
            && (self.get_account_id() == 0 || self.get_account_instance() != 1)
        {
            return false;
        }

        if account_type == EAccountType::k_EAccountTypeClan as u8
            && (self.get_account_id() == 0 || self.get_account_instance() != 0)
        {
            return false;
        }

        true
    }
}

/// The generic invalid Steam id.
pub const K_STEAM_ID_NIL: CSteamID = CSteamID::new();
