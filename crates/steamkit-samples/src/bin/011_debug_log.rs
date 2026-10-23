//! Shows how to listen to the steamkit debug log.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::util::debug_log::{DebugLog, IDebugListener};

/// The debug log listener of this sample.
struct MyListener;

impl IDebugListener for MyListener {
    fn write_line(&self, category: &str, msg: &str) {
        // this function will be called when internal steamkit components write
        // to the debuglog
        //
        // for this example, we'll print the output to the console
        println!("MyListener - {category}: {msg}");
    }
}

/// Logs on to Steam with the debug log enabled.
#[tokio::main]
async fn main() -> Result<()> {
    // install our debug listeners for this example

    // install an instance of our custom listener
    // it's defined at the top of this file.
    DebugLog::add_listener(Arc::new(MyListener));

    // install a listener as an anonymous method
    // this call is commented as it would be redundant to install a second listener that also displays messages to the console
    // DebugLog::add_listener_function(|category, msg| println!("AnonymousMethod - {category}: {msg}"));

    // Enable DebugLog in release builds
    DebugLog::set_enabled(true);

    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample3: No username and password specified!");
        return Ok(());
    };

    // create our steamclient instance
    let steam_client = SteamClient::new();

    // uncomment this if you'd like to dump raw sent and received packets
    // that can be opened for analysis in the nethook analyzer
    // NOTE: dumps may contain sensitive data (such as your Steam password)
    // steam_client.set_debug_network_listener(Some(Arc::new(
    //     steamkit::util::debug_network_listener::NetHookNetworkListener::new(None)?,
    // )));

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
        let steam_user = steam_user.clone();
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

            println!("Successfully logged on!");

            // at this point, we'd be able to perform actions on Steam

            // for this sample we'll just log off
            let _ = steam_user.log_off();
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
