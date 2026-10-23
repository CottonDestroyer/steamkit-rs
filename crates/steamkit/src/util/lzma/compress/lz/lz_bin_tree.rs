//! The binary tree match finder from the LZMA SDK (`LzBinTree`).
//!
//! `BinTree` is the "BT4" (or "BT2") match finder: a hash table that points
//! into a binary tree of candidate positions. Both are stored in flat
//! `Vec<u32>`s.

use crate::error::SteamError;
use crate::util::lzma::common::crc;

use super::i_match_finder::{IInWindowStream, IMatchFinder};
use super::lz_in_window::InWindow;

const HASH2_SIZE: u32 = 1 << 10;
const HASH3_SIZE: u32 = 1 << 16;
const BT2_HASH_SIZE: u32 = 1 << 16;
const START_MAX_LEN: u32 = 1;
const HASH3_OFFSET: u32 = HASH2_SIZE;
const EMPTY_HASH_VALUE: u32 = 0;
const MAX_VAL_FOR_NORMALIZE: u32 = (1 << 31) - 1;

/// The binary tree match finder.
#[derive(Debug, Clone)]
pub struct BinTree {
    window: InWindow,

    cyclic_buffer_pos: u32,
    cyclic_buffer_size: u32,
    match_max_len: u32,

    son: Vec<u32>,
    hash: Vec<u32>,

    cut_value: u32,
    hash_mask: u32,
    hash_size_sum: u32,

    hash_array: bool,

    num_hash_direct_bytes: u32,
    min_match_check: u32,
    fix_hash_size: u32,
}

impl Default for BinTree {
    fn default() -> Self {
        Self {
            window: InWindow::new(),
            cyclic_buffer_pos: 0,
            cyclic_buffer_size: 0,
            match_max_len: 0,
            son: Vec::new(),
            hash: Vec::new(),
            cut_value: 0xFF,
            hash_mask: 0,
            hash_size_sum: 0,
            hash_array: true,
            num_hash_direct_bytes: 0,
            min_match_check: 4,
            fix_hash_size: HASH2_SIZE + HASH3_SIZE,
        }
    }
}

impl BinTree {
    /// Creates a match finder with the default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the hash function: 4 byte hashes (the default) or 2 byte hashes.
    pub fn set_type(&mut self, num_hash_bytes: i32) {
        self.hash_array = num_hash_bytes > 2;
        if self.hash_array {
            self.num_hash_direct_bytes = 0;
            self.min_match_check = 4;
            self.fix_hash_size = HASH2_SIZE + HASH3_SIZE;
        } else {
            self.num_hash_direct_bytes = 2;
            self.min_match_check = 2 + 1;
            self.fix_hash_size = 0;
        }
    }

    /// The sliding window this match finder reads from.
    pub fn window(&self) -> &InWindow {
        &self.window
    }

    /// The sliding window this match finder reads from.
    pub fn window_mut(&mut self) -> &mut InWindow {
        &mut self.window
    }

    /// Prepares the match finder for a new stream.
    pub fn init(&mut self) {
        self.window.init();
        for slot in self.hash.iter_mut() {
            *slot = EMPTY_HASH_VALUE;
        }
        self.cyclic_buffer_pos = 0;
        self.window.reduce_offsets(-1);
    }

    /// Advances the current position by one byte.
    pub fn move_pos(&mut self) {
        self.cyclic_buffer_pos = self.cyclic_buffer_pos.wrapping_add(1);
        if self.cyclic_buffer_pos >= self.cyclic_buffer_size {
            self.cyclic_buffer_pos = 0;
        }
        self.window.move_pos();
        if self.window.pos == MAX_VAL_FOR_NORMALIZE {
            self.normalize();
        }
    }

    /// The byte at the absolute index `index` of the window buffer.
    #[inline]
    fn byte_at(&self, index: u32) -> u8 {
        self.window.buffer_base.get(index as usize).copied().unwrap_or(0)
    }

    #[inline]
    fn son_get(&self, index: u32) -> u32 {
        self.son.get(index as usize).copied().unwrap_or(0)
    }

    #[inline]
    fn son_set(&mut self, index: u32, value: u32) {
        if let Some(slot) = self.son.get_mut(index as usize) {
            *slot = value;
        }
    }

    #[inline]
    fn hash_get(&self, index: u32) -> u32 {
        self.hash.get(index as usize).copied().unwrap_or(0)
    }

    #[inline]
    fn hash_set(&mut self, index: u32, value: u32) {
        if let Some(slot) = self.hash.get_mut(index as usize) {
            *slot = value;
        }
    }

    /// Creates the match finder for the given history and look-ahead sizes.
    pub fn create(
        &mut self,
        history_size: u32,
        keep_add_buffer_before: u32,
        match_max_len: u32,
        keep_add_buffer_after: u32,
    ) -> Result<(), SteamError> {
        if history_size > MAX_VAL_FOR_NORMALIZE - 256 {
            return Err(SteamError::InvalidArgument(
                "LZMA match finder history size is too large".to_string(),
            ));
        }
        self.cut_value = 16 + (match_max_len >> 1);

        let window_reserv_size = (history_size
            .wrapping_add(keep_add_buffer_before)
            .wrapping_add(match_max_len)
            .wrapping_add(keep_add_buffer_after))
            / 2
            + 256;

        self.window.create(
            history_size.wrapping_add(keep_add_buffer_before),
            match_max_len.wrapping_add(keep_add_buffer_after),
            window_reserv_size,
        );

        self.match_max_len = match_max_len;

        let cyclic_buffer_size = history_size.wrapping_add(1);
        if self.cyclic_buffer_size != cyclic_buffer_size {
            self.cyclic_buffer_size = cyclic_buffer_size;
            self.son = vec![0u32; (self.cyclic_buffer_size as usize) * 2];
        }

        let mut hs = BT2_HASH_SIZE;

        if self.hash_array {
            let mut h = history_size.wrapping_sub(1);
            h |= h >> 1;
            h |= h >> 2;
            h |= h >> 4;
            h |= h >> 8;
            h >>= 1;
            h |= 0xFFFF;
            if h > (1 << 24) {
                h >>= 1;
            }
            self.hash_mask = h;
            hs = h.wrapping_add(1);
            hs = hs.wrapping_add(self.fix_hash_size);
        }
        if hs != self.hash_size_sum {
            self.hash_size_sum = hs;
            self.hash = vec![0u32; hs as usize];
        }

        Ok(())
    }

    /// Finds the matches at the current position.
    ///
    /// The matches are written into `distances` as (length, distance - 1)
    /// pairs; the number of written values is returned.
    pub fn get_matches(&mut self, distances: &mut [u32]) -> u32 {
        let len_limit;
        if self
            .window
            .pos
            .wrapping_add(self.match_max_len)
            <= self.window.stream_pos
        {
            len_limit = self.match_max_len;
        } else {
            len_limit = self.window.stream_pos.wrapping_sub(self.window.pos);
            if len_limit < self.min_match_check {
                self.move_pos();
                return 0;
            }
        }

        let mut offset = 0u32;
        let match_min_pos = if self.window.pos > self.cyclic_buffer_size {
            self.window.pos - self.cyclic_buffer_size
        } else {
            0
        };
        let cur = self.window.buffer_offset.wrapping_add(self.window.pos);
        let mut max_len = START_MAX_LEN;
        let hash_value;
        let mut hash2_value = 0u32;
        let mut hash3_value = 0u32;

        if self.hash_array {
            let mut temp = crc::TABLE[self.byte_at(cur) as usize] ^ self.byte_at(cur.wrapping_add(1)) as u32;
            hash2_value = temp & (HASH2_SIZE - 1);
            temp ^= (self.byte_at(cur.wrapping_add(2)) as u32) << 8;
            hash3_value = temp & (HASH3_SIZE - 1);
            hash_value = (temp ^ (crc::TABLE[self.byte_at(cur.wrapping_add(3)) as usize] << 5))
                & self.hash_mask;
        } else {
            hash_value = self.byte_at(cur) as u32 ^ ((self.byte_at(cur.wrapping_add(1)) as u32) << 8);
        }

        let mut cur_match = self.hash_get(self.fix_hash_size.wrapping_add(hash_value));
        if self.hash_array {
            let mut cur_match2 = self.hash_get(hash2_value);
            let cur_match3 = self.hash_get(HASH3_OFFSET.wrapping_add(hash3_value));
            self.hash_set(hash2_value, self.window.pos);
            self.hash_set(HASH3_OFFSET.wrapping_add(hash3_value), self.window.pos);
            if cur_match2 > match_min_pos
                && self.byte_at(self.window.buffer_offset.wrapping_add(cur_match2))
                    == self.byte_at(cur)
            {
                put(distances, offset, 2);
                offset = offset.wrapping_add(1);
                max_len = 2;
                put(
                    distances,
                    offset,
                    self.window.pos.wrapping_sub(cur_match2).wrapping_sub(1),
                );
                offset = offset.wrapping_add(1);
            }
            if cur_match3 > match_min_pos
                && self.byte_at(self.window.buffer_offset.wrapping_add(cur_match3))
                    == self.byte_at(cur)
            {
                if cur_match3 == cur_match2 {
                    offset = offset.wrapping_sub(2);
                }
                put(distances, offset, 3);
                offset = offset.wrapping_add(1);
                max_len = 3;
                put(
                    distances,
                    offset,
                    self.window.pos.wrapping_sub(cur_match3).wrapping_sub(1),
                );
                offset = offset.wrapping_add(1);
                cur_match2 = cur_match3;
            }
            if offset != 0 && cur_match2 == cur_match {
                offset = offset.wrapping_sub(2);
                max_len = START_MAX_LEN;
            }
        }

        self.hash_set(self.fix_hash_size.wrapping_add(hash_value), self.window.pos);

        let mut ptr0 = (self.cyclic_buffer_pos << 1) + 1;
        let mut ptr1 = self.cyclic_buffer_pos << 1;

        let mut len0 = self.num_hash_direct_bytes;
        let mut len1 = self.num_hash_direct_bytes;

        if self.num_hash_direct_bytes != 0 && cur_match > match_min_pos {
            if self.byte_at(
                self.window
                    .buffer_offset
                    .wrapping_add(cur_match)
                    .wrapping_add(self.num_hash_direct_bytes),
            ) != self.byte_at(cur.wrapping_add(self.num_hash_direct_bytes))
            {
                put(distances, offset, self.num_hash_direct_bytes);
                offset = offset.wrapping_add(1);
                max_len = self.num_hash_direct_bytes;
                put(
                    distances,
                    offset,
                    self.window.pos.wrapping_sub(cur_match).wrapping_sub(1),
                );
                offset = offset.wrapping_add(1);
            }
        }

        let mut count = self.cut_value;

        loop {
            if cur_match <= match_min_pos {
                self.son_set(ptr1, EMPTY_HASH_VALUE);
                self.son_set(ptr0, EMPTY_HASH_VALUE);
                break;
            }
            let count_is_zero = count == 0;
            count = count.wrapping_sub(1);
            if count_is_zero {
                self.son_set(ptr1, EMPTY_HASH_VALUE);
                self.son_set(ptr0, EMPTY_HASH_VALUE);
                break;
            }

            let delta = self.window.pos.wrapping_sub(cur_match);
            let cyclic_pos = if delta <= self.cyclic_buffer_pos {
                (self.cyclic_buffer_pos - delta) << 1
            } else {
                (self
                    .cyclic_buffer_pos
                    .wrapping_sub(delta)
                    .wrapping_add(self.cyclic_buffer_size))
                    << 1
            };

            let pby1 = self.window.buffer_offset.wrapping_add(cur_match);
            let mut len = len0.min(len1);
            if self.byte_at(pby1.wrapping_add(len)) == self.byte_at(cur.wrapping_add(len)) {
                loop {
                    len = len.wrapping_add(1);
                    if len == len_limit {
                        break;
                    }
                    if self.byte_at(pby1.wrapping_add(len)) != self.byte_at(cur.wrapping_add(len)) {
                        break;
                    }
                }
                if max_len < len {
                    put(distances, offset, len);
                    offset = offset.wrapping_add(1);
                    max_len = len;
                    put(distances, offset, delta.wrapping_sub(1));
                    offset = offset.wrapping_add(1);
                    if len == len_limit {
                        self.son_set(ptr1, self.son_get(cyclic_pos));
                        self.son_set(ptr0, self.son_get(cyclic_pos.wrapping_add(1)));
                        break;
                    }
                }
            }
            if self.byte_at(pby1.wrapping_add(len)) < self.byte_at(cur.wrapping_add(len)) {
                self.son_set(ptr1, cur_match);
                ptr1 = cyclic_pos.wrapping_add(1);
                cur_match = self.son_get(ptr1);
                len1 = len;
            } else {
                self.son_set(ptr0, cur_match);
                ptr0 = cyclic_pos;
                cur_match = self.son_get(ptr0);
                len0 = len;
            }
        }

        self.move_pos();
        offset
    }

    /// Skips `num` positions.
    pub fn skip(&mut self, num: u32) {
        let mut remaining = num;
        while remaining != 0 {
            remaining = remaining.wrapping_sub(1);

            let len_limit;
            if self
                .window
                .pos
                .wrapping_add(self.match_max_len)
                <= self.window.stream_pos
            {
                len_limit = self.match_max_len;
            } else {
                len_limit = self.window.stream_pos.wrapping_sub(self.window.pos);
                if len_limit < self.min_match_check {
                    self.move_pos();
                    continue;
                }
            }

            let match_min_pos = if self.window.pos > self.cyclic_buffer_size {
                self.window.pos - self.cyclic_buffer_size
            } else {
                0
            };
            let cur = self.window.buffer_offset.wrapping_add(self.window.pos);

            let hash_value;

            if self.hash_array {
                let mut temp =
                    crc::TABLE[self.byte_at(cur) as usize] ^ self.byte_at(cur.wrapping_add(1)) as u32;
                let hash2_value = temp & (HASH2_SIZE - 1);
                self.hash_set(hash2_value, self.window.pos);
                temp ^= (self.byte_at(cur.wrapping_add(2)) as u32) << 8;
                let hash3_value = temp & (HASH3_SIZE - 1);
                self.hash_set(HASH3_OFFSET.wrapping_add(hash3_value), self.window.pos);
                hash_value = (temp ^ (crc::TABLE[self.byte_at(cur.wrapping_add(3)) as usize] << 5))
                    & self.hash_mask;
            } else {
                hash_value =
                    self.byte_at(cur) as u32 ^ ((self.byte_at(cur.wrapping_add(1)) as u32) << 8);
            }

            let mut cur_match = self.hash_get(self.fix_hash_size.wrapping_add(hash_value));
            self.hash_set(self.fix_hash_size.wrapping_add(hash_value), self.window.pos);

            let mut ptr0 = (self.cyclic_buffer_pos << 1) + 1;
            let mut ptr1 = self.cyclic_buffer_pos << 1;

            let mut len0 = self.num_hash_direct_bytes;
            let mut len1 = self.num_hash_direct_bytes;

            let mut count = self.cut_value;
            loop {
                if cur_match <= match_min_pos {
                    self.son_set(ptr1, EMPTY_HASH_VALUE);
                    self.son_set(ptr0, EMPTY_HASH_VALUE);
                    break;
                }
                let count_is_zero = count == 0;
                count = count.wrapping_sub(1);
                if count_is_zero {
                    self.son_set(ptr1, EMPTY_HASH_VALUE);
                    self.son_set(ptr0, EMPTY_HASH_VALUE);
                    break;
                }

                let delta = self.window.pos.wrapping_sub(cur_match);
                let cyclic_pos = if delta <= self.cyclic_buffer_pos {
                    (self.cyclic_buffer_pos - delta) << 1
                } else {
                    (self
                        .cyclic_buffer_pos
                        .wrapping_sub(delta)
                        .wrapping_add(self.cyclic_buffer_size))
                        << 1
                };

                let pby1 = self.window.buffer_offset.wrapping_add(cur_match);
                let mut len = len0.min(len1);
                if self.byte_at(pby1.wrapping_add(len)) == self.byte_at(cur.wrapping_add(len)) {
                    loop {
                        len = len.wrapping_add(1);
                        if len == len_limit {
                            break;
                        }
                        if self.byte_at(pby1.wrapping_add(len)) != self.byte_at(cur.wrapping_add(len))
                        {
                            break;
                        }
                    }
                    if len == len_limit {
                        self.son_set(ptr1, self.son_get(cyclic_pos));
                        self.son_set(ptr0, self.son_get(cyclic_pos.wrapping_add(1)));
                        break;
                    }
                }
                if self.byte_at(pby1.wrapping_add(len)) < self.byte_at(cur.wrapping_add(len)) {
                    self.son_set(ptr1, cur_match);
                    ptr1 = cyclic_pos.wrapping_add(1);
                    cur_match = self.son_get(ptr1);
                    len1 = len;
                } else {
                    self.son_set(ptr0, cur_match);
                    ptr0 = cyclic_pos;
                    cur_match = self.son_get(ptr0);
                    len0 = len;
                }
            }

            self.move_pos();
        }
    }

    /// Sets the number of tree nodes that are visited per position.
    pub fn set_cut_value(&mut self, cut_value: u32) {
        self.cut_value = cut_value;
    }

    fn normalize_links(items: &mut [u32], num_items: u32, sub_value: u32) {
        for item in items.iter_mut().take(num_items as usize) {
            let value = *item;
            *item = if value <= sub_value {
                EMPTY_HASH_VALUE
            } else {
                value - sub_value
            };
        }
    }

    fn normalize(&mut self) {
        let sub_value = self.window.pos.wrapping_sub(self.cyclic_buffer_size);
        Self::normalize_links(&mut self.son, self.cyclic_buffer_size.wrapping_mul(2), sub_value);
        Self::normalize_links(&mut self.hash, self.hash_size_sum, sub_value);
        self.window.reduce_offsets(sub_value as i32);
    }
}

/// Writes a value into the match list, ignoring out-of-range offsets.
#[inline]
fn put(distances: &mut [u32], offset: u32, value: u32) {
    if let Some(slot) = distances.get_mut(offset as usize) {
        *slot = value;
    }
}

impl IInWindowStream for BinTree {
    fn set_stream(&mut self, stream: Vec<u8>) {
        self.window.set_stream(stream);
    }

    fn init(&mut self) {
        BinTree::init(self);
    }

    fn release_stream(&mut self) {
        self.window.release_stream();
    }

    fn get_index_byte(&self, index: i32) -> u8 {
        self.window.get_index_byte(index)
    }

    fn get_match_len(&self, index: i32, distance: u32, limit: u32) -> u32 {
        self.window.get_match_len(index, distance, limit)
    }

    fn get_num_available_bytes(&self) -> u32 {
        self.window.get_num_available_bytes()
    }
}

impl IMatchFinder for BinTree {
    fn create(
        &mut self,
        history_size: u32,
        keep_add_buffer_before: u32,
        match_max_len: u32,
        keep_add_buffer_after: u32,
    ) -> Result<(), SteamError> {
        BinTree::create(
            self,
            history_size,
            keep_add_buffer_before,
            match_max_len,
            keep_add_buffer_after,
        )
    }

    fn get_matches(&mut self, distances: &mut [u32]) -> u32 {
        BinTree::get_matches(self, distances)
    }

    fn skip(&mut self, num: u32) {
        BinTree::skip(self, num);
    }
}
