//! Shows how persisting the server list and the cell id improves the chance
//! of a successful connection.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::discovery::file_storage_server_list_provider::FileStorageServerListProvider;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::configuration::i_steam_configuration_builder::ISteamConfigurationBuilder;
use steamkit::steam::steam_client::configuration::steam_configuration::SteamConfiguration;
use steamkit::steam::steam_client::steam_client::SteamClient;

/// The name of the file the cell id is persisted in.
const CELL_ID_FILE: &str = "cellid.txt";

/// The name of the file the server list is persisted in.
const SERVER_LIST_FILE: &str = "servers_list.bin";

/// Logs on to Steam and waits until the user presses Ctrl+C.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample7: No username and password specified!");
        return Ok(());
    };

    let mut cell_id: u32 = 0;

    // if we've previously connected and saved our cellid, load it.
    if let Ok(contents) = std::fs::read_to_string(CELL_ID_FILE) {
        match contents.trim().parse::<u32>() {
            Ok(parsed) => {
                println!("Using persisted cell ID {parsed}");
                cell_id = parsed;
            }
            Err(_) => {
                println!("Error parsing cellid from cellid.txt. Continuing with cellid 0.");
                cell_id = 0;
            }
        }
    }

    let configuration = SteamConfiguration::create(|builder| {
        builder.with_cell_id(cell_id);
        builder.with_server_list_provider(Arc::new(FileStorageServerListProvider::new(
            SERVER_LIST_FILE,
        )));
    });

    // create our steamclient instance
    let steam_client = SteamClient::with_config(configuration);
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    let is_running = Arc::new(AtomicBool::new(true));

    // register a few callbacks we're interested in
    {
        let steam_user = steam_user.clone();
        let user = user.clone();
        let pass = pass.clone();

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            println!("Connected to Steam! Logging in '{user}'...");

            let details = LogOnDetails {
                username: Some(user.clone()),
                password: Some(pass.clone()),
                ..Default::default()
            };

            let _ = steam_user.log_on(&details);
        }));
    }

    {
        let is_running = is_running.clone();

        steam_client.subscribe::<DisconnectedCallback>(Box::new(move |_callback| {
            println!("Disconnected from Steam");

            is_running.store(false, Ordering::SeqCst);
        }));
    }

    {
        let is_running = is_running.clone();

        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
            if callback.result() != EResult::OK {
                if callback.result() == EResult::AccountLogonDenied {
                    // if we receive AccountLogonDenied or one of its flavors
                    // (AccountLogonDeniedNoMailSent, etc) then the account we're
                    // logging into is SteamGuard protected. See sample 5 for how
                    // SteamGuard can be handled
                    println!("Unable to logon to Steam: This account is SteamGuard protected.");

                    is_running.store(false, Ordering::SeqCst);
                    return;
                }

                println!(
                    "Unable to logon to Steam: {:?} / {:?}",
                    callback.result(),
                    callback.extended_result()
                );

                is_running.store(false, Ordering::SeqCst);
                return;
            }

            // save the current cellid somewhere. if we lose our saved server
            // list, we can use this when retrieving servers from the Steam
            // Directory.
            let cell_id = callback.cell_id();

            tokio::spawn(async move {
                if let Err(error) =
                    tokio::fs::write(CELL_ID_FILE, cell_id.to_string()).await
                {
                    eprintln!("Unable to write {CELL_ID_FILE}: {error}");
                }
            });

            println!("Successfully logged on! Press Ctrl+C to log off...");
        }));
    }

    {
        steam_client.subscribe::<LoggedOffCallback>(Box::new(move |callback| {
            println!("Logged off of Steam: {:?}", callback.result());
        }));
    }

    {
        let steam_user = steam_user.clone();

        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                println!("Received Ctrl+C, disconnecting...");
                let _ = steam_user.log_off();
            }
        });
    }

    println!("Connecting to Steam...");

    // initiate the connection
    steam_client.connect(None).await?;

    // create our callback handling loop
    while is_running.load(Ordering::SeqCst) {
        steam_client
            .run_wait_all_callbacks(Duration::from_secs(1))
            .await;
    }

    Ok(())
}
