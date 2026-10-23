//! NetHook2, rewritten in Rust.
//!
//! NetHook2 is a Windows DLL that gets injected into a running `Steam.exe`
//! and hooks the Steam client's networking code. Once hooked, it dumps every
//! message sent to and received from the Steam server.
//!
//! The DLL exports `Inject` and `Eject` and uses `DllMain` as its entry point,
//! so it's driven with `rundll32 "<path to dll>",Inject`.
//!
//! The protobuf types come from the `steamkit-protos` crate.

// This is a Windows DLL that talks to Win32 through the `windows` crate, so
// it's compiled out on other targets to keep `cargo check --workspace`
// working everywhere.
#![cfg(windows)]

pub mod binaryreader;
pub mod csimpledetour;
pub mod csimplescan;
pub mod crypto;
pub mod injector;
pub mod logger;
pub mod net;
pub mod nethook;
pub mod nh2_string;
pub mod sedebug;
pub mod sigscan;
pub mod steam;
pub mod steamclient;
pub mod version;
pub mod zip;

pub use injector::{Eject, Inject};
pub use nethook::DllMain;
