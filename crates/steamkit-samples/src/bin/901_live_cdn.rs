//! A live check of the content (CDN) side.
//!
//! Logs on anonymously (no account, no QR code), then walks the path a
//! downloader takes for Spacewar (app 480, depot 481, a few MB and free for
//! everyone): content server lists from the Web API and over the CM
//! connection, PICS for the manifest ID, the depot key, the manifest request
//! code, the manifest, and the chunks of every file, whose SHA-1 is
//! compared against the manifest. Every check prints PASS or FAIL.
//!
//! Nothing is written to disk and no key is printed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use sha1::{Digest, Sha1};
use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::cdn::client::Client as CdnClient;
use steamkit::steam::cdn::server::Server;
use steamkit::steam::handlers::steam_apps::steam_apps::{PICSRequest, SteamApps};
use steamkit::steam::handlers::steam_content::steam_content::SteamContent;
use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use steamkit::steam::handlers::steam_user::steam_user::SteamUser;
use steamkit::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use steamkit::steam::steam_client::steam_client::SteamClient;
use steamkit::steam::webapi::content_server_directory_service::ContentServerDirectoryService;
use steamkit::types::depot_manifest::{DepotManifest, FileData};

const APP_ID: u32 = 480;
const DEPOT_ID: u32 = 481;

/// Steamworks Common Redistributables: free for everyone, and its installers
/// are several MB, so their files are made of several chunks.
const BIG_APP_ID: u32 = 228980;
const BIG_DEPOT_ID: u32 = 228990;

/// How long a single check may wait for Steam.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// How many servers are tried for each download before giving up.
const MAX_SERVER_TRIES: usize = 5;

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

/// The manifest ID of the public branch from the depot section of the PICS data.
fn public_manifest_id(depot: &steamkit::types::key_value::KeyValue) -> Option<u64> {
    let public = depot.get("manifests").get("public");
    // Newer product info has `gid` below the branch, older has the ID itself.
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
    depot_id: u32,
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
                .download_depot_chunk_async(depot_id, chunk, server, &mut buffer, Some(depot_key), None, None)
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
            Some(error) => {
                last_error = format!("{}: {error}", server.host().unwrap_or("?"));
            }
        }
    }

    Err(last_error)
}

#[allow(clippy::too_many_arguments)]
async fn run_checks(
    steam_client: &Arc<SteamClient>,
    steam_apps: &SteamApps,
    steam_content: &SteamContent,
    report: &mut Report,
) {
    // The Web API directory needs no logon at all.
    let web_servers = match ContentServerDirectoryService::load(steam_client.configuration()).await {
        Ok(servers) if !servers.is_empty() => {
            report.pass("Server list (Web API)", format!("{} servers", servers.len()));
            servers
        }
        Ok(_) => {
            report.fail("Server list (Web API)", "empty");
            Vec::new()
        }
        Err(error) => {
            report.fail("Server list (Web API)", error.to_string());
            Vec::new()
        }
    };

    let cm_servers = match tokio::time::timeout(CHECK_TIMEOUT, steam_content.get_servers_for_steam_pipe(None, Some(20))).await {
        Ok(Ok(servers)) if !servers.is_empty() => {
            report.pass("Server list (CM)", format!("{} servers", servers.len()));
            servers
        }
        Ok(Ok(_)) => {
            report.fail("Server list (CM)", "empty");
            Vec::new()
        }
        Ok(Err(error)) => {
            report.fail("Server list (CM)", error.to_string());
            Vec::new()
        }
        Err(_) => {
            report.fail("Server list (CM)", "timed out");
            Vec::new()
        }
    };

    let servers = if cm_servers.is_empty() { web_servers } else { cm_servers };
    if servers.is_empty() {
        report.fail("Content servers", "none to download from");
        return;
    }

    // PICS: the manifest ID of the public branch.
    let product_info = async {
        let request = PICSRequest::new(APP_ID, 0);
        steam_apps.pics_get_product_info(Some(&request), None, false).await
    };
    let manifest_id = match tokio::time::timeout(CHECK_TIMEOUT, product_info).await {
        Ok(Ok(result_set)) => {
            let manifest_id = result_set
                .results
                .iter()
                .flatten()
                .find_map(|callback| callback.apps().get(&APP_ID))
                .and_then(|app| public_manifest_id(app.key_values().get("depots").get(&DEPOT_ID.to_string())));
            match manifest_id {
                Some(id) => {
                    report.pass("PICS manifest ID", format!("depot {DEPOT_ID}, public branch: {id}"));
                    id
                }
                None => {
                    report.fail("PICS manifest ID", "no public manifest for the depot");
                    return;
                }
            }
        }
        Ok(Err(error)) => {
            report.fail("PICS manifest ID", error.to_string());
            return;
        }
        Err(_) => {
            report.fail("PICS manifest ID", "timed out");
            return;
        }
    };

    let depot_key = match tokio::time::timeout(CHECK_TIMEOUT, steam_apps.get_depot_decryption_key(DEPOT_ID, APP_ID)).await {
        Ok(Ok(key)) if key.result() == EResult::OK => {
            report.pass("Depot key", format!("{} bytes (not shown)", key.depot_key().len()));
            key.depot_key().to_vec()
        }
        Ok(Ok(key)) => {
            report.fail("Depot key", format!("answered {:?}", key.result()));
            return;
        }
        Ok(Err(error)) => {
            report.fail("Depot key", error.to_string());
            return;
        }
        Err(_) => {
            report.fail("Depot key", "timed out");
            return;
        }
    };

    let request_code = match tokio::time::timeout(
        CHECK_TIMEOUT,
        steam_content.get_manifest_request_code(DEPOT_ID, APP_ID, manifest_id, None, None),
    )
    .await
    {
        Ok(Ok(code)) => {
            report.pass(
                "Manifest request code",
                if code == 0 { "0 (none granted; trying without)" } else { "granted" },
            );
            code
        }
        Ok(Err(error)) => {
            report.fail("Manifest request code", error.to_string());
            0
        }
        Err(_) => {
            report.fail("Manifest request code", "timed out");
            0
        }
    };

    let cdn = CdnClient::from_steam_client(steam_client);

    let mut manifest: Option<DepotManifest> = None;
    let mut last_error = String::from("no server to try");
    for server in servers.iter().take(MAX_SERVER_TRIES) {
        match cdn
            .download_manifest_async(DEPOT_ID, manifest_id, request_code, server, Some(&depot_key), None, None)
            .await
        {
            Ok(downloaded) => {
                report.pass(
                    "Manifest download",
                    format!(
                        "{}: {} files, {} bytes",
                        server.host().unwrap_or("?"),
                        downloaded.files().map_or(0, <[_]>::len),
                        downloaded.total_uncompressed_size()
                    ),
                );
                manifest = Some(downloaded);
                break;
            }
            Err(error) => last_error = format!("{}: {error}", server.host().unwrap_or("?")),
        }
    }
    let Some(manifest) = manifest else {
        report.fail("Manifest download", last_error);
        return;
    };

    if manifest.depot_id() == DEPOT_ID && manifest.manifest_gid() == manifest_id {
        report.pass("Manifest identity", "depot ID and manifest ID match the request");
    } else {
        report.fail(
            "Manifest identity",
            format!("got depot {} / manifest {}", manifest.depot_id(), manifest.manifest_gid()),
        );
    }

    let files = manifest.files().unwrap_or_default();
    if !manifest.filenames_encrypted() && files.iter().all(|file| !file.file_name.is_empty()) {
        report.pass("Filenames", "readable");
    } else {
        report.fail("Filenames", "still encrypted or empty");
    }

    // Every regular file with data: the depot is small, and it covers files
    // of one chunk as well as of several.
    let downloadable: Vec<&FileData> = files
        .iter()
        .filter(|file| file.total_size > 0 && !file.chunks.is_empty() && file.link_target.as_deref().map_or(true, str::is_empty))
        .collect();
    let Some(&file) = downloadable.iter().min_by_key(|file| file.total_size) else {
        report.fail("Chunk download", "the manifest has no file with data");
        return;
    };

    let mut mismatched = Vec::new();
    let mut bytes = 0;
    let mut chunks = 0;
    for file in &downloadable {
        match download_file(&cdn, DEPOT_ID, &servers, file, &depot_key).await {
            Ok(sha) if sha == file.file_hash => {
                bytes += file.total_size;
                chunks += file.chunks.len();
            }
            Ok(_) => mismatched.push(format!("{}: hash differs", file.file_name)),
            Err(error) => mismatched.push(format!("{}: {error}", file.file_name)),
        }
    }
    if mismatched.is_empty() {
        report.pass(
            "Chunk download + SHA-1",
            format!("{} files, {chunks} chunks, {bytes} bytes, every SHA-1 matches the manifest", downloadable.len()),
        );
    } else {
        report.fail("Chunk download + SHA-1", mismatched.join("; "));
    }

    // A wrong depot key must be refused, not produce garbage.
    let mut wrong_key = depot_key.clone();
    wrong_key[0] ^= 0xFF;
    let chunk = &file.chunks[0];
    let mut buffer = vec![0u8; chunk.uncompressed_length as usize];
    match cdn
        .download_depot_chunk_async(DEPOT_ID, chunk, &servers[0], &mut buffer, Some(&wrong_key), None, None)
        .await
    {
        Err(error) => report.pass("Wrong depot key rejected", error.to_string()),
        Ok(_) => report.fail("Wrong depot key rejected", "the chunk was accepted"),
    }

    // A CDN auth token: many servers don't need one, so only the answer is shown.
    if let Some(host) = servers[0].host() {
        match tokio::time::timeout(CHECK_TIMEOUT, steam_content.get_cdn_auth_token(APP_ID, DEPOT_ID, host)).await {
            Ok(Ok(token)) if token.result == EResult::OK => report.pass("CDN auth token", "granted (not shown)"),
            Ok(Ok(token)) => report.pass("CDN auth token", format!("answered {:?} (not required for this depot)", token.result)),
            Ok(Err(error)) => report.fail("CDN auth token", error.to_string()),
            Err(_) => report.fail("CDN auth token", "timed out"),
        }
    }
}

/// Everything up to the depot's manifest, for a depot that needs no account.
async fn fetch_manifest(
    steam_client: &Arc<SteamClient>,
    steam_apps: &SteamApps,
    steam_content: &SteamContent,
    servers: &[Server],
    app_id: u32,
    depot_id: u32,
) -> Result<(DepotManifest, Vec<u8>), String> {
    let request = PICSRequest::new(app_id, 0);
    let result_set = tokio::time::timeout(CHECK_TIMEOUT, steam_apps.pics_get_product_info(Some(&request), None, false))
        .await
        .map_err(|_| "PICS timed out".to_string())?
        .map_err(|error| error.to_string())?;
    let manifest_id = result_set
        .results
        .iter()
        .flatten()
        .find_map(|callback| callback.apps().get(&app_id))
        .and_then(|app| public_manifest_id(app.key_values().get("depots").get(&depot_id.to_string())))
        .ok_or("no public manifest for the depot")?;

    let key = tokio::time::timeout(CHECK_TIMEOUT, steam_apps.get_depot_decryption_key(depot_id, app_id))
        .await
        .map_err(|_| "depot key timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if key.result() != EResult::OK {
        return Err(format!("depot key: {:?}", key.result()));
    }
    let depot_key = key.depot_key().to_vec();

    let request_code = tokio::time::timeout(
        CHECK_TIMEOUT,
        steam_content.get_manifest_request_code(depot_id, app_id, manifest_id, None, None),
    )
    .await
    .map_err(|_| "request code timed out".to_string())?
    .map_err(|error| error.to_string())?;

    let cdn = CdnClient::from_steam_client(steam_client);
    let mut last_error = String::from("no server to try");
    for server in servers.iter().take(MAX_SERVER_TRIES) {
        match cdn
            .download_manifest_async(depot_id, manifest_id, request_code, server, Some(&depot_key), None, None)
            .await
        {
            Ok(manifest) => return Ok((manifest, depot_key)),
            Err(error) => last_error = format!("{}: {error}", server.host().unwrap_or("?")),
        }
    }
    Err(last_error)
}

/// Downloads the smallest file that is made of several chunks and compares
/// its SHA-1 to the manifest.
async fn run_multi_chunk(
    steam_client: &Arc<SteamClient>,
    steam_apps: &SteamApps,
    steam_content: &SteamContent,
    report: &mut Report,
) {
    let servers = match tokio::time::timeout(CHECK_TIMEOUT, steam_content.get_servers_for_steam_pipe(None, Some(20))).await {
        Ok(Ok(servers)) if !servers.is_empty() => servers,
        _ => {
            report.fail("Multi-chunk file", "no content servers");
            return;
        }
    };

    let (manifest, depot_key) =
        match fetch_manifest(steam_client, steam_apps, steam_content, &servers, BIG_APP_ID, BIG_DEPOT_ID).await {
            Ok(fetched) => fetched,
            Err(error) => {
                report.fail("Multi-chunk file", format!("depot {BIG_DEPOT_ID}: {error}"));
                return;
            }
        };

    let Some(file) = manifest
        .files()
        .unwrap_or_default()
        .iter()
        .filter(|file| file.chunks.len() > 1 && file.link_target.as_deref().map_or(true, str::is_empty))
        .min_by_key(|file| file.total_size)
    else {
        report.fail("Multi-chunk file", "the manifest has no file with several chunks");
        return;
    };

    let cdn = CdnClient::from_steam_client(steam_client);
    match download_file(&cdn, BIG_DEPOT_ID, &servers, file, &depot_key).await {
        Ok(sha) if sha == file.file_hash => report.pass(
            "Multi-chunk file + SHA-1",
            format!("{} ({} bytes, {} chunks) matches the manifest", file.file_name, file.total_size, file.chunks.len()),
        ),
        Ok(_) => report.fail("Multi-chunk file + SHA-1", format!("{}: hash differs", file.file_name)),
        Err(error) => report.fail("Multi-chunk file + SHA-1", error),
    }
}

/// Logs on anonymously and checks the content download path.
#[tokio::main]
async fn main() -> Result<()> {
    let steam_client = SteamClient::new();
    let steam_user = steam_client.get_required_handler::<SteamUser>()?;
    let steam_apps = steam_client.get_required_handler::<SteamApps>()?;
    let steam_content = steam_client.get_required_handler::<SteamContent>()?;

    let logged_on: Arc<Mutex<Option<EResult>>> = Arc::new(Mutex::new(None));
    let disconnected = Arc::new(AtomicBool::new(false));

    {
        let steam_user = steam_user.clone();
        steam_client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
            let _ = steam_user.log_on_anonymous();
        }));
    }

    {
        let logged_on = logged_on.clone();
        steam_client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
            *logged_on.lock().unwrap() = Some(callback.result());
        }));
    }

    {
        let disconnected = disconnected.clone();
        steam_client.subscribe::<DisconnectedCallback>(Box::new(move |_callback| {
            disconnected.store(true, Ordering::SeqCst);
        }));
    }

    let pump_stop = tokio_util::sync::CancellationToken::new();
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
    println!();
    println!("Results:");

    let result = wait_for(CHECK_TIMEOUT, || *logged_on.lock().unwrap()).await;
    match result {
        Some(EResult::OK) => {
            report.pass("Anonymous logon", "accepted");
            run_checks(&steam_client, &steam_apps, &steam_content, &mut report).await;
            run_multi_chunk(&steam_client, &steam_apps, &steam_content, &mut report).await;
            let _ = steam_user.log_off();
            wait_for(CHECK_TIMEOUT, || disconnected.load(Ordering::SeqCst).then_some(())).await;
        }
        Some(other) => report.fail("Anonymous logon", format!("{other:?}")),
        None => report.fail("Anonymous logon", "no answer"),
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
