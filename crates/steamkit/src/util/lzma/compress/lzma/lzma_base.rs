//! Shared LZMA constants and the encoder state machine (`LzmaBase`).

/// The number of repeated distances that are kept.
pub const NUM_REP_DISTANCES: u32 = 4;

/// The number of states of the LZMA state machine.
pub const NUM_STATES: u32 = 12;

/// The number of bits of a position slot.
pub const NUM_POS_SLOT_BITS: i32 = 6;

/// The minimum dictionary log size.
pub const DIC_LOG_SIZE_MIN: i32 = 0;

/// The number of bits of the "length to position state" index.
pub const NUM_LEN_TO_POS_STATES_BITS: i32 = 2;

/// The number of "length to position state" indexes.
pub const NUM_LEN_TO_POS_STATES: u32 = 1 << NUM_LEN_TO_POS_STATES_BITS;

/// The minimum match length.
pub const MATCH_MIN_LEN: u32 = 2;

/// The number of bits of the alignment tree.
pub const NUM_ALIGN_BITS: i32 = 4;

/// The size of the alignment tree.
pub const ALIGN_TABLE_SIZE: u32 = 1 << NUM_ALIGN_BITS;

/// The mask of the alignment tree.
pub const ALIGN_MASK: u32 = ALIGN_TABLE_SIZE - 1;

/// The first index of the special position models.
pub const START_POS_MODEL_INDEX: u32 = 4;

/// The end index of the special position models.
pub const END_POS_MODEL_INDEX: u32 = 14;

/// The number of special position models.
pub const NUM_POS_MODELS: u32 = END_POS_MODEL_INDEX - START_POS_MODEL_INDEX;

/// The number of distances that have their own price table.
pub const NUM_FULL_DISTANCES: u32 = 1 << (END_POS_MODEL_INDEX / 2);

/// The maximum number of literal position state bits.
pub const NUM_LIT_POS_STATES_BITS_ENCODING_MAX: u32 = 4;

/// The maximum number of literal context bits.
pub const NUM_LIT_CONTEXT_BITS_MAX: u32 = 8;

/// The maximum number of position state bits.
pub const NUM_POS_STATES_BITS_MAX: i32 = 4;

/// The maximum number of position states.
pub const NUM_POS_STATES_MAX: u32 = 1 << NUM_POS_STATES_BITS_MAX;

/// The maximum number of position state bits that can be encoded.
pub const NUM_POS_STATES_BITS_ENCODING_MAX: i32 = 4;

/// The maximum number of position states that can be encoded.
pub const NUM_POS_STATES_ENCODING_MAX: u32 = 1 << NUM_POS_STATES_BITS_ENCODING_MAX;

/// The number of bits of the low length tree.
pub const NUM_LOW_LEN_BITS: i32 = 3;

/// The number of bits of the mid length tree.
pub const NUM_MID_LEN_BITS: i32 = 3;

/// The number of bits of the high length tree.
pub const NUM_HIGH_LEN_BITS: i32 = 8;

/// The number of symbols of the low length tree.
pub const NUM_LOW_LEN_SYMBOLS: u32 = 1 << NUM_LOW_LEN_BITS;

/// The number of symbols of the mid length tree.
pub const NUM_MID_LEN_SYMBOLS: u32 = 1 << NUM_MID_LEN_BITS;

/// The total number of length symbols.
pub const NUM_LEN_SYMBOLS: u32 = NUM_LOW_LEN_SYMBOLS + NUM_MID_LEN_SYMBOLS + (1 << NUM_HIGH_LEN_BITS);

/// The maximum match length.
pub const MATCH_MAX_LEN: u32 = MATCH_MIN_LEN + NUM_LEN_SYMBOLS - 1;

/// The state of the LZMA state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    /// The index of the state.
    pub index: u32,
}

impl State {
    /// Creates a state that has not been initialised yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the state.
    pub fn init(&mut self) {
        self.index = 0;
    }

    /// Applies the transition of a literal.
    pub fn update_char(&mut self) {
        if self.index < 4 {
            self.index = 0;
        } else if self.index < 10 {
            self.index -= 3;
        } else {
            self.index -= 6;
        }
    }

    /// Applies the transition of a match.
    pub fn update_match(&mut self) {
        self.index = if self.index < 7 { 7 } else { 10 };
    }

    /// Applies the transition of a repeated match.
    pub fn update_rep(&mut self) {
        self.index = if self.index < 7 { 8 } else { 11 };
    }

    /// Applies the transition of a short repeated match.
    pub fn update_short_rep(&mut self) {
        self.index = if self.index < 7 { 9 } else { 11 };
    }

    /// Whether the state is a literal state.
    pub fn is_char_state(&self) -> bool {
        self.index < 7
    }
}

/// The "length to position state" index of a match length.
pub fn get_len_to_pos_state(len: u32) -> u32 {
    let len = len.wrapping_sub(MATCH_MIN_LEN);
    if len < NUM_LEN_TO_POS_STATES {
        len
    } else {
        NUM_LEN_TO_POS_STATES - 1
    }
}
