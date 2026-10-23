//! The Steam client library name.

/// The name of the Steam client library that NetHook2 hooks into.
pub const STEAMCLIENT_DLL: &str = if cfg!(target_arch = "x86_64") {
    "steamclient64.dll"
} else {
    "steamclient.dll"
};
