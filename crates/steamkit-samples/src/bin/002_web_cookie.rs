//! Logs on to Steam and shows how the access token is used to build the
//! `steamLoginSecure` cookie.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::authentication::auth_session_details::AuthSessionDetails;
use steamkit::steam::authentication::user_console_authenticator::UserConsoleAuthenticator;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use tokio_util::sync::CancellationToken;

/// Reads a string out of the shared slot.
fn read_slot(slot: &Mutex<String>) -> String {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Writes a string into the shared slot.
fn write_slot(slot: &Mutex<String>, value: String) {
    match slot.lock() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => *poisoned.into_inner() = value,
    }
}

/// Authenticates with the credentials and logs on to the Steam network.
async fn on_connected(
    client: Arc<SteamClient>,
    steam_user: Arc<SteamUser>,
    user: String,
    pass: String,
    access_token: Arc<Mutex<String>>,
    refresh_token: Arc<Mutex<String>>,
    is_running: Arc<AtomicBool>,
) {
    println!("Connected to Steam! Logging in '{user}'...");

    // Begin authenticating via credentials
    let details = AuthSessionDetails {
        username: Some(user),
        password: Some(pass),
        is_persistent_session: false,
        authenticator: Some(Arc::new(UserConsoleAuthenticator::new())),
        ..Default::default()
    };

    let auth_session = match client.authentication().begin_auth_session_via_credentials(&details).await
    {
        Ok(session) => session,
        Err(error) => {
            eprintln!("Unable to begin the authentication session: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    // Start polling Steam for authentication response
    let poll_response = match auth_session
        .polling_wait_for_result(CancellationToken::new())
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("Unable to authenticate: {error}");
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    // Logon to Steam with the access token we have received
    // Note that we are using RefreshToken for logging on here
    let log_on_details = LogOnDetails {
        username: Some(poll_response.account_name.clone()),
        access_token: Some(poll_response.refresh_token.clone()),
        ..Default::default()
    };

    if let Err(error) = steam_user.log_on(&log_on_details) {
        eprintln!("Unable to log on to Steam: {error}");
        is_running.store(false, Ordering::SeqCst);
        return;
    }

    // AccessToken can be used as the steamLoginSecure cookie
    // RefreshToken is required to generate new access tokens
    write_slot(&access_token, poll_response.access_token.clone());
    write_slot(&refresh_token, poll_response.refresh_token.clone());
}

/// Renews the access token once the client is logged on.
async fn on_logged_on(
    client: Arc<SteamClient>,
    steam_user: Arc<SteamUser>,
    steam_id: steamkit::types::steam_id::SteamID,
    access_token: Arc<Mutex<String>>,
    refresh_token: Arc<Mutex<String>>,
) {
    println!("Successfully logged on!");

    // This is how you concatenate the cookie, you can set it on the Steam
    // domains and it should work but actual usage of this will be left as an
    // exercise for the reader
    let _steam_login_secure = format!("{}||{}", steam_id.render(false), read_slot(&access_token));

    // The access token expires in 24 hours (at the time of writing) so you will
    // have to renew it. Parse this token with a JWT library to get the
    // expiration date and set up a timer to renew it. To renew you will have to
    // call this:
    // When allow_renewal is set to true, Steam may return a new refresh token
    let current_refresh_token = read_slot(&refresh_token);

    match client
        .authentication()
        .generate_access_token_for_app(steam_id, &current_refresh_token, false)
        .await
    {
        Ok(new_tokens) => {
            write_slot(&access_token, new_tokens.access_token.clone());

            if !new_tokens.refresh_token.is_empty() {
                write_slot(&refresh_token, new_tokens.refresh_token.clone());
            }
        }
        Err(error) => {
            eprintln!("Unable to generate a new access token: {error}");
        }
    }

    // Do not forget to update steamLoginSecure with the new accessToken!

    // for this sample we'll just log off
    let _ = steam_user.log_off();
}

/// Logs on to Steam and shows the web cookie.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        eprintln!("Sample1a: No username and password specified!");
        return Ok(());
    };

    let is_running = Arc::new(AtomicBool::new(true));
    let access_token = Arc::new(Mutex::new(String::new()));
    let refresh_token = Arc::new(Mutex::new(String::new()));

    // create our steamclient instance
    let steam_client = SteamClient::new();
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    // register a few callbacks we're interested in
    {
        let client = steam_client.clone();
        let steam_user = steam_user.clone();
        let user = user.clone();
        let pass = pass.clone();
        let access_token = access_token.clone();
        let refresh_token = refresh_token.clone();
        let is_running = is_running.clone();

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            let client = client.clone();
            let steam_user = steam_user.clone();
            let user = user.clone();
            let pass = pass.clone();
            let access_token = access_token.clone();
            let refresh_token = refresh_token.clone();
            let is_running = is_running.clone();

            tokio::spawn(async move {
                on_connected(
                    client,
                    steam_user,
                    user,
                    pass,
                    access_token,
                    refresh_token,
                    is_running,
                )
                .await;
            });
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
        let client = steam_client.clone();
        let steam_user = steam_user.clone();
        let access_token = access_token.clone();
        let refresh_token = refresh_token.clone();
        let is_running = is_running.clone();

        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
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

            let Some(steam_id) = callback.client_steam_id() else {
                is_running.store(false, Ordering::SeqCst);
                return;
            };

            let client = client.clone();
            let steam_user = steam_user.clone();
            let access_token = access_token.clone();
            let refresh_token = refresh_token.clone();

            tokio::spawn(async move {
                on_logged_on(client, steam_user, steam_id, access_token, refresh_token).await;
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
