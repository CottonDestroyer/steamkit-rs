//! Logs on to Steam with the modern authentication flow: the credentials are
//! exchanged for tokens through the `Authentication` unified service, and the
//! refresh token is then used to log on to the Steam network.

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

/// Reads the guard data out of the shared slot.
///
/// The callbacks are `Fn` closures, so the guard data lives in a shared slot
/// instead of a captured local.
fn read_guard_data(slot: &Mutex<Option<String>>) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Stores the guard data in the shared slot.
fn write_guard_data(slot: &Mutex<Option<String>>, data: Option<String>) {
    match slot.lock() {
        Ok(mut guard) => *guard = data,
        Err(poisoned) => *poisoned.into_inner() = data,
    }
}

/// Authenticates with the credentials and logs on to the Steam network.
async fn on_connected(
    client: Arc<SteamClient>,
    steam_user: Arc<SteamUser>,
    user: String,
    pass: String,
    guard_data: Arc<Mutex<Option<String>>>,
    is_running: Arc<AtomicBool>,
) {
    println!("Connected to Steam! Logging in '{user}'...");

    let should_remember_password = false;

    // Begin authenticating via credentials
    let details = AuthSessionDetails {
        username: Some(user),
        password: Some(pass),
        is_persistent_session: should_remember_password,
        // See the guard data comment below
        guard_data: read_guard_data(&guard_data),
        // `UserConsoleAuthenticator` is the default authenticator implementation
        // provided by SteamKit for ease of use which blocks the thread and asks
        // for user input to enter the code. However, if you require special
        // handling (e.g. you have the TOTP secret and can generate codes on the
        // fly), you can implement your own `IAuthenticator`.
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

    if let Some(new_guard_data) = poll_response.new_guard_data.clone() {
        // When using certain two factor methods (such as email 2fa), guard data
        // may be provided by Steam for use in future authentication sessions to
        // avoid triggering 2FA again (this works similarly to the old sentry
        // file system). Do note that this guard data is also a JWT token and has
        // an expiration date.
        write_guard_data(&guard_data, Some(new_guard_data));
    }

    // Logon to Steam with the access token we have received
    // Note that we are using RefreshToken for logging on here
    let log_on_details = LogOnDetails {
        username: Some(poll_response.account_name.clone()),
        access_token: Some(poll_response.refresh_token.clone()),
        // If you set is_persistent_session to true, this also must be set to
        // true for it to work correctly
        should_remember_password,
        ..Default::default()
    };

    if let Err(error) = steam_user.log_on(&log_on_details) {
        eprintln!("Unable to log on to Steam: {error}");
        is_running.store(false, Ordering::SeqCst);
    }

    // This is not required, but it is possible to parse the JWT access token to
    // see the scope and expiration date.
    parse_json_web_token(&poll_response.access_token, "AccessToken");
    parse_json_web_token(&poll_response.refresh_token, "RefreshToken");
}

/// Decodes a single base64url encoded segment of a JSON web token.
///
/// Done by hand to avoid pulling in a base64 crate just for this.
fn decode_base64_url(input: &str) -> Result<Vec<u8>> {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut lookup = [u8::MAX; 256];
    for (index, byte) in ALPHABET.iter().enumerate() {
        lookup[*byte as usize] = index as u8;
    }
    // Fix up base64url to normal base64
    lookup[b'-' as usize] = 62;
    lookup[b'_' as usize] = 63;

    let mut decoded = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }

        let value = lookup[byte as usize];
        if value == u8::MAX {
            anyhow::bail!("invalid base64 character '{}'", byte as char);
        }

        buffer = (buffer << 6) | u32::from(value);
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            decoded.push(((buffer >> bits) & 0xFF) as u8);
        }
    }

    Ok(decoded)
}

/// Parses a JSON web token and prints its payload.
///
/// This is simply showing how to parse a JWT, this is not required to login to
/// Steam.
fn parse_json_web_token(token: &str, name: &str) {
    // You can use a JWT library to do the parsing for you
    let Some(payload_segment) = token.split('.').nth(1) else {
        return;
    };

    let Ok(payload_bytes) = decode_base64_url(payload_segment) else {
        return;
    };

    // Payload can be parsed as JSON, and then fields such expiration date,
    // scope, etc can be accessed
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&payload_bytes) else {
        return;
    };

    // For brevity we will simply output formatted json to console
    let Ok(formatted) = serde_json::to_string_pretty(&payload) else {
        return;
    };

    println!("{name}: {formatted}");
    println!();
}

/// Logs on to Steam and logs off again.
#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);

    let (Some(user), Some(pass)) = (arguments.next(), arguments.next()) else {
        eprintln!("Sample1a: No username and password specified!");
        return Ok(());
    };

    // save our logon details
    let previously_stored_guard_data: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    // create our steamclient instance
    let steam_client = SteamClient::new();
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    let is_running = Arc::new(AtomicBool::new(true));

    // register a few callbacks we're interested in
    // these are registered with a callback manager, which will then route the
    // callbacks to the functions specified
    {
        let client = steam_client.clone();
        let steam_user = steam_user.clone();
        let user = user.clone();
        let pass = pass.clone();
        let guard_data = previously_stored_guard_data.clone();
        let is_running = is_running.clone();

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            let client = client.clone();
            let steam_user = steam_user.clone();
            let user = user.clone();
            let pass = pass.clone();
            let guard_data = guard_data.clone();
            let is_running = is_running.clone();

            tokio::spawn(async move {
                on_connected(client, steam_user, user, pass, guard_data, is_running).await;
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
        let steam_user = steam_user.clone();
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
        // in order for the callbacks to get routed, they need to be handled by the manager
        steam_client
            .run_wait_all_callbacks(Duration::from_secs(1))
            .await;
    }

    Ok(())
}
