//! A test bed for capturing packets with nethook, to be turned into tests
//! later.

use std::sync::Arc;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_apps::steam_apps::{PICSRequest, SteamApps};
use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use steamkit::steam::handlers::steam_user::steam_user::SteamUser;
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::util::debug_network_listener::NetHookNetworkListener;
use tokio_util::sync::CancellationToken;

/// Logs on to Steam anonymously and requests some product information.
#[tokio::main]
async fn main() -> Result<()> {
    let steam_client = SteamClient::new();
    let cancellation = CancellationToken::new();

    steam_client.set_debug_network_listener(Some(Arc::new(NetHookNetworkListener::new(None)?)));

    let steam_user = steam_client.get_required_handler::<SteamUser>()?;
    let steam_apps = steam_client.get_required_handler::<SteamApps>()?;

    {
        let steam_user = steam_user.clone();

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            println!("Connected to Steam! Logging in...");

            let _ = steam_user.log_on_anonymous();
        }));
    }

    {
        let cancellation = cancellation.clone();

        steam_client.subscribe::<DisconnectedCallback>(Box::new(move |_callback| {
            println!("Disconnected from Steam");

            cancellation.cancel();
        }));
    }

    {
        let steam_apps = steam_apps.clone();
        let steam_user = steam_user.clone();
        let cancellation = cancellation.clone();

        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
            if callback.result() != EResult::OK {
                println!("Unable to logon to Steam: {:?}", callback.result());
                cancellation.cancel();
                return;
            }

            println!("Successfully logged on!");

            let steam_apps = steam_apps.clone();
            let steam_user = steam_user.clone();

            tokio::spawn(async move {
                let request = PICSRequest::new(480, 0);

                if let Err(error) = steam_apps
                    .pics_get_product_info(Some(&request), None, false)
                    .await
                {
                    eprintln!("Unable to request the product info: {error}");
                }

                if let Err(error) = steam_apps
                    .pics_get_product_info(Some(&request), None, true)
                    .await
                {
                    eprintln!("Unable to request the product info: {error}");
                }

                let _ = steam_user.log_off();
            });
        }));
    }

    println!("Connecting to Steam...");

    steam_client.connect(None).await?;

    while !cancellation.is_cancelled() {
        match steam_client
            .wait_for_callback_async(cancellation.clone())
            .await
        {
            Ok(callback) => steam_client.callback_manager().dispatch(callback),
            Err(_) => break,
        }
    }

    Ok(())
}
