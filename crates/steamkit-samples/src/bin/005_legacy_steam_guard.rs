//! The old login flow with SteamGuard. The server answers the first logon
//! attempt with `AccountLogonDenied` and emails an auth code to the account;
//! the second attempt has to include that code.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use tokio::io::{AsyncBufReadExt, BufReader};

/// The codes that are entered by the user.
#[derive(Clone, Debug, Default)]
struct GuardCodes {
    /// The auth code that was sent to the email address of the account.
    auth_code: Option<String>,

    /// The code of the authenticator app.
    two_factor_code: Option<String>,
}

/// Reads a value out of the shared slot.
fn read_slot<T: Clone>(slot: &Mutex<T>) -> T {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Writes a value into the shared slot.
fn write_slot<T>(slot: &Mutex<T>, value: T) {
    match slot.lock() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => *poisoned.into_inner() = value,
    }
}

/// Reads a single line from the console.
async fn read_line(reader: &Arc<tokio::sync::Mutex<BufReader<tokio::io::Stdin>>>) -> Option<String> {
    let mut line = String::new();

    let read = {
        let mut guard = reader.lock().await;
        guard.read_line(&mut line).await.ok()?
    };

    if read == 0 {
        return None;
    }

    Some(line.trim().to_string())
}

/// Asks the user for the 2 factor code of their authenticator app.
async fn ask_for_two_factor_code(
    reader: Arc<tokio::sync::Mutex<BufReader<tokio::io::Stdin>>>,
    codes: Arc<Mutex<GuardCodes>>,
) {
    print!("Please enter your 2 factor auth code from your authenticator app: ");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    if let Some(code) = read_line(&reader).await {
        write_slot(&codes, GuardCodes {
            auth_code: read_slot(&codes).auth_code,
            two_factor_code: Some(code),
        });
    }
}

/// Asks the user for the auth code that was sent to their email address.
async fn ask_for_auth_code(
    reader: Arc<tokio::sync::Mutex<BufReader<tokio::io::Stdin>>>,
    codes: Arc<Mutex<GuardCodes>>,
    email_domain: String,
) {
    print!("Please enter the auth code sent to the email at {email_domain}: ");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    if let Some(code) = read_line(&reader).await {
        write_slot(&codes, GuardCodes {
            auth_code: Some(code),
            two_factor_code: read_slot(&codes).two_factor_code,
        });
    }
}

/// Logs on to Steam and handles the SteamGuard prompt.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        println!("Sample5: No username and password specified!");
        return Ok(());
    };

    let codes = Arc::new(Mutex::new(GuardCodes::default()));

    // create our steamclient instance
    let steam_client = SteamClient::new();
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    let reader = Arc::new(tokio::sync::Mutex::new(BufReader::new(
        tokio::io::stdin(),
    )));

    let is_running = Arc::new(AtomicBool::new(true));

    // register a few callbacks we're interested in
    {
        let steam_user = steam_user.clone();
        let user = user.clone();
        let pass = pass.clone();
        let codes = codes.clone();

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            println!("Connected to Steam! Logging in '{user}'...");

            let current = read_slot(&codes);

            let details = LogOnDetails {
                username: Some(user.clone()),
                password: Some(pass.clone()),

                // in this sample, we pass in an additional authcode
                // this value will be None (the default) for our first logon attempt
                auth_code: current.auth_code,

                // if the account is using 2-factor auth, we'll provide the two factor code instead
                // this will also be None on our first logon attempt
                two_factor_code: current.two_factor_code,
                ..Default::default()
            };

            let _ = steam_user.log_on(&details);
        }));
    }

    {
        let client = steam_client.clone();
        let reconnect_client = steam_client.clone();

        client.subscribe::<DisconnectedCallback>(Box::new(move |_callback| {
            // after receiving an AccountLogonDenied, we'll be disconnected from steam
            // so after we read an authcode from the user, we need to reconnect to
            // begin the logon flow again
            println!("Disconnected from Steam, reconnecting in 5...");

            let steam_client = reconnect_client.clone();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;

                let _ = steam_client.connect(None).await;
            });
        }));
    }

    {
        let reader = reader.clone();
        let codes = codes.clone();
        let is_running = is_running.clone();

        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
            let is_steam_guard = callback.result() == EResult::AccountLogonDenied;
            let is_2fa = callback.result() == EResult::AccountLoginDeniedNeedTwoFactor;

            if is_steam_guard || is_2fa {
                println!("This account is SteamGuard protected!");

                if is_2fa {
                    let reader = reader.clone();
                    let codes = codes.clone();

                    tokio::spawn(async move {
                        ask_for_two_factor_code(reader, codes).await;
                    });
                } else {
                    let reader = reader.clone();
                    let codes = codes.clone();
                    let email_domain = callback.email_domain().unwrap_or_default().to_string();

                    tokio::spawn(async move {
                        ask_for_auth_code(reader, codes, email_domain).await;
                    });
                }

                return;
            }

            if callback.result() != EResult::OK {
                // The server can say "use a different CM server" in answer to a logon,
                // or before one is sent. The client moves to another server by itself
                // and repeats a logon that is waiting for its answer, so
                // this is not the end of the session.
                if callback.result() == EResult::TryAnotherCM
                    || callback.result() == EResult::ServiceUnavailable
                {
                    println!(
                        "The CM server asked for a different one; the client moves on its own."
                    );
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
