# steamkit-rs-protos

Steam protobuf messages and enums for Rust, generated with [`prost`](https://crates.io/crates/prost) from Valve's `.proto` files. It is the message layer of [`steamkit-rs`](https://crates.io/crates/steamkit-rs), a port of SteamKit2, and can be used on its own.

```toml
[dependencies]
steamkit-rs-protos = "0.1.2"
```

The library is called `steamkit_protos`:

| Module | Contents | Feature |
|---|---|---|
| `steam` | Steam client messages | always on |
| `gc` | Shared game coordinator messages | always on |
| `webui` | Web UI service messages | `webui` |
| `csgo` | Counter-Strike game coordinator | `csgo` |
| `dota2` | Dota 2 game coordinator | `dota2` |
| `tf2` | Team Fortress 2 game coordinator | `tf2` |
| `deadlock` | Deadlock game coordinator | `deadlock` |

All the features are on by default. Each of those modules is a lot of generated code, so turn off the ones you don't use to build faster and use less memory. This keeps the Dota 2 messages and nothing else:

```toml
[dependencies]
steamkit-rs-protos = { version = "0.1.2", default-features = false, features = ["dota2"] }
```

```rust
use prost::Message;
use steamkit_protos::steam::CPlayerGetGameBadgeLevelsRequest;

let request = CPlayerGetGameBadgeLevelsRequest { appid: Some(440) };
let bytes = request.encode_to_vec();

let decoded = CPlayerGetGameBadgeLevelsRequest::decode(bytes.as_slice()).unwrap();
assert_eq!(decoded.appid, Some(440));
```

The proto files don't declare a package, so every file in a directory ends up in the same module.

The `.proto` files are compiled when the crate is built, with a vendored `protoc`, so no system `protoc` is needed.

## License

LGPL-2.1-only, the same as SteamKit2.
