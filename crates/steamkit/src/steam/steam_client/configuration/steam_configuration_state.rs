//! The raw values of a configuration, filled in by [`SteamConfigurationBuilder`].

use std::fmt;
use std::ops::BitOr;
use std::sync::Arc;
use std::time::Duration;

use crate::base::generated::steam_language::{EClientPersonaStateFlag, EUniverse};
use crate::networking::steam3::protocol_type::ProtocolTypes;
use crate::steam::discovery::i_server_list_provider::IServerListProvider;

use super::i_machine_info_provider::IMachineInfoProvider;
use super::steam_configuration::HttpClientFactory;

/// A bit mask of [`EClientPersonaStateFlag`]s (the generated enum can only
/// hold one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PersonaStateFlags(i32);

impl PersonaStateFlags {
    /// The flags requested by default: player name, presence, source id, game
    /// extra info and last seen.
    pub const DEFAULT: PersonaStateFlags = PersonaStateFlags(
        EClientPersonaStateFlag::PlayerName.value()
            | EClientPersonaStateFlag::Presence.value()
            | EClientPersonaStateFlag::SourceID.value()
            | EClientPersonaStateFlag::GameExtraInfo.value()
            | EClientPersonaStateFlag::LastSeen.value(),
    );

    /// Creates a mask from raw bits.
    pub const fn new(bits: i32) -> Self {
        Self(bits)
    }

    /// The raw bits.
    pub const fn bits(self) -> i32 {
        self.0
    }

    /// Whether the flag is set.
    pub const fn contains(self, flag: EClientPersonaStateFlag) -> bool {
        self.0 & flag.value() != 0
    }

    /// Sets a flag.
    pub fn insert(&mut self, flag: EClientPersonaStateFlag) {
        self.0 |= flag.value();
    }

    /// The first flag that's set, for APIs that take a single flag.
    pub fn representative(&self) -> Option<EClientPersonaStateFlag> {
        EClientPersonaStateFlag::ALL
            .iter()
            .copied()
            .find(|flag| self.contains(*flag))
    }

    /// The union of two masks.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOr for PersonaStateFlags {
    type Output = PersonaStateFlags;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl From<EClientPersonaStateFlag> for PersonaStateFlags {
    fn from(flag: EClientPersonaStateFlag) -> Self {
        Self(flag.value())
    }
}

/// The raw values of a [`SteamConfiguration`](super::SteamConfiguration).
#[derive(Clone)]
pub struct SteamConfigurationState {
    /// Whether to fetch the server list from the Steam Directory.
    pub allow_directory_fetch: bool,
    /// The Steam cell to prefer.
    pub cell_id: u32,
    /// The timeout for connecting to Steam servers.
    pub connection_timeout: Duration,
    /// The default persona state flags requested for a new friend.
    pub default_persona_state_flags: PersonaStateFlags,
    /// The factory that creates HTTP clients.
    pub http_client_factory: HttpClientFactory,
    /// The machine info provider used when logging on.
    pub machine_info_provider: Arc<dyn IMachineInfoProvider>,
    /// The transports to use when connecting.
    pub protocol_types: ProtocolTypes,
    /// The server list provider to use.
    pub server_list_provider: Arc<dyn IServerListProvider>,
    /// The universe to connect to.
    pub universe: EUniverse,
    /// The base address of the Steam Web API.
    pub web_api_base_address: reqwest::Url,
    /// The Web API key attached to requests.
    pub web_api_key: String,
}

impl fmt::Debug for SteamConfigurationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamConfigurationState")
            .field("allow_directory_fetch", &self.allow_directory_fetch)
            .field("cell_id", &self.cell_id)
            .field("connection_timeout", &self.connection_timeout)
            .field("default_persona_state_flags", &self.default_persona_state_flags)
            .field("machine_info_provider", &"Arc<dyn IMachineInfoProvider>")
            .field("protocol_types", &self.protocol_types)
            .field("server_list_provider", &"Arc<dyn IServerListProvider>")
            .field("universe", &self.universe)
            .field("web_api_base_address", &self.web_api_base_address)
            .field("web_api_key", &self.web_api_key)
            .finish()
    }
}

impl Default for SteamConfigurationState {
    fn default() -> Self {
        Self {
            allow_directory_fetch: true,
            cell_id: 0,
            connection_timeout: Duration::from_secs(5),
            default_persona_state_flags: PersonaStateFlags::DEFAULT,
            http_client_factory: super::steam_configuration::default_http_client_factory(),
            machine_info_provider: crate::util::hardware_utils::MachineInfoProvider::get_default_provider(),
            protocol_types: ProtocolTypes::TCP | ProtocolTypes::WEB_SOCKET,
            server_list_provider: Arc::new(crate::steam::discovery::memory_server_list_provider::MemoryServerListProvider::new()),
            universe: EUniverse::Public,
            web_api_base_address: super::steam_configuration::default_web_api_base_address(),
            web_api_key: String::new(),
        }
    }
}
