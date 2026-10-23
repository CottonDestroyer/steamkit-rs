# steamkit-rs

A Rust port of [SteamKit2](https://github.com/SteamRE/SteamKit), the .NET library for talking to the Steam network.

It covers the CM protocol (TCP, WebSocket and UDP transports, channel encryption), the handlers (`SteamUser`, `SteamFriends`, `SteamApps`, unified messages, game coordinator, ...), the authentication flows (credentials and QR code), the Web API, the CDN client and depot manifests. It also ports SteamKit's tooling: the `.steamd` code generator, the protobuf dumper, and the NetHook2 DLL plus its dump analyzer.

## Status

- Logging on works against live Steam over TCP (QR flow tested on a real account).
- A logged-on session works: friends list, unified messages, PICS product info and depot keys.
- The WebSocket transport connects, but Steam closes the session after a minute or two, so the default connection order is TCP first.
- CDN downloads, game coordinators and long-running sessions haven't been tested live yet.
- The NetHook2 DLL builds but hasn't been injected into a real Steam client.

## Building

```sh
cargo build --workspace
cargo test --workspace --no-fail-fast
```

The test suite needs no network access; the few live tests are `#[ignore]`d.

## Samples

`crates/steamkit-samples` has ports of the SteamKit2 samples. The QR login one needs no password:

```sh
cargo run -p steamkit-samples --bin 001_authentication_with_qr_code
```

Scan the code with the Steam mobile app. On Windows the console can't draw the block characters, so the code is also written to `%TEMP%\steamkit-qr-login.svg`.

`900_live_check` logs on the same way and runs a set of PASS/FAIL checks against a logged-on session. It doesn't change anything on the account.

## Crates

| Crate | What it is |
|---|---|
| `steamkit` | the library |
| `steamkit-protos` | generated protobuf messages (built from `proto/` at compile time) |
| `steamkit-tools` | `.steamd` code generator, protobuf generator / dumper, NetHook dump analyzer |
| `steamkit-nethook` | NetHook2, a Windows DLL that logs the Steam client's network traffic |
| `steamkit-samples` | the sample programs |
| `steamkit-tests` | the test suite |

## License

LGPL-2.1, the same as SteamKit2. See [LICENSE](LICENSE).
