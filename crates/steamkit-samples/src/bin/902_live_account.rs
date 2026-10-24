//! A live check of what needs an account: persona state, the Dota 2 game
//! coordinator, a CDN download with the real depot key of an owned app, and
//! a long session.
//!
//! Logs on with the QR flow from sample 001. Visible changes are avoided: the
//! persona state only goes through `Invisible` (friends don't see it) and
//! back to `Offline`, the name is never changed, no chat message is sent and
//! no friend request is touched. The game coordinator check does tell Steam
//! the account is "playing" Dota 2 for a few seconds, which friends can't see
//! while the account is offline.
//!
//! The session is kept up for `STEAMKIT_SOAK_MINUTES` minutes (default 30),
//! asking Steam something every five minutes to show it still answers.
//! No token, key or SteamID is printed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use steamkit::base::generated::steam_language::EResult;
use steamkit::base::client_msg::ClientMsgProtobuf;
use steamkit::base::generated::steam_language::{EMsg, EPersonaState};
use steamkit::base::gc::client_msg_gc::ClientGCMsgProtobuf;
use steamkit::steam::authentication::auth_session_details::AuthSessionDetails;
use steamkit::steam::cdn::client::Client as CdnClient;
use steamkit::steam::cdn::server::Server;
use steamkit::steam::handlers::steam_content::steam_content::SteamContent;
use steamkit::steam::handlers::steam_friends::callbacks::{PersonaStateCallback, ProfileInfoCallback};
use steamkit::steam::handlers::steam_game_coordinator::callbacks::MessageCallback;
use steamkit::steam::handlers::steam_game_coordinator::steam_game_coordinator::SteamGameCoordinator;
use steamkit::types::depot_manifest::FileData;
use steamkit::types::game_id::GameID;
use steamkit_protos::dota2::{CMsgClientHello, EgcBaseClientMsg, ESourceEngine};
use steamkit_protos::steam::{c_msg_client_games_played, CMsgClientGamesPlayed};
use sha1::{Digest, Sha1};
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

/// Counter-Strike 2: free, so every account has it; its common depot's
/// manifest is downloaded with the account's real depot key.
const APP_ID: u32 = 730;
const DEPOT_ID: u32 = 731;

/// Dota 2, for the game coordinator.
const DOTA_APP_ID: u32 = 570;
const GC_CLIENT_WELCOME: u32 = EgcBaseClientMsg::KEMsgGcClientWelcome as u32;

/// How many servers are tried for each download before giving up.
const MAX_SERVER_TRIES: usize = 5;

/// How long the user has to scan the QR code.
const SCAN_TIMEOUT: Duration = Duration::from_secs(600);

/// How long a single check may wait for Steam.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the soak asks Steam something.
const SOAK_PROBE_EVERY: Duration = Duration::from_secs(300);

/// How long the session is kept up before the log off.
fn soak_duration() -> Duration {
    let minutes = std::env::var("STEAMKIT_SOAK_MINUTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30);
    Duration::from_secs(minutes * 60)
}

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
    /// The persona state Steam last reported for the account itself.
    own_persona_state: Mutex<Option<EPersonaState>>,
    /// Set when the Dota game coordinator welcomed the client.
    gc_welcome: AtomicBool,
    /// Set when the profile info of the account arrived.
    profile_info: AtomicBool,
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

/// The manifest ID of the public branch from the depot section of the PICS data.
fn public_manifest_id(depot: &steamkit::types::key_value::KeyValue) -> Option<u64> {
    let public = depot.get("manifests").get("public");
    public
        .get("gid")
        .as_string()
        .or_else(|| public.as_string())
        .and_then(|gid| gid.parse().ok())
}

/// Downloads every chunk of `file` from the first server that works and
/// returns the SHA-1 of the reassembled file.
async fn download_file(
    cdn: &CdnClient,
    servers: &[Server],
    file: &FileData,
    depot_key: &[u8],
) -> Result<Vec<u8>, String> {
    let mut data = vec![0u8; file.total_size as usize];
    let mut last_error = String::from("no server to try");

    for server in servers.iter().take(MAX_SERVER_TRIES) {
        let mut failed = None;
        for chunk in &file.chunks {
            let mut buffer = vec![0u8; chunk.uncompressed_length as usize];
            match cdn
                .download_depot_chunk_async(DEPOT_ID, chunk, server, &mut buffer, Some(depot_key), None, None)
                .await
            {
                Ok(written) => {
                    let start = chunk.offset as usize;
                    data[start..start + written].copy_from_slice(&buffer[..written]);
                }
                Err(error) => {
                    failed = Some(error.to_string());
                    break;
                }
            }
        }

        match failed {
            None => return Ok(Sha1::digest(&data).to_vec()),
            Some(error) => last_error = format!("{}: {error}", server.host().unwrap_or("?")),
        }
    }

    Err(last_error)
}

/// Asks Steam something that needs a live, logged-on session.
async fn probe_session(unified_messages: &SteamUnifiedMessages) -> Result<u32, String> {
    let request = CPlayerGetGameBadgeLevelsRequest { appid: Some(440) };
    let response = tokio::time::timeout(
        CHECK_TIMEOUT,
        unified_messages.send_message::<CPlayerGetGameBadgeLevelsRequest, CPlayerGetGameBadgeLevelsResponse>(
            "Player.GetGameBadgeLevels#1",
            request,
        ),
    )
    .await
    .map_err(|_| "timed out".to_string())?
    .map_err(|error| error.to_string())?;

    if response.result() == EResult::OK {
        Ok(response.body().player_level.unwrap_or_default())
    } else {
        Err(format!("answered {:?}", response.result()))
    }
}

/// Tells Steam which game (if any) the account is playing.
fn set_game_played(steam_client: &SteamClient, app_id: Option<u32>) {
    let mut play_game = ClientMsgProtobuf::<CMsgClientGamesPlayed>::new(EMsg::ClientGamesPlayed);
    if let Some(app_id) = app_id {
        play_game.body.games_played.push(c_msg_client_games_played::GamePlayed {
            game_id: Some(GameID::from_app_id(app_id as i32).to_u64()),
            ..Default::default()
        });
    }
    let _ = steam_client.send(&play_game);
}

/// The persona state: `Invisible` is not shown to friends, and the state goes
/// back to `Offline` (the state of a fresh logon) afterwards.
async fn check_persona(steam_friends: &SteamFriends, observed: &Observed, report: &mut Report) {
    *observed.own_persona_state.lock().unwrap() = None;
    if let Err(error) = steam_friends.set_persona_state(EPersonaState::Invisible) {
        report.fail("Persona state", error.to_string());
        return;
    }

    let confirmed = wait_for(CHECK_TIMEOUT, || {
        (*observed.own_persona_state.lock().unwrap() == Some(EPersonaState::Invisible)).then_some(())
    })
    .await;
    match confirmed {
        Some(()) => report.pass("Persona state", "Invisible confirmed by Steam"),
        None => report.fail(
            "Persona state",
            format!("Steam never reported Invisible (last: {:?})", *observed.own_persona_state.lock().unwrap()),
        ),
    }

    let _ = steam_friends.set_persona_state(EPersonaState::Offline);
}

/// The Dota 2 game coordinator: start "playing", say hello, wait for the welcome.
async fn check_game_coordinator(
    steam_client: &Arc<SteamClient>,
    game_coordinator: &SteamGameCoordinator,
    observed: &Observed,
    report: &mut Report,
) {
    set_game_played(steam_client, Some(DOTA_APP_ID));

    // The GC needs a moment after the game started, so the hello is repeated.
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let mut welcomed = false;
    while Instant::now() < deadline && !welcomed {
        let mut hello = ClientGCMsgProtobuf::<CMsgClientHello>::new(EgcBaseClientMsg::KEMsgGcClientHello as u32);
        hello.body.engine = Some(ESourceEngine::KEseSource2 as i32);
        let _ = game_coordinator.send(&hello, DOTA_APP_ID);

        welcomed = wait_for(Duration::from_secs(5), || observed.gc_welcome.load(Ordering::SeqCst).then_some(()))
            .await
            .is_some();
    }

    set_game_played(steam_client, None);

    if welcomed {
        report.pass("Game coordinator", "Dota 2 GC welcomed the client");
    } else {
        report.fail("Game coordinator", "no welcome (does the account have Dota 2?)");
    }
}

/// The account's own profile info, a read-only social request.
async fn check_profile_info(
    steam_client: &Arc<SteamClient>,
    steam_friends: &SteamFriends,
    observed: &Observed,
    report: &mut Report,
) {
    let Some(steam_id) = steam_client.steam_id() else {
        report.fail("Profile info", "no SteamID");
        return;
    };
    if let Err(error) = steam_friends.request_profile_info(steam_id) {
        report.fail("Profile info", error.to_string());
        return;
    }
    match wait_for(CHECK_TIMEOUT, || observed.profile_info.load(Ordering::SeqCst).then_some(())).await {
        Some(()) => report.pass("Profile info", "own profile received"),
        None => report.fail("Profile info", "never arrived"),
    }
}

/// A CDN download with the real depot key of an app the account owns.
async fn check_cdn(
    steam_client: &Arc<SteamClient>,
    steam_apps: &SteamApps,
    steam_content: &SteamContent,
    report: &mut Report,
) {
    let servers = match tokio::time::timeout(CHECK_TIMEOUT, steam_content.get_servers_for_steam_pipe(None, Some(20))).await {
        Ok(Ok(servers)) if !servers.is_empty() => servers,
        _ => {
            report.fail("CDN servers", "no content servers");
            return;
        }
    };

    // Access token, product info and the manifest ID of the public branch.
    let manifest_id = async {
        let tokens = steam_apps
            .pics_get_access_tokens(Some(APP_ID), None)
            .await
            .map_err(|error| error.to_string())?;
        let access_token = tokens.app_tokens().get(&APP_ID).copied().unwrap_or(0);
        let request = PICSRequest::new(APP_ID, access_token);
        let result_set = steam_apps
            .pics_get_product_info(Some(&request), None, false)
            .await
            .map_err(|error| error.to_string())?;
        result_set
            .results
            .iter()
            .flatten()
            .find_map(|callback| callback.apps().get(&APP_ID))
            .and_then(|app| public_manifest_id(app.key_values().get("depots").get(&DEPOT_ID.to_string())))
            .ok_or_else(|| "no public manifest for the depot".to_string())
    };
    let manifest_id = match tokio::time::timeout(CHECK_TIMEOUT, manifest_id).await {
        Ok(Ok(id)) => id,
        Ok(Err(error)) => return report.fail("CDN manifest ID", error),
        Err(_) => return report.fail("CDN manifest ID", "timed out"),
    };
    report.pass("CDN manifest ID", format!("depot {DEPOT_ID}: {manifest_id}"));

    let depot_key = match tokio::time::timeout(CHECK_TIMEOUT, steam_apps.get_depot_decryption_key(DEPOT_ID, APP_ID)).await {
        Ok(Ok(key)) if key.result() == EResult::OK => key.depot_key().to_vec(),
        Ok(Ok(key)) => return report.fail("CDN depot key", format!("answered {:?}", key.result())),
        Ok(Err(error)) => return report.fail("CDN depot key", error.to_string()),
        Err(_) => return report.fail("CDN depot key", "timed out"),
    };

    let request_code = match tokio::time::timeout(
        CHECK_TIMEOUT,
        steam_content.get_manifest_request_code(DEPOT_ID, APP_ID, manifest_id, None, None),
    )
    .await
    {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => return report.fail("CDN request code", error.to_string()),
        Err(_) => return report.fail("CDN request code", "timed out"),
    };

    // An auth token for the first server; the answer shows whether the CDN asks for one.
    if let Some(host) = servers[0].host() {
        match tokio::time::timeout(CHECK_TIMEOUT, steam_content.get_cdn_auth_token(APP_ID, DEPOT_ID, host)).await {
            Ok(Ok(token)) if token.result == EResult::OK => report.pass("CDN auth token", "granted (not shown)"),
            Ok(Ok(token)) => report.pass("CDN auth token", format!("answered {:?}", token.result)),
            Ok(Err(error)) => report.fail("CDN auth token", error.to_string()),
            Err(_) => report.fail("CDN auth token", "timed out"),
        }
    }

    let cdn = CdnClient::from_steam_client(steam_client);
    let mut manifest = None;
    let mut last_error = String::from("no server to try");
    for server in servers.iter().take(MAX_SERVER_TRIES) {
        match cdn
            .download_manifest_async(DEPOT_ID, manifest_id, request_code, server, Some(&depot_key), None, None)
            .await
        {
            Ok(downloaded) => {
                manifest = Some(downloaded);
                break;
            }
            Err(error) => last_error = format!("{}: {error}", server.host().unwrap_or("?")),
        }
    }
    let Some(manifest) = manifest else {
        return report.fail("CDN manifest", last_error);
    };
    let files = manifest.files().unwrap_or_default();
    if manifest.filenames_encrypted() || files.iter().any(|file| file.file_name.is_empty()) {
        return report.fail("CDN manifest", "filenames are still encrypted");
    }
    report.pass(
        "CDN manifest",
        format!("{} files, {} bytes, names readable", files.len(), manifest.total_uncompressed_size()),
    );

    // The smallest file with several chunks if there is one, else the smallest with data.
    let candidates = || {
        files
            .iter()
            .filter(|file| file.total_size > 0 && file.link_target.as_deref().map_or(true, str::is_empty))
    };
    let file = candidates()
        .filter(|file| file.chunks.len() > 1)
        .min_by_key(|file| file.total_size)
        .or_else(|| candidates().filter(|file| !file.chunks.is_empty()).min_by_key(|file| file.total_size));
    let Some(file) = file else {
        return report.fail("CDN chunks", "the manifest has no file with data");
    };
    match download_file(&cdn, &servers, file, &depot_key).await {
        Ok(sha) if sha == file.file_hash => report.pass(
            "CDN chunks + SHA-1",
            format!("{} bytes in {} chunk(s) match the manifest", file.total_size, file.chunks.len()),
        ),
        Ok(_) => report.fail("CDN chunks + SHA-1", "hash differs from the manifest"),
        Err(error) => report.fail("CDN chunks + SHA-1", error),
    }
}

/// Keeps the session up, asking Steam something every few minutes.
async fn soak(
    steam_client: &Arc<SteamClient>,
    unified_messages: &SteamUnifiedMessages,
    observed: &Observed,
    report: &mut Report,
) {
    let total = soak_duration();
    println!("        (soak: keeping the session up for {} minutes...)", total.as_secs() / 60);

    let moves_before = observed.server_moves.load(Ordering::SeqCst);
    let start = Instant::now();
    let mut probes = 0;
    let mut probe_failures = Vec::new();

    while start.elapsed() < total {
        let wait = SOAK_PROBE_EVERY.min(total.saturating_sub(start.elapsed()));
        tokio::time::sleep(wait).await;

        if observed.disconnected.lock().unwrap().is_some() || !steam_client.is_connected() {
            report.fail(
                "Session soak",
                format!("disconnected after {} minutes", start.elapsed().as_secs() / 60),
            );
            return;
        }

        probes += 1;
        match probe_session(unified_messages).await {
            Ok(_) => println!("        ({} min: session answers)", start.elapsed().as_secs() / 60),
            Err(error) => probe_failures.push(format!("{} min: {error}", start.elapsed().as_secs() / 60)),
        }
    }

    let moves = observed.server_moves.load(Ordering::SeqCst) - moves_before;
    if probe_failures.is_empty() {
        report.pass(
            "Session soak",
            format!("{} minutes, {probes} probes answered, {moves} server move(s)", total.as_secs() / 60),
        );
    } else {
        report.fail("Session soak", probe_failures.join("; "));
    }
}

/// The checks that need a logged-on session.
#[allow(clippy::too_many_arguments)]
async fn run_checks(
    steam_client: &Arc<SteamClient>,
    steam_friends: &SteamFriends,
    steam_apps: &SteamApps,
    steam_content: &SteamContent,
    game_coordinator: &SteamGameCoordinator,
    unified_messages: &SteamUnifiedMessages,
    observed: &Observed,
    report: &mut Report,
) {
    match steam_client.steam_id() {
        Some(steam_id) if steam_id.is_individual_account() => report.pass("SteamID", "assigned, an individual account"),
        _ => report.fail("SteamID", "missing or not an individual account"),
    }

    match probe_session(unified_messages).await {
        Ok(level) => report.pass("Unified message", format!("player level {level}")),
        Err(error) => report.fail("Unified message", error),
    }

    check_persona(steam_friends, observed, report).await;
    check_profile_info(steam_client, steam_friends, observed, report).await;
    check_game_coordinator(steam_client, game_coordinator, observed, report).await;
    check_cdn(steam_client, steam_apps, steam_content, report).await;
    soak(steam_client, unified_messages, observed, report).await;
}

/// Logs on with a QR code and checks what needs an account.
#[tokio::main]
async fn main() -> Result<()> {
    let steam_client = SteamClient::new();
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;
    let steam_friends = steam_client.get_required_handler::<SteamFriends>()?;
    let steam_apps = steam_client.get_required_handler::<SteamApps>()?;
    let unified_messages = steam_client.get_required_handler::<SteamUnifiedMessages>()?;
    let steam_content = steam_client.get_required_handler::<SteamContent>()?;
    let game_coordinator = steam_client.get_required_handler::<SteamGameCoordinator>()?;

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
        let own_client = steam_client.clone();

        steam_client.subscribe::<PersonaStateCallback>(Box::new(move |callback| {
            if Some(callback.friend_id()) == own_client.steam_id() {
                *observed.own_persona_state.lock().unwrap() = Some(callback.state());
            }
        }));
    }

    {
        let observed = observed.clone();

        steam_client.subscribe::<MessageCallback>(Box::new(move |callback| {
            if callback.app_id() == DOTA_APP_ID && callback.e_msg() == GC_CLIENT_WELCOME {
                observed.gc_welcome.store(true, Ordering::SeqCst);
            }
        }));
    }

    {
        let observed = observed.clone();

        steam_client.subscribe::<ProfileInfoCallback>(Box::new(move |callback| {
            if callback.result() == EResult::OK {
                observed.profile_info.store(true, Ordering::SeqCst);
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
            &steam_content,
            &game_coordinator,
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
