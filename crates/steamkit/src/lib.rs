#![doc = include_str!("../README.md")]
//!
//! # Modules
//!
//! The modules follow SteamKit2's namespaces:
//!
//! | SteamKit2 | Module |
//! |---|---|
//! | `SteamKit2.Base` | [`base`] |
//! | `SteamKit2.Base.GC` | [`base::gc`] |
//! | `SteamKit2.Base.Generated` | [`base::generated`] (re-exports `steamkit_protos`) |
//! | `SteamKit2.Networking.Steam3` | [`networking::steam3`] |
//! | `SteamKit2.SteamClient` | [`steam::steam_client`] |
//! | `SteamKit2.Internal` | [`steam::cm_client`], [`steam::handlers`] and friends |
//! | `SteamKit2.Authentication` | [`steam::authentication`] |
//! | `SteamKit2.CDN` | [`steam::cdn`] |
//! | `SteamKit2.Discovery` | [`steam::discovery`] |
//! | `SteamKit2.WebAPI` | [`steam::webapi`] |
//! | `SteamKit2.Types` | [`types`] |
//! | `SteamKit2.Util` | [`util`] |
//!
//! The main types are re-exported at the crate root.

#![forbid(unsafe_code)]

pub mod base;
pub mod error;
pub mod networking;
pub mod steam;
pub mod types;
pub mod util;

pub use error::SteamError;
