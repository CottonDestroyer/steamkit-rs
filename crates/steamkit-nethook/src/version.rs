//! Build info, stamped in by `build.rs`: the build date, the short commit SHA,
//! the commit date and whether the working tree was dirty.

/// The date and time the DLL was built.
pub const BUILD_DATE: &str = match option_env!("NETHOOK_BUILD_DATE") {
    Some(value) => value,
    None => "unknown",
};

/// The short commit SHA the DLL was built from, or an empty string when git was unavailable.
pub const BUILT_FROM_COMMIT_SHA: &str = match option_env!("NETHOOK_COMMIT_SHA") {
    Some(value) => value,
    None => "",
};

/// The date of the commit the DLL was built from, or an empty string when git was unavailable.
pub const BUILT_FROM_COMMIT_DATE: &str = match option_env!("NETHOOK_COMMIT_DATE") {
    Some(value) => value,
    None => "",
};

/// Returns `true` when the working tree had uncommitted changes at build time.
pub fn built_from_dirty() -> bool {
    matches!(option_env!("NETHOOK_DIRTY"), Some("1"))
}
