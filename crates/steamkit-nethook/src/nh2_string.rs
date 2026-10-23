//! Byte string helpers for paths from the ANSI Win32 API. Only ASCII case is
//! folded, which is all that matters for Windows paths here.

/// Returns `true` when `haystack` ends with `needle`, comparing ASCII letters case-insensitively.
///
/// Used by the injector to match module and process names.
pub fn string_case_insensitive_ends_with(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }

    let start = haystack.len() - needle.len();

    haystack[start..]
        .iter()
        .zip(needle.iter())
        .all(|(left, right)| left.to_ascii_lowercase() == right.to_ascii_lowercase())
}

/// Returns `true` when the two byte strings are equal, comparing ASCII letters case-insensitively.
///
/// Used by the injector to compare two full module paths.
pub fn string_case_insensitive_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
}
