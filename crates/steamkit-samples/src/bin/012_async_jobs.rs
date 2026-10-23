//! Shows how job based messages are used with async/await.
//!
//! Some information about a Steam product is requested through the SteamApps
//! handler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_apps::callbacks::{
    DepotKeyCallback, PICSProductInfoCallback, PICSTokensCallback,
};
use steamkit::steam::handlers::steam_apps::steam_apps::{PICSRequest, SteamApps};
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;

/// Formats bytes as uppercase hex pairs separated by dashes (`0A-1B-2C`).
fn to_hex_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join("-")
}

/// Requests the depot key, the access token and the product info of an app.
async fn on_logged_on(
    steam_apps: Arc<SteamApps>,
    is_running: Arc<AtomicBool>,
) {
    // in this sample, we'll simply do a few async requests to acquire
    // information about Counter-Strike 2
    let app_id: u32 = 730;
    let depot_id: u32 = 731;

    // first, we'll request a depot decryption key for CSGO's common files depot (731)
    let depot_key: DepotKeyCallback = match steam_apps.get_depot_decryption_key(depot_id, app_id).await
    {
        Ok(depot_key) => depot_key,
        Err(error) => {
            eprintln!("Unable to request the depot key: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    if depot_key.result() == EResult::OK {
        println!("Got our depot key: {}", to_hex_string(depot_key.depot_key()));
    } else {
        println!("Unable to request depot key!");
    }

    // now request the access token
    let access_token_result: PICSTokensCallback =
        match steam_apps.pics_get_access_tokens(Some(app_id), None).await {
            Ok(result) => result,
            Err(error) => {
                eprintln!("Unable to request the access token: {error}");
                is_running.store(false, Ordering::SeqCst);
                return;
            }
        };

    // Get the access token, if the app is owned it may be a long value, or 0 if
    // it is public. If the request was denied, the appid will be listed in
    // app_tokens_denied.
    let access_token = access_token_result
        .app_tokens()
        .get(&app_id)
        .copied()
        .unwrap_or(0);

    // create a request product info request struct
    let request = PICSRequest::new(app_id, access_token);

    // now request some product info
    //
    // note that with some requests, Steam can return multiple results, so these
    // jobs don't return the callback object directly, but rather a result set
    // that could contain multiple callback objects if Steam gives us multiple
    // results
    let result_set = match steam_apps
        .pics_get_product_info(Some(&request), None, false)
        .await
    {
        Ok(result_set) => result_set,
        Err(error) => {
            eprintln!("Unable to request the product info: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    if result_set.complete {
        // the request fully completed, we can handle the data
        if let Some(product_info) = result_set.results.as_ref().and_then(|results| results.first())
        {
            // ... do something with our product info
            println!(
                "Received the product info of {app_id}: {:?}",
                product_info.apps().len()
            );
        }
    } else if result_set.failed {
        // the request partially completed, and then Steam encountered a remote
        // failure. for async jobs with only a single result (such as
        // get_depot_decryption_key), this would normally be an error. but since
        // Steam had given us a partial set of callbacks we get to decide what
        // to do with the data
        //
        // keep in mind that if Steam immediately fails to provide any data, or
        // times out while waiting for the first result, the job returns an
        // error instead
        //
        // the result set might not have our data, so we need to test to see if we
        // have results for our request
        if find_product_info(&result_set.results, app_id).is_some() {
            // we were lucky and Steam gave us the info we requested before failing
        } else {
            // bad luck
            println!("Steam failed before it sent the product info of {app_id}.");
        }
    } else {
        // the request partially completed, but then we timed out. essentially the
        // same as the previous case, but Steam didn't explicitly fail.
        //
        // we still need to check our result set to see if we have our data
        if find_product_info(&result_set.results, app_id).is_some() {
            // we were lucky and Steam gave us the info we requested before timing out
        } else {
            // bad luck
            println!("Steam timed out before it sent the product info of {app_id}.");
        }
    }

    // the job is just a future, so it can also be spawned as a task instead
    // of being awaited inline
    let steam_apps = steam_apps.clone();
    let is_running = is_running.clone();

    tokio::spawn(async move {
        let depot_key = match steam_apps.get_depot_decryption_key(depot_id, app_id).await {
            Ok(depot_key) => depot_key,
            Err(error) => {
                eprintln!("Unable to request the depot key: {error}");
                is_running.store(false, Ordering::SeqCst);
                return;
            }
        };

        // do something with the depot key
        println!("Got our depot key: {}", to_hex_string(depot_key.depot_key()));

        // we're finished with this sample, drop out of the callback loop
        is_running.store(false, Ordering::SeqCst);
    });
}

/// Finds the product info of an app in a result set.
fn find_product_info(
    results: &Option<Vec<PICSProductInfoCallback>>,
    app_id: u32,
) -> Option<&PICSProductInfoCallback> {
    results
        .as_ref()
        .and_then(|results| results.iter().find(|callback| callback.apps().contains_key(&app_id)))
}

/// Logs on to Steam and requests some product information.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample9: No username and password specified!");
        return Ok(());
    };

    // create our steamclient instance
    let steam_client = SteamClient::new();
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    // get our steamapps handler, we'll use this as an example of how async jobs
    // can be handled
    let steam_apps = steam_client.get_required_handler::<SteamApps>()?;

    let is_running = Arc::new(AtomicBool::new(true));

    // register a few callbacks we're interested in
    //
    // notice that we're not subscribing to the PICSProductInfoCallback
    // callback here (or other SteamApps callbacks) since this sample is using
    // the async job directly, we no longer need to subscribe to the callback.
    // however, if we still wish to use callbacks (or have existing code which
    // subscribes to callbacks), they will continue to operate alongside direct
    // async job handling. (i.e.: steamclient will still post callbacks for any
    // async jobs that are completed)
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
        let steam_apps = steam_apps.clone();
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

            let steam_apps = steam_apps.clone();
            let is_running = is_running.clone();

            tokio::spawn(async move {
                on_logged_on(steam_apps, is_running).await;
            });
        }));
    }

    {
        steam_client.subscribe::<LoggedOffCallback>(Box::new(move |callback| {
            println!("Logged off of Steam: {:?}", callback.result());
        }));
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
