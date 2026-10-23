//! `SteamConfiguration` tests: [`steam_configuration_default`] covers the
//! defaults and [`steam_configuration_configured_object`] a fully configured
//! object.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::{Duration, SystemTime};

use steamkit::base::generated::steam_language::{EClientPersonaStateFlag, EUniverse};
use steamkit::error::SteamError;
use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::steam::discovery::i_server_list_provider::IServerListProvider;
use steamkit::steam::discovery::server_record::ServerRecord;
use steamkit::steam::steam_client::configuration::i_machine_info_provider::IMachineInfoProvider;
use steamkit::steam::steam_client::configuration::i_steam_configuration_builder::ISteamConfigurationBuilder;
use steamkit::steam::steam_client::configuration::steam_configuration::{
    default_http_client_factory, default_web_api_base_address, HttpClientPurpose, SteamConfiguration,
};
use steamkit::steam::steam_client::configuration::steam_configuration_state::PersonaStateFlags;
use steamkit::util::hardware_utils::DefaultMachineInfoProvider;

/// Runs a future to completion on the current thread.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime")
        .block_on(future)
}

/// Sends one request through `client` to a local server and returns the user-agent
/// the server saw on the wire.
async fn user_agent_of(client: &reqwest::Client) -> Option<String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local listener");
    let address = listener.local_addr().expect("the local address");

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("a connection");
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await;
        String::from_utf8_lossy(&request).into_owned()
    });

    let _ = client.get(format!("http://{address}/")).send().await;
    let request = server.await.expect("the server task");
    request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("user-agent") {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

/// The defaults.
mod steam_configuration_default {
    use super::*;

    fn configuration() -> SteamConfiguration {
        SteamConfiguration::create_default()
    }

    #[test]
    fn cell_id_is_zero() {
        assert_eq!(configuration().cell_id(), 0u32);
    }

    #[test]
    fn connection_timeout_is_five_seconds() {
        assert_eq!(configuration().connection_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn default_persona_state_flags() {
        // The full flag mask is compared.
        let expected = PersonaStateFlags::new(
            EClientPersonaStateFlag::PlayerName.value()
                | EClientPersonaStateFlag::Presence.value()
                | EClientPersonaStateFlag::SourceID.value()
                | EClientPersonaStateFlag::GameExtraInfo.value()
                | EClientPersonaStateFlag::LastSeen.value(),
        );
        assert_eq!(configuration().default_persona_state_flags_mask(), expected);
    }

    #[test]
    fn default_http_client_factory() {
        let configuration = configuration();
        // The factory returns a concrete reqwest::Client.
        let client = (configuration.http_client_factory())(HttpClientPurpose::WebAPI);

        // The user agent is "SteamKit/<version>". The version is steamkit's
        // CARGO_PKG_VERSION, which this crate can't read, so only the prefix is
        // checked.
        //
        // reqwest keeps the default headers private, so the agent is observed on
        // the wire: a local server receives one request and reports it.
        let agent = block_on(user_agent_of(&client));        assert!(
            matches!(agent.as_deref(), Some(agent) if agent.starts_with("SteamKit/")),
            "the default client must carry a SteamKit/<version> user agent, got {agent:?}"
        );
    }

    #[test]
    fn default_machine_info_provider() {
        let configuration = configuration();
        let provider = configuration.machine_info_provider();
        // The provider can't be downcast (no as_any), so compare it with the
        // shared fallback instance instead; on Windows the default is the
        // platform provider, which is a different allocation.
        let fallback = DefaultMachineInfoProvider::instance();
        assert!(
            !Arc::ptr_eq(provider, &fallback),
            "the default machine info provider must not be the fallback DefaultMachineInfoProvider"
        );
    }

    #[tokio::test]
    async fn server_list_provider_is_nothing_fancy() {
        // The provider can't be downcast, so check that it behaves like a
        // MemoryServerListProvider: no servers, and a refresh time of the Unix
        // epoch.
        let configuration = configuration();
        let provider = configuration.server_list_provider();

        let servers = provider.fetch_server_list().await;
        assert!(
            matches!(servers, Ok(ref list) if list.is_empty()),
            "a fresh MemoryServerListProvider holds no servers"
        );

        let refresh = provider.last_server_list_refresh().await;
        assert!(
            matches!(refresh, Ok(time) if time == SystemTime::UNIX_EPOCH),
            "a fresh MemoryServerListProvider reports the Unix epoch"
        );
    }

    #[test]
    fn server_list_is_not_null() {
        let configuration = configuration();
        // server_list() returns a shared Arc that's always present.
        assert!(Arc::strong_count(configuration.server_list()) >= 1);
    }

    #[test]
    fn default_protocols() {
        assert_eq!(
            configuration().protocol_types(),
            ProtocolTypes::TCP | ProtocolTypes::WEB_SOCKET
        );
    }

    #[test]
    fn public_universe() {
        assert_eq!(configuration().universe(), EUniverse::Public);
    }

    #[test]
    fn web_api_address() {
        assert_eq!(
            configuration().web_api_base_address().as_str(),
            "https://api.steampowered.com/"
        );
    }

    #[test]
    fn no_web_api_key() {
        // A missing key is an empty string.
        assert!(configuration().web_api_key().is_empty());
    }
}


/// A fully configured object.
mod steam_configuration_configured_object {
    use super::*;

    /// A custom provider. It's never called; it only has to differ from the
    /// default.
    struct CustomMachineInfoProvider;

    impl IMachineInfoProvider for CustomMachineInfoProvider {
        fn machine_guid(&self) -> Option<Vec<u8>> {
            None
        }

        fn mac_address(&self) -> Option<Vec<u8>> {
            None
        }

        fn disk_id(&self) -> Option<Vec<u8>> {
            None
        }
    }

    /// A custom server list provider, same idea as `CustomMachineInfoProvider`.
    struct CustomServerListProvider;

    #[async_trait::async_trait]
    impl IServerListProvider for CustomServerListProvider {
        async fn last_server_list_refresh(&self) -> Result<SystemTime, SteamError> {
            Ok(SystemTime::UNIX_EPOCH)
        }

        async fn fetch_server_list(&self) -> Result<Vec<ServerRecord>, SteamError> {
            Ok(Vec::new())
        }

        async fn update_server_list(&self, _endpoints: Vec<ServerRecord>) -> Result<(), SteamError> {
            Ok(())
        }
    }

    fn configuration() -> SteamConfiguration {
        let machine: Arc<dyn IMachineInfoProvider> = Arc::new(CustomMachineInfoProvider);
        let server: Arc<dyn IServerListProvider> = Arc::new(CustomServerListProvider);

        // Derive the custom base address from the library's default URL.
        let mut web_api_base_address = default_web_api_base_address();
        web_api_base_address
            .set_scheme("http")
            .expect("http is a valid scheme");
        web_api_base_address
            .set_host(Some("foo.bar.com"))
            .expect("foo.bar.com is a valid host");
        web_api_base_address.set_path("/api/");

        SteamConfiguration::create(|b| {
            b.with_directory_fetch(false)
                .with_cell_id(123)
                .with_connection_timeout(Duration::from_secs(60))
                .with_default_persona_state_flags(PersonaStateFlags::from(
                    EClientPersonaStateFlag::SourceID,
                ))
                .with_http_client_factory(Arc::new(|_purpose: HttpClientPurpose| {
                    default_http_client_factory()(HttpClientPurpose::WebAPI)
                }))
                .with_machine_info_provider(machine.clone())
                .with_protocol_types(ProtocolTypes::WEB_SOCKET | ProtocolTypes::UDP)
                .with_server_list_provider(server.clone())
                .with_universe(EUniverse::Internal)
                .with_web_api_base_address(web_api_base_address)
                .with_web_api_key("T0PS3kR1t".to_string());
        })
    }

    #[test]
    fn directory_fetch_is_configured() {
        assert!(!configuration().allow_directory_fetch());
    }

    #[test]
    fn cell_id_is_configured() {
        assert_eq!(configuration().cell_id(), 123u32);
    }

    #[test]
    fn connection_timeout_is_configured() {
        assert_eq!(configuration().connection_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn http_client_factory_is_configured() {
        // The custom factory must replace the default one and be called for
        // WebAPI.
        let invoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let invoked_in_factory = invoked.clone();
        let configuration = SteamConfiguration::create(|b| {
            b.with_http_client_factory(Arc::new(move |purpose: HttpClientPurpose| {
                assert_eq!(purpose, HttpClientPurpose::WebAPI);
                invoked_in_factory.store(true, std::sync::atomic::Ordering::SeqCst);
                default_http_client_factory()(purpose)
            }));
        });

        let _client = (configuration.http_client_factory())(HttpClientPurpose::WebAPI);
        assert!(
            invoked.load(std::sync::atomic::Ordering::SeqCst),
            "the custom http client factory must be installed and invoked"
        );
    }

    #[test]
    fn machine_info_provider_is_configured() {
        let machine: Arc<dyn IMachineInfoProvider> = Arc::new(CustomMachineInfoProvider);
        let machine_clone = machine.clone();
        let configuration = SteamConfiguration::create(|b| {
            b.with_machine_info_provider(machine.clone());
        });

        assert!(Arc::ptr_eq(
            configuration.machine_info_provider(),
            configuration.machine_info_provider()
        ));

        assert!(Arc::ptr_eq(configuration.machine_info_provider(), &machine_clone));
    }

    #[test]
    fn persona_state_flags_is_configured() {
        assert_eq!(
            configuration().default_persona_state_flags(),
            EClientPersonaStateFlag::SourceID
        );
    }

    #[test]
    fn server_list_provider_is_configured() {
        let server: Arc<dyn IServerListProvider> = Arc::new(CustomServerListProvider);
        let server_clone = server.clone();
        let configuration = SteamConfiguration::create(|b| {
            b.with_server_list_provider(server.clone());
        });

        assert!(Arc::ptr_eq(configuration.server_list_provider(), &server_clone));
    }

    #[test]
    fn server_list_is_not_null() {
        let configuration = configuration();
        assert!(Arc::strong_count(configuration.server_list()) >= 1);
    }

    #[test]
    fn protocols_are_configured() {
        assert_eq!(
            configuration().protocol_types(),
            ProtocolTypes::WEB_SOCKET | ProtocolTypes::UDP
        );
    }

    #[test]
    fn universe_is_configured() {
        assert_eq!(configuration().universe(), EUniverse::Internal);
    }

    #[test]
    fn web_api_address() {
        assert_eq!(
            configuration().web_api_base_address().as_str(),
            "http://foo.bar.com/api/"
        );
    }

    #[test]
    fn no_web_api_key() {
        assert_eq!(configuration().web_api_key(), "T0PS3kR1t");
    }
}
