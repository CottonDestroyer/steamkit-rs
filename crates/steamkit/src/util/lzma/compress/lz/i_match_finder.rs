//! Match finder traits from the LZMA SDK (`IMatchFinder`).
//!
//! The input is an owned `Vec<u8>` instead of a stream.

use std::fmt;

use crate::error::SteamError;

/// A read-only view over the bytes that feed the sliding window.
pub trait IInWindowStream {
    /// Sets the input stream.
    fn set_stream(&mut self, stream: Vec<u8>);

    /// Prepares the window for a new stream.
    fn init(&mut self);

    /// Releases the input stream.
    fn release_stream(&mut self);

    /// The byte at `index` relative to the current position.
    ///
    /// `index` may be negative, in which case it refers to a byte before the
    /// current position.
    fn get_index_byte(&self, index: i32) -> u8;

    /// The length of the match at `index` that is `distance` bytes away,
    /// limited to `limit` bytes.
    fn get_match_len(&self, index: i32, distance: u32, limit: u32) -> u32;

    /// The number of bytes that have not been consumed yet.
    fn get_num_available_bytes(&self) -> u32;
}

/// A match finder.
pub trait IMatchFinder: IInWindowStream + fmt::Debug {
    /// Creates the match finder for the given history and look-ahead sizes.
    fn create(
        &mut self,
        history_size: u32,
        keep_add_buffer_before: u32,
        match_max_len: u32,
        keep_add_buffer_after: u32,
    ) -> Result<(), SteamError>;

    /// Finds matches at the current position and writes them into `distances`
    /// as (length, distance - 1) pairs. Returns the number of values written.
    fn get_matches(&mut self, distances: &mut [u32]) -> u32;

    /// Skips `num` positions.
    fn skip(&mut self, num: u32);
}
