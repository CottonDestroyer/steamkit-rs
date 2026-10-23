//! `FileStorageServerListProvider` tests.
//!
//! The list is written to a uniquely named file in the temp directory, which
//! is removed afterwards.

use std::path::PathBuf;

use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::steam::discovery::file_storage_server_list_provider::FileStorageServerListProvider;
use steamkit::steam::discovery::i_server_list_provider::IServerListProvider;
use steamkit::steam::discovery::server_record::ServerRecord;

/// A unique file name for the persisted server list.
fn temporary_filename() -> PathBuf {
    let unique = rand::random::<u64>();
    std::env::temp_dir().join(format!("steamkit-rs-servertest-{unique:016x}.bin"))
}

#[tokio::test]
async fn reads_updated_server_list() {
    let filename = temporary_filename();
    let provider = FileStorageServerListProvider::new(&filename);

    let fetched = provider.fetch_server_list().await;
    assert!(
        fetched.is_ok(),
        "the initial fetch should succeed: {fetched:?}"
    );

    let servers = vec![
        ServerRecord::create_socket_server(
            "0.0.0.0:1234"
                .parse()
                .expect("0.0.0.0:1234 is a valid socket address"),
        ),
        ServerRecord::create_socket_server(
            "127.0.0.1:4321"
                .parse()
                .expect("127.0.0.1:4321 is a valid socket address"),
        ),
    ];

    let updated = provider.update_server_list(servers).await;
    assert!(
        updated.is_ok(),
        "UpdateServerListAsync should succeed: {updated:?}"
    );

    let servers = provider.fetch_server_list().await;
    let servers = match servers {
        Ok(servers) => servers,
        Err(error) => panic!("FetchServerListAsync should succeed: {error:?}"),
    };

    assert_eq!(2, servers.len());

    assert_eq!("0.0.0.0", servers[0].get_host());
    assert_eq!(1234, servers[0].get_port());
    assert_eq!(
        ProtocolTypes::TCP | ProtocolTypes::UDP,
        servers[0].protocol_types()
    );

    let cleared = provider.update_server_list(Vec::new()).await;
    assert!(
        cleared.is_ok(),
        "clearing the list should succeed: {cleared:?}"
    );

    let _ = std::fs::remove_file(&filename);
}
