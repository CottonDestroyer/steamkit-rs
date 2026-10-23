//! A live check of a logged-on session.
//!
//! Logs on with the QR flow from sample 001, then tries what a logged-on
//! session can do (the SteamID, account info, friends list, a unified service
//! call, PICS product info and a depot key), keeps the session up for a
//! minute, and logs off. Every check prints PASS or FAIL.
//!
//! It doesn't change anything on the account: the persona state is never
//! set, so the account doesn't appear online to friends, and no friend
//! request is accepted. No token, key or SteamID is printed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::authentication::auth_session_details::AuthSessionDetails;
use steamkit::steam::handlers::steam_apps::steam_apps::{PICSRequest, SteamApps};
use steamkit::steam::handlers::steam_friends::callbacks::FriendsListCallback;
use steamkit::steam::handlers::steam_friends::steam_friends::SteamFriends;
use steamkit::steam::handlers::steam_unified_messages::steam_unified_messages::SteamUnifiedMessages;
use steamkit::steam::handlers::steam_user::callbacks::{AccountInfoCallback, LoggedOnCallback};
use steamkit::steam::handlers::steam_user::steam_user::{LogOnDetails, SteamUser};
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit_protos::steam::{CPlayerGetGameBadgeLevelsRequest, CPlayerGetGameBadgeLevelsResponse};
use tokio_util::sync::CancellationToken;

mod qr_code;

/// Counter-Strike 2: a free app, so its product info and its common depot's
/// key are available to every account.
const APP_ID: u32 = 730;
const DEPOT_ID: u32 = 731;

/// How long the user has to scan the QR code.
const SCAN_TIMEOUT: Duration = Duration::from_secs(600);

/// How long a single check may wait for Steam.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the session is kept up before the log off.
const KEEP_ALIVE: Duration = Duration::from_secs(60);

/// What the callbacks have seen, read by the checks.
#[derive(Default)]
struct Observed {
    /// The result and extended result of the logon, once Steam answered it.
    logged_on: Mutex<Option<(EResult, EResult)>>,
    /// How often Steam moved the session to another CM server.
    server_moves: AtomicUsize,
    /// Set when the account info arrived.
    account_info: AtomicBool,
    /// The number of entries of the first full friends list.
    friends_list: Mutex<Option<usize>>,
    /// `user_initiated` of the disconnection, once there was one.
    disconnected: Mutex<Option<bool>>,
}

/// The QR authentication state shared by every `Connected` callback; see the
/// same struct in sample 001.
#[derive(Default)]
struct QrLogon {
    current: Mutex<Option<CancellationToken>>,
    logon_sent: AtomicBool,
}

impl QrLogon {
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

/// The PASS / FAIL lines and their tally.
#[derive(Default)]
struct Report {
    passed: usize,
    failed: usize,
}

impl Report {
    fn pass(&mut self, check: &str, detail: impl AsRef<str>) {
        self.passed += 1;
        println!("  PASS  {check:<28} {}", detail.as_ref());
    }

    fn fail(&mut self, check: &str, detail: impl AsRef<str>) {
        self.failed += 1;
        println!("  FAIL  {check:<28} {}", detail.as_ref());
    }
}

/// Polls `probe` until it yields a value or `timeout` has passed.
async fn wait_for<T>(timeout: Duration, probe: impl Fn() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Draws the challenge URL as ASCII art and writes it as an SVG, as sample 001 does.
fn draw_qr_code(challenge_url: &str) {
    let Ok(code) = qr_code::QrCode::encode(challenge_url) else {
        println!("The challenge URL is too long to be rendered as a QR code.");
        return;
    };

    println!("Use the Steam Mobile App to sign in via QR code:");
    println!("{}", code.to_ascii_art());

    let path = std::env::temp_dir().join("steamkit-qr-login.svg");
    if std::fs::write(&path, code.to_svg()).is_ok() {
        println!("Or open this in a browser and scan it from there:");
        println!("  {}", path.display());
    }
}

/// Runs one QR authentication session and logs on with its result.
async fn authenticate(
    client: Arc<SteamClient>,
    steam_user: Arc<SteamUser>,
    qr_logon: Arc<QrLogon>,
    cancellation: CancellationToken,
) {
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

    let mut challenge_url_changed = auth_session.challenge_url_changed();
    let refresh_cancellation = cancellation.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = refresh_cancellation.cancelled() => break,
                changed = challenge_url_changed.recv() => {
                    let Ok(challenge_url) = changed else { break };

                    println!();
                    println!("Steam has refreshed the QR code.");

                    draw_qr_code(&challenge_url);
                }
            }
        }
    });

    draw_qr_code(&auth_session.challenge_url());

    let poll_response = match auth_session.polling_wait_for_result(cancellation.clone()).await {
        Ok(response) => response,
        Err(_) if cancellation.is_cancelled() => return,
        Err(error) => {
            eprintln!("Unable to authenticate: {error}");
            return;
        }
    };

    qr_logon.logon_sent.store(true, Ordering::SeqCst);
    cancellation.cancel();

    println!();
    println!("Approved; logging on...");

    let details = LogOnDetails {
        username: Some(poll_response.account_name.clone()),
        access_token: Some(poll_response.refresh_token.clone()),
        ..Default::default()
    };

    let _ = steam_user.log_on(&details);
}

/// The checks that need a logged-on session.
async fn run_checks(
    steam_client: &Arc<SteamClient>,
    steam_friends: &SteamFriends,
    steam_apps: &SteamApps,
    unified_messages: &SteamUnifiedMessages,
    observed: &Observed,
    report: &mut Report,
) {
    match steam_client.steam_id() {
        Some(steam_id) if steam_id.is_individual_account() => {
            report.pass("SteamID", "assigned, an individual account")
        }
        Some(_) => report.fail("SteamID", "assigned, but not an individual account"),
        None => report.fail("SteamID", "not assigned after the logon"),
    }

    match wait_for(CHECK_TIMEOUT, || observed.account_info.load(Ordering::SeqCst).then_some(())).await {
        Some(()) => report.pass(
            "Account info",
            if steam_friends.get_persona_name().is_some_and(|name| !name.is_empty()) {
                "received, with a persona name"
            } else {
                "received, but no persona name"
            },
        ),
        None => report.fail("Account info", "never arrived"),
    }

    match wait_for(CHECK_TIMEOUT, || *observed.friends_list.lock().unwrap()).await {
        Some(entries) => report.pass(
            "Friends list",
            format!("{entries} entries, {} of them friends", steam_friends.get_friend_count()),
        ),
        None => report.fail("Friends list", "never arrived"),
    }

    let request = CPlayerGetGameBadgeLevelsRequest { appid: Some(440) };
    let badge_levels = tokio::time::timeout(
        CHECK_TIMEOUT,
        unified_messages.send_message::<CPlayerGetGameBadgeLevelsRequest, CPlayerGetGameBadgeLevelsResponse>(
            "Player.GetGameBadgeLevels#1",
            request,
        ),
    )
    .await;
    match badge_levels {
        Ok(Ok(response)) if response.result() == EResult::OK => report.pass(
            "Unified message",
            format!(
                "Player.GetGameBadgeLevels: player level {}",
                response.body().player_level.unwrap_or_default()
            ),
        ),
        Ok(Ok(response)) => report.fail("Unified message", format!("answered {:?}", response.result())),
        Ok(Err(error)) => report.fail("Unified message", error.to_string()),
        Err(_) => report.fail("Unified message", "timed out"),
    }

    let product_info = async {
        let tokens = steam_apps.pics_get_access_tokens(Some(APP_ID), None).await?;
        let access_token = tokens.app_tokens().get(&APP_ID).copied().unwrap_or(0);
        let request = PICSRequest::new(APP_ID, access_token);
        steam_apps.pics_get_product_info(Some(&request), None, false).await
    };
    match tokio::time::timeout(CHECK_TIMEOUT, product_info).await {
        Ok(Ok(result_set)) => {
            let app = result_set
                .results
                .iter()
                .flatten()
                .find_map(|callback| callback.apps().get(&APP_ID));
            match app {
                Some(app) => {
                    let name = app.key_values().get("common").get("name").as_string().unwrap_or("?");
                    report.pass("PICS product info", format!("app {APP_ID} is \"{name}\""))
                }
                None => report.fail("PICS product info", format!("no entry for app {APP_ID}")),
            }
        }
        Ok(Err(error)) => report.fail("PICS product info", error.to_string()),
        Err(_) => report.fail("PICS product info", "timed out"),
    }

    match tokio::time::timeout(CHECK_TIMEOUT, steam_apps.get_depot_decryption_key(DEPOT_ID, APP_ID)).await {
        Ok(Ok(depot_key)) if depot_key.result() == EResult::OK => report.pass(
            "Depot key",
            format!("depot {DEPOT_ID}: {} byte key received (not shown)", depot_key.depot_key().len()),
        ),
        Ok(Ok(depot_key)) => report.fail("Depot key", format!("depot {DEPOT_ID}: answered {:?}", depot_key.result())),
        Ok(Err(error)) => report.fail("Depot key", error.to_string()),
        Err(_) => report.fail("Depot key", "timed out"),
    }

    println!("        (keeping the session up for {} seconds...)", KEEP_ALIVE.as_secs());
    let moves_before = observed.server_moves.load(Ordering::SeqCst);
    tokio::time::sleep(KEEP_ALIVE).await;
    let moves = observed.server_moves.load(Ordering::SeqCst) - moves_before;
    if observed.disconnected.lock().unwrap().is_none() && steam_client.is_connected() {
        report.pass(
            "Session stays up",
            format!("{} seconds, {moves} server move(s)", KEEP_ALIVE.as_secs()),
        );
    } else {
        report.fail("Session stays up", "disconnected while idle");
    }
}

/// Logs on with a QR code and checks what a logged-on session can do.
#[tokio::main]
async fn main() -> Result<()> {
    let steam_client = SteamClient::new();
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;
    let steam_friends = steam_client.get_required_handler::<SteamFriends>()?;
    let steam_apps = steam_client.get_required_handler::<SteamApps>()?;
    let unified_messages = steam_client.get_required_handler::<SteamUnifiedMessages>()?;

    let observed = Arc::new(Observed::default());

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
                authenticate(client, steam_user, qr_logon, cancellation).await;
            });
        }));
    }

    {
        let observed = observed.clone();

        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
            if callback.result() == EResult::TryAnotherCM || callback.result() == EResult::ServiceUnavailable {
                observed.server_moves.fetch_add(1, Ordering::SeqCst);
                println!("(Steam moved the session to another CM server: {:?})", callback.result());
                return;
            }

            *observed.logged_on.lock().unwrap() = Some((callback.result(), callback.extended_result()));
        }));
    }

    {
        let observed = observed.clone();

        steam_client.subscribe::<AccountInfoCallback>(Box::new(move |_callback| {
            observed.account_info.store(true, Ordering::SeqCst);
        }));
    }

    {
        let observed = observed.clone();

        steam_client.subscribe::<FriendsListCallback>(Box::new(move |callback| {
            if !callback.incremental() {
                observed.friends_list.lock().unwrap().get_or_insert(callback.friend_list().len());
            }
        }));
    }

    {
        let observed = observed.clone();

        steam_client.subscribe::<DisconnectedCallback>(Box::new(move |callback| {
            *observed.disconnected.lock().unwrap() = Some(callback.user_initiated());
        }));
    }

    let pump_stop = CancellationToken::new();
    let pump = {
        let client = steam_client.clone();
        let stop = pump_stop.clone();

        tokio::spawn(async move {
            while !stop.is_cancelled() {
                client.run_wait_all_callbacks(Duration::from_millis(200)).await;
            }
        })
    };

    println!("Connecting to Steam...");
    steam_client.connect(None).await?;

    let mut report = Report::default();

    let logon = wait_for(SCAN_TIMEOUT, || {
        if let Some(result) = *observed.logged_on.lock().unwrap() {
            return Some(Ok(result));
        }
        observed.disconnected.lock().unwrap().map(|_| Err(()))
    })
    .await;

    println!();
    println!("Results:");

    let logged_on = match logon {
        Some(Ok((EResult::OK, _))) => {
            report.pass("Logon", "accepted");
            true
        }
        Some(Ok((result, extended))) => {
            report.fail("Logon", format!("{result:?} / {extended:?}"));
            false
        }
        Some(Err(())) => {
            report.fail("Logon", "disconnected before the logon");
            false
        }
        None => {
            report.fail("Logon", "the QR code was not scanned in time");
            false
        }
    };

    let moves = observed.server_moves.load(Ordering::SeqCst);
    if moves > 0 {
        println!("        ({moves} server move(s) before the logon)");
    }

    if logged_on {
        run_checks(
            &steam_client,
            &steam_friends,
            &steam_apps,
            &unified_messages,
            &observed,
            &mut report,
        )
        .await;

        let _ = steam_user.log_off();

        match wait_for(CHECK_TIMEOUT, || *observed.disconnected.lock().unwrap()).await {
            Some(_) => {
                // A logoff must not be undone by a move to another server.
                tokio::time::sleep(Duration::from_secs(5)).await;
                if steam_client.is_connected() || steam_client.steam_id().is_some() {
                    report.fail("Log off", "the client reconnected after the log off");
                } else {
                    report.pass("Log off", "ended the session, and it stayed ended");
                }
            }
            None => report.fail("Log off", "no disconnection after the log off"),
        }
    }

    pump_stop.cancel();
    let _ = pump.await;
    steam_client.disconnect().await;

    println!();
    println!("{} passed, {} failed", report.passed, report.failed);

    if report.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
