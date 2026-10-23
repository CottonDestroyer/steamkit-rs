//! Logs on to Steam with the QR code flow: Steam hands out a challenge URL
//! that is shown as a QR code, and the user confirms the login with the Steam
//! mobile app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::authentication::auth_session_details::AuthSessionDetails;
use steamkit::steam::handlers::steam_user::callbacks::{LoggedOffCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use tokio_util::sync::CancellationToken;

mod qr_code;

/// Draws the challenge URL of a QR authentication session as ASCII art.
fn draw_qr_code(challenge_url: &str) {
    println!("Challenge URL: {challenge_url}");
    println!();

    let Ok(code) = qr_code::QrCode::encode(challenge_url) else {
        println!("The challenge URL is too long to be rendered as a QR code.");
        return;
    };

    println!("Use the Steam Mobile App to sign in via QR code:");
    println!("{}", code.to_ascii_art());

    // The art above is drawn with block characters, which a Windows console
    // renders as "?" because of the OEM code page. The SVG is the reliable way
    // to scan it: open it in a browser and scan it off the screen.
    let path = std::env::temp_dir().join("steamkit-qr-login.svg");
    if std::fs::write(&path, code.to_svg()).is_ok() {
        println!("Or open this in a browser and scan it from there:");
        println!("  {}", path.display());
    }
}

/// The QR authentication state shared by every `Connected` callback.
///
/// The client moves to another CM server by itself when Steam asks it to,
/// and raises `Connected` again on the new server. Starting a fresh QR session
/// on every `Connected` would leave the old one polling in the background.
#[derive(Default)]
struct QrLogon {
    /// Cancels the QR session in progress, if any.
    current: Mutex<Option<CancellationToken>>,
    /// Set once the logon has been sent; the client repeats it on a new server
    /// by itself, so no new QR session is needed after that.
    logon_sent: AtomicBool,
}

impl QrLogon {
    /// Ends the QR session in progress and returns the token of a new one, or
    /// `None` when the logon has already been sent.
    fn restart(&self) -> Option<CancellationToken> {
        if self.logon_sent.load(Ordering::SeqCst) {
            return None;
        }

        let cancellation = CancellationToken::new();
        if let Some(previous) = self.current.lock().unwrap().replace(cancellation.clone()) {
            previous.cancel();
        }
        Some(cancellation)
    }
}

/// Starts a QR authentication session and logs on to the Steam network.
///
/// `cancellation` ends the session's polling and its QR refreshes.
async fn on_connected(
    client: Arc<SteamClient>,
    steam_user: Arc<SteamUser>,
    qr_logon: Arc<QrLogon>,
    cancellation: CancellationToken,
) {
    // Start an authentication session by requesting a link
    let auth_session = match client
        .authentication()
        .begin_auth_session_via_qr(&AuthSessionDetails::new())
        .await
    {
        Ok(session) => session,
        Err(error) => {
            eprintln!("Unable to begin the QR authentication session: {error}");
            return;
        }
    };

    // Steam will periodically refresh the challenge url, this callback allows
    // you to draw a new qr code
    let mut challenge_url_changed = auth_session.challenge_url_changed();
    let refresh_cancellation = cancellation.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = refresh_cancellation.cancelled() => break,
                changed = challenge_url_changed.recv() => {
                    let Ok(challenge_url) = changed else { break };

                    println!();
                    println!("Steam has refreshed the challenge url");

                    draw_qr_code(&challenge_url);
                }
            }
        }
    });

    // Draw current qr right away
    draw_qr_code(&auth_session.challenge_url());

    // Start polling Steam for authentication response
    // This response is later used to logon to Steam after connecting
    let poll_response = match auth_session.polling_wait_for_result(cancellation.clone()).await {
        Ok(response) => response,
        // A move to another CM server replaced this session with a new one.
        Err(_) if cancellation.is_cancelled() => return,
        Err(error) => {
            eprintln!("Unable to authenticate: {error}");
            return;
        }
    };

    qr_logon.logon_sent.store(true, Ordering::SeqCst);
    cancellation.cancel();

    println!("Logging in as '{}'...", poll_response.account_name);

    // Logon to Steam with the access token we have received
    let details = LogOnDetails {
        username: Some(poll_response.account_name.clone()),
        access_token: Some(poll_response.refresh_token.clone()),
        ..Default::default()
    };

    let _ = steam_user.log_on(&details);
}

/// Logs on to Steam with a QR code.
#[tokio::main]
async fn main() -> Result<()> {
    // create our steamclient instance
    let steam_client = SteamClient::new();
    // get the steamuser handler, which is used for logging on after successfully connecting
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;

    let is_running = Arc::new(AtomicBool::new(true));

    // register a few callbacks we're interested in
    {
        let client = steam_client.clone();
        let steam_user = steam_user.clone();
        let qr_logon = Arc::new(QrLogon::default());

        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            let Some(cancellation) = qr_logon.restart() else {
                return;
            };

            let client = client.clone();
            let steam_user = steam_user.clone();
            let qr_logon = qr_logon.clone();

            tokio::spawn(async move {
                on_connected(client, steam_user, qr_logon, cancellation).await;
            });
        }));
    }

    {
        let is_running = is_running.clone();

        steam_client.subscribe::<DisconnectedCallback>(Box::new(move |_callback| {
            // A move to another CM server is not announced as a disconnection,
            // so this is the end of the session.
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
        steam_client
            .run_wait_all_callbacks(Duration::from_secs(1))
            .await;
    }

    Ok(())
}
