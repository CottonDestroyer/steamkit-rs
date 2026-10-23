//! Live end-to-end depot test.
//!
//! A differential test against the real Steam network. It logs in
//! anonymously, asks the CM server for the depot decryption key and a CDN
//! auth token, downloads the depot manifest and one chunk from the content
//! servers, decrypts the chunk, and compares the result with a reference file
//! (for example one downloaded with DepotDownloader).
//!
//! It needs the network, so it is ignored by default. To run it:
//!
//! ```text
//! STEAM_APP=228980 STEAM_DEPOT=228990 \
//! STEAM_MANIFEST=1829726630299308803 STEAM_CHUNK=0 \
//! STEAM_EXPECT=/path/to/the/reference/file \
//!   cargo test --test depot_e2e -- --ignored --nocapture
//! ```
//!
//! `STEAM_EXPECT` is optional. Without it the test still exercises the whole
//! path and prints the decrypted chunk's length and hash.

use std::fs;

use steamkit::base::generated::steam_language::EResult;
use steamkit::steam::cdn::client::Client as CdnClient;
use steamkit::steam::handlers::steam_apps::steam_apps::SteamApps;
use steamkit::steam::handlers::steam_content::steam_content::SteamContent;
use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use steamkit::steam::steam_client::callbacks::ConnectedCallback;
use steamkit::steam::handlers::steam_user::steam_user::SteamUser;
use steamkit::steam::steam_client::steam_client::SteamClient;

fn env_u32(name: &str) -> u32 {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} must be set"))
        .parse()
        .unwrap_or_else(|e| panic!("{name} is not a number: {e}"))
}

fn env_u64(name: &str) -> u64 {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} must be set"))
        .parse()
        .unwrap_or_else(|e| panic!("{name} is not a number: {e}"))
}

#[tokio::test]
#[ignore = "needs the network and a depot whose key the anonymous account can read"]
async fn downloads_and_decrypts_a_depot_chunk() {
    let app = env_u32("STEAM_APP");
    let depot = env_u32("STEAM_DEPOT");
    let manifest_gid = env_u64("STEAM_MANIFEST");
    let file_index: usize = std::env::var("STEAM_FILE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let client = SteamClient::new();

    // A subscriber makes the CM conversation visible; run with RUST_LOG=debug.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();

    client.connect(None).await.expect("the CM connection must succeed");

    // `connect` returns as soon as the socket is up, but the CM server only
    // accepts a logon after the `Connected` event, so wait for that first.
    let (logon_tx, logon_rx) = tokio::sync::oneshot::channel();
    let logon_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(logon_tx)));
    let sender = logon_tx.clone();
    client.subscribe::<LoggedOnCallback>(Box::new(move |callback| {
        if let Ok(mut guard) = sender.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(callback.result());
            }
        }
    }));

    let steam_user = client
        .get_required_handler::<SteamUser>()
        .expect("the SteamUser handler must be registered");

    let (connected_tx, connected_rx) = tokio::sync::oneshot::channel();
    let connected_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(connected_tx)));
    let on_connected = connected_tx.clone();
    let logon_user = steam_user.clone();
    client.subscribe::<ConnectedCallback>(Box::new(move |_| {
        if let Ok(mut guard) = on_connected.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
            }
        }
        logon_user
            .log_on_anonymous()
            .expect("the anonymous logon request must be sent");
    }));

    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        connected_rx,
    )
    .await
    .expect("the CM connection must be established within 60s")
    .expect("the CM connection must be established");

    let logon_result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        logon_rx,
    )
    .await
    .expect("the anonymous logon must be answered within 60s")
    .expect("the anonymous logon must be answered");
    println!("anonymous logon: {logon_result:?}");
    assert!(
        logon_result == EResult::OK,
        "the anonymous logon must succeed, got {logon_result:?}"
    );

    let apps = client
        .get_required_handler::<SteamApps>()
        .expect("the SteamApps handler must be registered");
    let key = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        apps.get_depot_decryption_key(depot, app),
    )
    .await
    .expect("the depot key request must be answered within 60s")
    .expect("the depot key request must succeed")
    .depot_key()
    .to_vec();
    assert_eq!(key.len(), 16, "a depot key is 16 bytes");
    println!("depot key: {}", hex(&key));

    let content = client
        .get_required_handler::<SteamContent>()
        .expect("the SteamContent handler must be registered");
    let servers = content
        .get_servers_for_steam_pipe(None, None)
        .await
        .expect("the content server list must arrive");
    assert!(!servers.is_empty(), "there must be at least one content server");
    let server = &servers[0];
    println!(
        "content server: host={:?} port={} protocol={:?}",
        server.host(),
        server.port(),
        server.protocol()
    );

    let token = content
        .get_cdn_auth_token(app, depot, server.host().unwrap_or_default())
        .await
        .expect("the CDN auth token request must succeed");
    println!("cdn auth token: result={:?} length={}", token.result, token.token.len());
    let auth = token.token.as_str();

    let cdn = CdnClient::from_steam_client(&client);
    let manifest = cdn
        .download_manifest_async(depot, manifest_gid, 0, server, Some(&key), None, Some(auth))
        .await
        .expect("the manifest must download");

    let files = manifest.files().expect("the manifest must have files");
    println!(
        "manifest: depot={} gid={} files={} encrypted={}",
        manifest.depot_id(),
        manifest.manifest_gid(),
        files.len(),
        manifest.filenames_encrypted()
    );

    let file = files
        .get(file_index)
        .expect("the manifest must have the requested file");
    println!(
        "file {file_index}: {} size={} chunks={}",
        file.file_name, file.total_size, file.chunks.len()
    );

    // A depot file is the concatenation of its chunks, each fetched and
    // decompressed on its own, so that's what gets compared with the reference.
    let mut assembled: Vec<u8> = Vec::with_capacity(file.total_size as usize);
    for (chunk_index, chunk) in file.chunks.iter().enumerate() {
        let mut destination = vec![0u8; chunk.uncompressed_length as usize];
        let written = cdn
            .download_depot_chunk_async(
                depot,
                chunk,
                server,
                &mut destination,
                Some(&key),
                None,
                Some(auth),
            )
            .await
            .expect("the chunk must download and decrypt");
        println!(
            "  chunk {chunk_index}: id={} csize={} usize={} -> {written} bytes",
            chunk
                .chunk_id
                .as_deref()
                .map(hex)
                .unwrap_or_else(|| "none".to_owned()),
            chunk.compressed_length,
            chunk.uncompressed_length
        );
        assembled.extend_from_slice(&destination[..written]);
    }

    println!(
        "assembled {} bytes, sha256 {}",
        assembled.len(),
        hex(&sha256(&assembled))
    );

    if let Ok(expected_path) = std::env::var("STEAM_EXPECT") {
        let expected = fs::read(&expected_path)
            .unwrap_or_else(|e| panic!("reading the reference file {expected_path}: {e}"));
        assert_eq!(
            assembled.len(),
            expected.len(),
            "the assembled file is {} bytes, the reference is {}",
            assembled.len(),
            expected.len()
        );
        assert!(
            assembled == expected,
            "the assembled file differs from the reference"
        );
        println!("MATCHES the reference byte for byte");
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// A small SHA-256, so the test does not need another dependency.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_length = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());

    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let temp1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let temp2 = s0.wrapping_add(maj);

            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(temp1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = temp1.wrapping_add(temp2);
        }

        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}
