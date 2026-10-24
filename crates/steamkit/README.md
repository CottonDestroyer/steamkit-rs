# steamkit-rs

A Rust port of [SteamKit2](https://github.com/SteamRE/SteamKit), the .NET library for talking to the Steam network.

The package is `steamkit-rs` (the `steamkit` name was taken on crates.io), but the library is called `steamkit`:

```toml
[dependencies]
steamkit-rs = "0.1"
tokio = { version = "1", features = ["full"] }
```

## What it covers

- **Connections:** the CM protocol over TCP, WebSocket and UDP, with channel encryption, and the CM server list.
- **Authentication:** username and password (with Steam Guard) and QR code sign-in, both ending in a refresh token you can log on with.
- **Handlers:** `SteamUser`, `SteamFriends`, `SteamApps` (PICS, depot keys), `SteamContent`, unified messages, `SteamGameCoordinator`, `SteamCloud`, `SteamUserStats`, `SteamWorkshop` and more.
- **Content:** the CDN client, depot manifests and chunk decryption and decompression.
- **Web API:** typed access to the Steam Web API and the content server directory.
- **Messages:** every Steam, Dota 2, Counter-Strike, Team Fortress 2 and Deadlock protobuf message, from the `steamkit-rs-protos` crate.

## Example

Connect to Steam, log on anonymously and print the result:

```rust,no_run
use std::time::Duration;

use steamkit::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use steamkit::steam::handlers::steam_user::steam_user::SteamUser;
use steamkit::steam::steam_client::callbacks::ConnectedCallback;
use steamkit::steam::steam_client::steam_client::SteamClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SteamClient::new();
    let steam_user = client.get_required_handler::<SteamUser>()?;

    // Once connected, log on. Callbacks run while `run_wait_all_callbacks` is called.
    client.subscribe::<ConnectedCallback>(Box::new(move |_callback| {
        let _ = steam_user.log_on_anonymous();
    }));

    client.subscribe::<LoggedOnCallback>(Box::new(|callback| {
        println!("Logon result: {:?}", callback.result());
    }));

    client.connect(None).await?;

    for _ in 0..10 {
        client.run_wait_all_callbacks(Duration::from_secs(1)).await;
    }

    client.disconnect().await;
    Ok(())
}
```

Handlers are fetched with `get_required_handler`, events arrive as callbacks you `subscribe` to, and requests that have a reply (such as `SteamApps::pics_get_product_info`) are `async` functions that resolve with it.

## Samples

The [repository](https://github.com/CottonDestroyer/steamkit-rs/tree/main/crates/steamkit-samples) has ports of the SteamKit2 samples (login with a password or a QR code, friends, unified messages, game coordinators, the Web API) and two samples that download a depot from the CDN and check every file against its manifest.

## License

LGPL-2.1-only, the same as SteamKit2.
