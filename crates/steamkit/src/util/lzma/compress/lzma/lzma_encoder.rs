//! The LZMA encoder from the 7-Zip LZMA SDK (`LzmaEncoder`).
//!
//! This is the full optimal-parsing encoder: probability models, price
//! tables, the literal/length/position encoders and the optimal parser. It
//! produces the same output as the reference SDK encoder.

use crate::error::SteamError;

use super::lzma_base::{
    get_len_to_pos_state, State, ALIGN_MASK, ALIGN_TABLE_SIZE, END_POS_MODEL_INDEX,
    MATCH_MAX_LEN, MATCH_MIN_LEN, NUM_ALIGN_BITS, NUM_FULL_DISTANCES, NUM_HIGH_LEN_BITS,
    NUM_LEN_SYMBOLS, NUM_LEN_TO_POS_STATES, NUM_LOW_LEN_BITS, NUM_LOW_LEN_SYMBOLS,
    NUM_MID_LEN_BITS, NUM_MID_LEN_SYMBOLS, NUM_POS_SLOT_BITS, NUM_POS_STATES_BITS_ENCODING_MAX,
    NUM_POS_STATES_BITS_MAX, NUM_POS_STATES_ENCODING_MAX, NUM_REP_DISTANCES, NUM_STATES,
    START_POS_MODEL_INDEX,
};
use super::super::range_coder::range_coder::Encoder as RangeEncoder;
use super::super::range_coder::range_coder_bit::BitEncoder;
use super::super::range_coder::range_coder_bit_tree::BitTreeEncoder;

use crate::util::lzma::compress::lz::i_match_finder::IMatchFinder;
use crate::util::lzma::compress::lz::lz_bin_tree::BinTree;

use crate::util::lzma::i_coder::{
    CoderPropID, CoderProperty, ICodeProgress, ICoder, InvalidParamException,
    ISetCoderProperties, IWriteCoderProperties,
};

/// The price of an operation that must not be chosen.
const K_IFINITY_PRICE: u32 = 0x0FFF_FFFF;

/// The number of positions the optimal parser looks ahead.
const K_NUM_OPTS: u32 = 1 << 12;

const K_DEFAULT_DICTIONARY_LOG_SIZE: u32 = 22;
const K_NUM_FAST_BYTES_DEFAULT: u32 = 0x20;

/// The match finder that is used to find the matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EMatchFinderType {
    /// The "BT2" match finder: a two byte hash.
    BT2,
    /// The "BT4" match finder: a four byte hash.
    BT4,
}

impl Default for EMatchFinderType {
    fn default() -> Self {
        EMatchFinderType::BT4
    }
}

/// The lookup table that maps a distance to its position slot.
const FAST_POS: [u8; 1 << 11] = build_fast_pos();

const fn build_fast_pos() -> [u8; 1 << 11] {
    let mut table = [0u8; 1 << 11];
    table[0] = 0;
    table[1] = 1;
    let mut c = 2usize;
    let mut slot_fast = 2u32;
    while slot_fast < 22 {
        let k = 1u32 << ((slot_fast >> 1) - 1);
        let mut j = 0u32;
        while j < k {
            table[c] = slot_fast as u8;
            c += 1;
            j += 1;
        }
        slot_fast += 1;
    }
    table
}

/// The position slot of a distance.
pub fn get_pos_slot(pos: u32) -> u32 {
    if pos < (1 << 11) {
        FAST_POS[pos as usize] as u32
    } else if pos < (1 << 21) {
        FAST_POS[(pos >> 10) as usize] as u32 + 20
    } else {
        FAST_POS[(pos >> 20) as usize] as u32 + 40
    }
}

/// The position slot of a distance, using the encoder's price table.
pub fn get_pos_slot2(pos: u32) -> u32 {
    if pos < (1 << 17) {
        FAST_POS[(pos >> 6) as usize] as u32 + 12
    } else if pos < (1 << 27) {
        FAST_POS[(pos >> 16) as usize] as u32 + 32
    } else {
        FAST_POS[(pos >> 26) as usize] as u32 + 52
    }
}

/// The literal coder of a single (position state, previous byte) context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralEncoder2 {
    m_encoders: Vec<BitEncoder>,
}

impl Default for LiteralEncoder2 {
    fn default() -> Self {
        Self {
            m_encoders: vec![BitEncoder::new(); 0x300],
        }
    }
}

impl LiteralEncoder2 {
    /// Creates the literal coder.
    pub fn create(&mut self) {
        self.m_encoders = vec![BitEncoder::new(); 0x300];
    }

    /// Resets every probability of the coder.
    pub fn init(&mut self) {
        for encoder in self.m_encoders.iter_mut() {
            encoder.init();
        }
    }

    /// Encodes a literal.
    pub fn encode(&mut self, range_encoder: &mut RangeEncoder, symbol: u8) {
        let mut context = 1u32;
        for i in (0..=7).rev() {
            let bit = ((symbol as u32) >> i) & 1;
            self.m_encoders[context as usize].encode(range_encoder, bit);
            context = (context << 1) | bit;
        }
    }

    /// Encodes a literal that follows a match.
    pub fn encode_matched(
        &mut self,
        range_encoder: &mut RangeEncoder,
        match_byte: u8,
        symbol: u8,
    ) {
        let mut context = 1u32;
        let mut same = true;
        for i in (0..=7).rev() {
            let bit = ((symbol as u32) >> i) & 1;
            let mut state = context;
            if same {
                let match_bit = ((match_byte as u32) >> i) & 1;
                state += (1 + match_bit) << 8;
                same = match_bit == bit;
            }
            self.m_encoders[state as usize].encode(range_encoder, bit);
            context = (context << 1) | bit;
        }
    }

    /// The price of encoding a literal.
    pub fn get_price(&self, match_mode: bool, match_byte: u8, symbol: u8) -> u32 {
        let mut price = 0u32;
        let mut context = 1u32;
        let mut i: i32 = 7;
        if match_mode {
            while i >= 0 {
                let match_bit = ((match_byte as u32) >> i) & 1;
                let bit = ((symbol as u32) >> i) & 1;
                price += self.m_encoders[(((1 + match_bit) << 8) + context) as usize].get_price(bit);
                context = (context << 1) | bit;
                if match_bit != bit {
                    i -= 1;
                    break;
                }
                i -= 1;
            }
        }
        while i >= 0 {
            let bit = ((symbol as u32) >> i) & 1;
            price += self.m_encoders[context as usize].get_price(bit);
            context = (context << 1) | bit;
            i -= 1;
        }
        price
    }
}

/// The set of literal coders, one per (position state, previous byte) pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralEncoder {
    coders: Vec<LiteralEncoder2>,
    num_prev_bits: i32,
    num_pos_bits: i32,
    pos_mask: u32,
}

impl Default for LiteralEncoder {
    fn default() -> Self {
        Self {
            coders: Vec::new(),
            num_prev_bits: 0,
            num_pos_bits: 0,
            pos_mask: 0,
        }
    }
}

impl LiteralEncoder {
    /// Creates the literal encoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// (Re)creates the coders for the given number of position and previous
    /// byte bits.
    pub fn create(&mut self, num_pos_bits: i32, num_prev_bits: i32) {
        if !self.coders.is_empty()
            && self.num_prev_bits == num_prev_bits
            && self.num_pos_bits == num_pos_bits
        {
            return;
        }
        self.num_pos_bits = num_pos_bits;
        self.pos_mask = (1u32 << num_pos_bits) - 1;
        self.num_prev_bits = num_prev_bits;
        let num_states = 1u32 << (self.num_prev_bits + self.num_pos_bits);
        self.coders = (0..num_states)
            .map(|_| {
                let mut coder = LiteralEncoder2::default();
                coder.create();
                coder
            })
            .collect();
    }

    /// Resets every probability of every coder.
    pub fn init(&mut self) {
        for coder in self.coders.iter_mut() {
            coder.init();
        }
    }

    /// The index of the coder that handles the given position and previous byte.
    pub fn get_sub_coder_index(&self, pos: u32, prev_byte: u8) -> usize {
        (((pos & self.pos_mask) << self.num_prev_bits)
            + ((prev_byte as u32) >> (8 - self.num_prev_bits))) as usize
    }

    /// The coder that handles the given position and previous byte.
    pub fn coder(&self, index: usize) -> &LiteralEncoder2 {
        &self.coders[index]
    }

    /// The coder that handles the given position and previous byte.
    pub fn coder_mut(&mut self, index: usize) -> &mut LiteralEncoder2 {
        &mut self.coders[index]
    }
}

/// The length coder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LenEncoder {
    choice: BitEncoder,
    choice2: BitEncoder,
    low_coder: Vec<BitTreeEncoder>,
    mid_coder: Vec<BitTreeEncoder>,
    high_coder: BitTreeEncoder,
}

impl Default for LenEncoder {
    fn default() -> Self {
        let mut low_coder = Vec::with_capacity(NUM_POS_STATES_ENCODING_MAX as usize);
        let mut mid_coder = Vec::with_capacity(NUM_POS_STATES_ENCODING_MAX as usize);
        for _ in 0..NUM_POS_STATES_ENCODING_MAX {
            low_coder.push(BitTreeEncoder::new(NUM_LOW_LEN_BITS as u32));
            mid_coder.push(BitTreeEncoder::new(NUM_MID_LEN_BITS as u32));
        }
        Self {
            choice: BitEncoder::new(),
            choice2: BitEncoder::new(),
            low_coder,
            mid_coder,
            high_coder: BitTreeEncoder::new(NUM_HIGH_LEN_BITS as u32),
        }
    }
}

impl LenEncoder {
    /// Creates the length encoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the probabilities of the encoder.
    pub fn init(&mut self, num_pos_states: u32) {
        self.choice.init();
        self.choice2.init();
        for pos_state in 0..num_pos_states as usize {
            if let Some(coder) = self.low_coder.get_mut(pos_state) {
                coder.init();
            }
            if let Some(coder) = self.mid_coder.get_mut(pos_state) {
                coder.init();
            }
        }
        self.high_coder.init();
    }

    /// Encodes a length symbol.
    pub fn encode(&mut self, range_encoder: &mut RangeEncoder, symbol: u32, pos_state: u32) {
        if symbol < NUM_LOW_LEN_SYMBOLS {
            self.choice.encode(range_encoder, 0);
            if let Some(coder) = self.low_coder.get_mut(pos_state as usize) {
                coder.encode(range_encoder, symbol);
            }
        } else {
            let symbol = symbol - NUM_LOW_LEN_SYMBOLS;
            self.choice.encode(range_encoder, 1);
            if symbol < NUM_MID_LEN_SYMBOLS {
                self.choice2.encode(range_encoder, 0);
                if let Some(coder) = self.mid_coder.get_mut(pos_state as usize) {
                    coder.encode(range_encoder, symbol);
                }
            } else {
                self.choice2.encode(range_encoder, 1);
                self.high_coder
                    .encode(range_encoder, symbol - NUM_MID_LEN_SYMBOLS);
            }
        }
    }

    /// Fills the price table of a position state.
    pub fn set_prices(
        &self,
        pos_state: u32,
        num_symbols: u32,
        prices: &mut [u32],
        st: u32,
    ) {
        let a0 = self.choice.get_price0();
        let a1 = self.choice.get_price1();
        let b0 = a1 + self.choice2.get_price0();
        let b1 = a1 + self.choice2.get_price1();

        let mut i = 0u32;
        while i < NUM_LOW_LEN_SYMBOLS {
            if i >= num_symbols {
                return;
            }
            let price = match self.low_coder.get(pos_state as usize) {
                Some(coder) => a0 + coder.get_price(i),
                None => a0,
            };
            if let Some(slot) = prices.get_mut((st + i) as usize) {
                *slot = price;
            }
            i += 1;
        }
        while i < NUM_LOW_LEN_SYMBOLS + NUM_MID_LEN_SYMBOLS {
            if i >= num_symbols {
                return;
            }
            let price = match self.mid_coder.get(pos_state as usize) {
                Some(coder) => b0 + coder.get_price(i - NUM_LOW_LEN_SYMBOLS),
                None => b0,
            };
            if let Some(slot) = prices.get_mut((st + i) as usize) {
                *slot = price;
            }
            i += 1;
        }
        while i < num_symbols {
            let price =
                b1 + self.high_coder.get_price(i - NUM_LOW_LEN_SYMBOLS - NUM_MID_LEN_SYMBOLS);
            if let Some(slot) = prices.get_mut((st + i) as usize) {
                *slot = price;
            }
            i += 1;
        }
    }
}

/// The length coder that keeps a price table of the possible lengths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LenPriceTableEncoder {
    inner: LenEncoder,
    prices: Vec<u32>,
    table_size: u32,
    counters: Vec<u32>,
}

impl Default for LenPriceTableEncoder {
    fn default() -> Self {
        Self {
            inner: LenEncoder::new(),
            prices: vec![0u32; (NUM_LEN_SYMBOLS << NUM_POS_STATES_BITS_ENCODING_MAX) as usize],
            table_size: 0,
            counters: vec![0u32; NUM_POS_STATES_ENCODING_MAX as usize],
        }
    }
}

impl LenPriceTableEncoder {
    /// Creates the length encoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the number of lengths the price table covers.
    pub fn set_table_size(&mut self, table_size: u32) {
        self.table_size = table_size;
    }

    /// Resets the probabilities of the encoder.
    pub fn init(&mut self, num_pos_states: u32) {
        self.inner.init(num_pos_states);
    }

    /// The price of a length symbol.
    pub fn get_price(&self, symbol: u32, pos_state: u32) -> u32 {
        self.prices
            .get((pos_state * NUM_LEN_SYMBOLS + symbol) as usize)
            .copied()
            .unwrap_or(0)
    }

    fn update_table(&mut self, pos_state: u32) {
        let st = pos_state * NUM_LEN_SYMBOLS;
        let table_size = self.table_size;
        self.inner
            .set_prices(pos_state, table_size, &mut self.prices, st);
        if let Some(counter) = self.counters.get_mut(pos_state as usize) {
            *counter = table_size;
        }
    }

    /// Rebuilds the price tables of the given position states.
    pub fn update_tables(&mut self, num_pos_states: u32) {
        for pos_state in 0..num_pos_states {
            self.update_table(pos_state);
        }
    }

    /// Encodes a length symbol and refreshes the price table when needed.
    pub fn encode(&mut self, range_encoder: &mut RangeEncoder, symbol: u32, pos_state: u32) {
        self.inner.encode(range_encoder, symbol, pos_state);

        let mut needs_update = false;
        if let Some(counter) = self.counters.get_mut(pos_state as usize) {
            *counter = counter.wrapping_sub(1);
            needs_update = *counter == 0;
        }
        if needs_update {
            self.update_table(pos_state);
        }
    }
}

/// One position of the optimal parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Optimal {
    /// The state of the parser at this position.
    pub state: State,
    /// Whether the previous operation was a literal.
    pub prev1_is_char: bool,
    /// Whether the operation before that was a literal as well.
    pub prev2: bool,
    /// The position of the operation before the previous one.
    pub pos_prev2: u32,
    /// The distance of the operation before the previous one.
    pub back_prev2: u32,
    /// The price of the best sequence that ends here.
    pub price: u32,
    /// The position the best sequence comes from.
    pub pos_prev: u32,
    /// The distance of the best sequence.
    pub back_prev: u32,
    /// The first repeated distance at this position.
    pub backs0: u32,
    /// The second repeated distance at this position.
    pub backs1: u32,
    /// The third repeated distance at this position.
    pub backs2: u32,
    /// The fourth repeated distance at this position.
    pub backs3: u32,
}

impl Optimal {
    /// Marks this position as being reached by a literal.
    pub fn make_as_char(&mut self) {
        self.back_prev = 0xFFFF_FFFF;
        self.prev1_is_char = false;
    }

    /// Marks this position as being reached by a short repeated match.
    pub fn make_as_short_rep(&mut self) {
        self.back_prev = 0;
        self.prev1_is_char = false;
    }

    /// Whether this position is reached by a short repeated match.
    pub fn is_short_rep(&self) -> bool {
        self.back_prev == 0
    }
}

/// The LZMA encoder.
#[derive(Debug)]
pub struct Encoder {
    state: State,
    previous_byte: u8,
    rep_distances: [u32; NUM_REP_DISTANCES as usize],

    optimum: Vec<Optimal>,
    match_finder: Box<dyn IMatchFinder>,
    range_encoder: RangeEncoder,

    is_match: Vec<BitEncoder>,
    is_rep: Vec<BitEncoder>,
    is_rep_g0: Vec<BitEncoder>,
    is_rep_g1: Vec<BitEncoder>,
    is_rep_g2: Vec<BitEncoder>,
    is_rep0_long: Vec<BitEncoder>,

    pos_slot_encoder: Vec<BitTreeEncoder>,
    pos_encoders: Vec<BitEncoder>,
    pos_align_encoder: BitTreeEncoder,

    len_encoder: LenPriceTableEncoder,
    rep_match_len_encoder: LenPriceTableEncoder,

    literal_encoder: LiteralEncoder,

    match_distances: Vec<u32>,

    num_fast_bytes: u32,
    longest_match_length: u32,
    num_distance_pairs: u32,

    additional_offset: u32,

    optimum_end_index: u32,
    optimum_current_index: u32,

    longest_match_was_found: bool,

    pos_slot_prices: Vec<u32>,
    distances_prices: Vec<u32>,
    align_prices: Vec<u32>,
    align_price_count: u32,

    dist_table_size: u32,

    pos_state_bits: i32,
    pos_state_mask: u32,
    num_literal_pos_state_bits: i32,
    num_literal_context_bits: i32,

    dictionary_size: u32,
    dictionary_size_prev: u32,
    num_fast_bytes_prev: u32,

    now_pos64: i64,
    finished: bool,
    in_stream: Option<Vec<u8>>,

    match_finder_type: EMatchFinderType,
    write_end_mark: bool,

    need_release_mf_stream: bool,

    reps: [u32; NUM_REP_DISTANCES as usize],
    rep_lens: [u32; NUM_REP_DISTANCES as usize],

    match_price_count: u32,
    train_size: u32,

    properties: [u8; 5],
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder {
    /// Creates an encoder with the default properties.
    pub fn new() -> Self {
        let mut optimum = Vec::with_capacity(K_NUM_OPTS as usize);
        for _ in 0..K_NUM_OPTS {
            optimum.push(Optimal::default());
        }

        let mut pos_slot_encoder = Vec::with_capacity(NUM_LEN_TO_POS_STATES as usize);
        for _ in 0..NUM_LEN_TO_POS_STATES {
            pos_slot_encoder.push(BitTreeEncoder::new(NUM_POS_SLOT_BITS as u32));
        }

        let mut match_finder = BinTree::new();
        match_finder.set_type(4);

        Self {
            state: State::new(),
            previous_byte: 0,
            rep_distances: [0; NUM_REP_DISTANCES as usize],

            optimum,
            match_finder: Box::new(match_finder),
            range_encoder: RangeEncoder::new(),

            is_match: vec![BitEncoder::new(); (NUM_STATES << NUM_POS_STATES_BITS_MAX) as usize],
            is_rep: vec![BitEncoder::new(); NUM_STATES as usize],
            is_rep_g0: vec![BitEncoder::new(); NUM_STATES as usize],
            is_rep_g1: vec![BitEncoder::new(); NUM_STATES as usize],
            is_rep_g2: vec![BitEncoder::new(); NUM_STATES as usize],
            is_rep0_long: vec![BitEncoder::new(); (NUM_STATES << NUM_POS_STATES_BITS_MAX) as usize],

            pos_slot_encoder,
            pos_encoders: vec![
                BitEncoder::new();
                (NUM_FULL_DISTANCES - END_POS_MODEL_INDEX) as usize
            ],
            pos_align_encoder: BitTreeEncoder::new(NUM_ALIGN_BITS as u32),

            len_encoder: LenPriceTableEncoder::new(),
            rep_match_len_encoder: LenPriceTableEncoder::new(),

            literal_encoder: LiteralEncoder::new(),

            match_distances: vec![0u32; (MATCH_MAX_LEN * 2 + 2) as usize],

            num_fast_bytes: K_NUM_FAST_BYTES_DEFAULT,
            longest_match_length: 0,
            num_distance_pairs: 0,

            additional_offset: 0,

            optimum_end_index: 0,
            optimum_current_index: 0,

            longest_match_was_found: false,

            pos_slot_prices: vec![0u32; 1 << (NUM_POS_SLOT_BITS + 2)],
            distances_prices: vec![0u32; (NUM_FULL_DISTANCES << 2) as usize],
            align_prices: vec![0u32; ALIGN_TABLE_SIZE as usize],
            align_price_count: 0,

            dist_table_size: K_DEFAULT_DICTIONARY_LOG_SIZE * 2,

            pos_state_bits: 2,
            pos_state_mask: 3,
            num_literal_pos_state_bits: 0,
            num_literal_context_bits: 3,

            dictionary_size: 1 << K_DEFAULT_DICTIONARY_LOG_SIZE,
            dictionary_size_prev: 0xFFFF_FFFF,
            num_fast_bytes_prev: 0xFFFF_FFFF,

            now_pos64: 0,
            finished: false,
            in_stream: None,

            match_finder_type: EMatchFinderType::BT4,
            write_end_mark: false,

            need_release_mf_stream: false,

            reps: [0; NUM_REP_DISTANCES as usize],
            rep_lens: [0; NUM_REP_DISTANCES as usize],

            match_price_count: 0,
            train_size: 0,

            properties: [0u8; 5],
        }
    }

    /// Enables or disables the end of stream marker.
    pub fn set_write_end_marker_mode(&mut self, write_end_marker: bool) {
        self.write_end_mark = write_end_marker;
    }

    /// Sets the number of bytes that are used to train the match finder.
    pub fn set_train_size(&mut self, train_size: u32) {
        self.train_size = train_size;
    }

    fn base_init(&mut self) {
        self.state.init();
        self.previous_byte = 0;
        for slot in self.rep_distances.iter_mut() {
            *slot = 0;
        }
    }

    fn create(&mut self) -> Result<(), SteamError> {
        self.literal_encoder
            .create(self.num_literal_pos_state_bits, self.num_literal_context_bits);

        if self.dictionary_size == self.dictionary_size_prev
            && self.num_fast_bytes_prev == self.num_fast_bytes
        {
            return Ok(());
        }

        self.match_finder.create(
            self.dictionary_size,
            K_NUM_OPTS,
            self.num_fast_bytes,
            MATCH_MAX_LEN + 1,
        )?;
        self.dictionary_size_prev = self.dictionary_size;
        self.num_fast_bytes_prev = self.num_fast_bytes;

        Ok(())
    }

    fn init(&mut self) {
        self.base_init();
        self.range_encoder.init();

        for i in 0..NUM_STATES {
            for j in 0..=self.pos_state_mask {
                let complex_state = (i << NUM_POS_STATES_BITS_MAX) + j;
                if let Some(encoder) = self.is_match.get_mut(complex_state as usize) {
                    encoder.init();
                }
                if let Some(encoder) = self.is_rep0_long.get_mut(complex_state as usize) {
                    encoder.init();
                }
            }
            if let Some(encoder) = self.is_rep.get_mut(i as usize) {
                encoder.init();
            }
            if let Some(encoder) = self.is_rep_g0.get_mut(i as usize) {
                encoder.init();
            }
            if let Some(encoder) = self.is_rep_g1.get_mut(i as usize) {
                encoder.init();
            }
            if let Some(encoder) = self.is_rep_g2.get_mut(i as usize) {
                encoder.init();
            }
        }
        self.literal_encoder.init();
        for encoder in self.pos_slot_encoder.iter_mut() {
            encoder.init();
        }
        for encoder in self.pos_encoders.iter_mut() {
            encoder.init();
        }

        self.len_encoder.init(1u32 << self.pos_state_bits);
        self.rep_match_len_encoder.init(1u32 << self.pos_state_bits);

        self.pos_align_encoder.init();

        self.longest_match_was_found = false;
        self.optimum_end_index = 0;
        self.optimum_current_index = 0;
        self.additional_offset = 0;
    }

    fn read_match_distances(&mut self) -> (u32, u32) {
        let mut len_res = 0u32;
        let num_distance_pairs = self.match_finder.get_matches(&mut self.match_distances);
        if num_distance_pairs > 0 {
            len_res = self
                .match_distances
                .get((num_distance_pairs - 2) as usize)
                .copied()
                .unwrap_or(0);
            if len_res == self.num_fast_bytes {
                let distance = self
                    .match_distances
                    .get((num_distance_pairs - 1) as usize)
                    .copied()
                    .unwrap_or(0);
                len_res += self.match_finder.get_match_len(
                    len_res as i32 - 1,
                    distance,
                    MATCH_MAX_LEN - len_res,
                );
            }
        }
        self.additional_offset = self.additional_offset.wrapping_add(1);
        (len_res, num_distance_pairs)
    }

    fn move_pos(&mut self, num: u32) {
        if num > 0 {
            self.match_finder.skip(num);
            self.additional_offset = self.additional_offset.wrapping_add(num);
        }
    }

    fn get_rep_len1_price(&self, state: State, pos_state: u32) -> u32 {
        let long_index = ((state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize;
        self.is_rep_g0
            .get(state.index as usize)
            .map(|encoder| encoder.get_price0())
            .unwrap_or(0)
            + self
                .is_rep0_long
                .get(long_index)
                .map(|encoder| encoder.get_price0())
                .unwrap_or(0)
    }

    fn get_pure_rep_price(&self, rep_index: u32, state: State, pos_state: u32) -> u32 {
        if rep_index == 0 {
            let long_index = ((state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize;
            return self
                .is_rep_g0
                .get(state.index as usize)
                .map(|encoder| encoder.get_price0())
                .unwrap_or(0)
                + self
                    .is_rep0_long
                    .get(long_index)
                    .map(|encoder| encoder.get_price1())
                    .unwrap_or(0);
        }

        let mut price = self
            .is_rep_g0
            .get(state.index as usize)
            .map(|encoder| encoder.get_price1())
            .unwrap_or(0);
        if rep_index == 1 {
            price += self
                .is_rep_g1
                .get(state.index as usize)
                .map(|encoder| encoder.get_price0())
                .unwrap_or(0);
        } else {
            price += self
                .is_rep_g1
                .get(state.index as usize)
                .map(|encoder| encoder.get_price1())
                .unwrap_or(0);
            price += self
                .is_rep_g2
                .get(state.index as usize)
                .map(|encoder| encoder.get_price(rep_index - 2))
                .unwrap_or(0);
        }
        price
    }

    fn get_rep_price(&self, rep_index: u32, len: u32, state: State, pos_state: u32) -> u32 {
        self.rep_match_len_encoder
            .get_price(len - MATCH_MIN_LEN, pos_state)
            + self.get_pure_rep_price(rep_index, state, pos_state)
    }

    fn get_pos_len_price(&self, pos: u32, len: u32, pos_state: u32) -> u32 {
        let len_to_pos_state = get_len_to_pos_state(len);
        let price = if pos < NUM_FULL_DISTANCES {
            self.distances_prices
                .get(((len_to_pos_state * NUM_FULL_DISTANCES) + pos) as usize)
                .copied()
                .unwrap_or(0)
        } else {
            self.pos_slot_prices
                .get(((len_to_pos_state << NUM_POS_SLOT_BITS) + get_pos_slot2(pos)) as usize)
                .copied()
                .unwrap_or(0)
                + self
                    .align_prices
                    .get((pos & ALIGN_MASK) as usize)
                    .copied()
                    .unwrap_or(0)
        };
        price + self.len_encoder.get_price(len - MATCH_MIN_LEN, pos_state)
    }

    fn backward(&mut self, cur: u32) -> (u32, u32) {
        self.optimum_end_index = cur;
        let mut pos_mem = self.optimum[cur as usize].pos_prev;
        let mut back_mem = self.optimum[cur as usize].back_prev;
        let mut cur = cur;

        loop {
            if self.optimum[cur as usize].prev1_is_char {
                let pos_mem_index = pos_mem as usize;
                self.optimum[pos_mem_index].make_as_char();
                self.optimum[pos_mem_index].pos_prev = pos_mem.wrapping_sub(1);
                if self.optimum[cur as usize].prev2 {
                    let previous = pos_mem.wrapping_sub(1) as usize;
                    self.optimum[previous].prev1_is_char = false;
                    self.optimum[previous].pos_prev = self.optimum[cur as usize].pos_prev2;
                    self.optimum[previous].back_prev = self.optimum[cur as usize].back_prev2;
                }
            }
            let pos_prev = pos_mem;
            let back_cur = back_mem;

            back_mem = self.optimum[pos_prev as usize].back_prev;
            pos_mem = self.optimum[pos_prev as usize].pos_prev;

            self.optimum[pos_prev as usize].back_prev = back_cur;
            self.optimum[pos_prev as usize].pos_prev = cur;
            cur = pos_prev;
            if cur == 0 {
                break;
            }
        }

        let back_res = self.optimum[0].back_prev;
        self.optimum_current_index = self.optimum[0].pos_prev;
        (self.optimum_current_index, back_res)
    }

    fn fill_distances_prices(&mut self) {
        let mut temp_prices = vec![0u32; NUM_FULL_DISTANCES as usize];

        for i in START_POS_MODEL_INDEX..NUM_FULL_DISTANCES {
            let pos_slot = get_pos_slot(i);
            let footer_bits = ((pos_slot >> 1) - 1) as i32;
            let base_val = (2 | (pos_slot & 1)) << footer_bits;
            temp_prices[i as usize] = BitTreeEncoder::reverse_get_price_at(
                &self.pos_encoders,
                base_val.wrapping_sub(pos_slot).wrapping_sub(1),
                footer_bits,
                i - base_val,
            );
        }

        for len_to_pos_state in 0..NUM_LEN_TO_POS_STATES {
            let st = len_to_pos_state << NUM_POS_SLOT_BITS;
            for pos_slot in 0..self.dist_table_size {
                let price = self
                    .pos_slot_encoder
                    .get(len_to_pos_state as usize)
                    .map(|encoder| encoder.get_price(pos_slot))
                    .unwrap_or(0);
                if let Some(slot) = self.pos_slot_prices.get_mut((st + pos_slot) as usize) {
                    *slot = price;
                }
            }
            for pos_slot in END_POS_MODEL_INDEX..self.dist_table_size {
                let extra = (((pos_slot >> 1) - 1) - NUM_ALIGN_BITS as u32)
                    << BitEncoder::NUM_BIT_PRICE_SHIFT_BITS;
                if let Some(slot) = self.pos_slot_prices.get_mut((st + pos_slot) as usize) {
                    *slot = slot.wrapping_add(extra);
                }
            }

            let st2 = len_to_pos_state * NUM_FULL_DISTANCES;
            for i in 0..START_POS_MODEL_INDEX {
                let price = self
                    .pos_slot_prices
                    .get((st + i) as usize)
                    .copied()
                    .unwrap_or(0);
                if let Some(slot) = self.distances_prices.get_mut((st2 + i) as usize) {
                    *slot = price;
                }
            }
            for i in START_POS_MODEL_INDEX..NUM_FULL_DISTANCES {
                let price = self
                    .pos_slot_prices
                    .get((st + get_pos_slot(i)) as usize)
                    .copied()
                    .unwrap_or(0)
                    + temp_prices.get(i as usize).copied().unwrap_or(0);
                if let Some(slot) = self.distances_prices.get_mut((st2 + i) as usize) {
                    *slot = price;
                }
            }
        }
        self.match_price_count = 0;
    }

    fn fill_align_prices(&mut self) {
        for i in 0..ALIGN_TABLE_SIZE {
            let price = self.pos_align_encoder.reverse_get_price(i);
            if let Some(slot) = self.align_prices.get_mut(i as usize) {
                *slot = price;
            }
        }
        self.align_price_count = 0;
    }

    fn write_end_marker(&mut self, pos_state: u32) {
        if !self.write_end_mark {
            return;
        }

        let complex_state = ((self.state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize;
        if let Some(encoder) = self.is_match.get_mut(complex_state) {
            encoder.encode(&mut self.range_encoder, 1);
        }
        if let Some(encoder) = self.is_rep.get_mut(self.state.index as usize) {
            encoder.encode(&mut self.range_encoder, 0);
        }
        self.state.update_match();
        let len = MATCH_MIN_LEN;
        self.len_encoder
            .encode(&mut self.range_encoder, len - MATCH_MIN_LEN, pos_state);
        let pos_slot = (1u32 << NUM_POS_SLOT_BITS) - 1;
        let len_to_pos_state = get_len_to_pos_state(len);
        if let Some(encoder) = self.pos_slot_encoder.get_mut(len_to_pos_state as usize) {
            encoder.encode(&mut self.range_encoder, pos_slot);
        }
        let footer_bits = 30;
        let pos_reduced = (1u32 << footer_bits) - 1;
        self.range_encoder
            .encode_direct_bits(pos_reduced >> NUM_ALIGN_BITS, footer_bits - NUM_ALIGN_BITS);
        self.pos_align_encoder
            .reverse_encode(&mut self.range_encoder, pos_reduced & ALIGN_MASK);
    }

    fn flush(&mut self, now_pos: u32) {
        self.release_mf_stream();
        self.write_end_marker(now_pos & self.pos_state_mask);
        self.range_encoder.flush_data();
        self.range_encoder.flush_stream();
    }

    fn release_mf_stream(&mut self) {
        if self.need_release_mf_stream {
            self.match_finder.release_stream();
            self.need_release_mf_stream = false;
        }
    }

    fn set_streams(&mut self, in_stream: Vec<u8>) -> Result<(), SteamError> {
        self.in_stream = Some(in_stream);
        self.finished = false;
        self.create()?;
        self.init();

        self.fill_distances_prices();
        self.fill_align_prices();

        self.len_encoder
            .set_table_size(self.num_fast_bytes + 1 - MATCH_MIN_LEN);
        self.len_encoder.update_tables(1u32 << self.pos_state_bits);
        self.rep_match_len_encoder
            .set_table_size(self.num_fast_bytes + 1 - MATCH_MIN_LEN);
        self.rep_match_len_encoder
            .update_tables(1u32 << self.pos_state_bits);

        self.now_pos64 = 0;
        Ok(())
    }

    fn release_streams(&mut self) {
        self.release_mf_stream();
        self.range_encoder.release_stream();
    }
}

impl Encoder {
    #[allow(unused_assignments)]
    fn get_optimum(&mut self, position: u32) -> (u32, u32) {
        let mut position = position;
        if self.optimum_end_index != self.optimum_current_index {
            let index = self.optimum_current_index as usize;
            let len_res = self.optimum[index].pos_prev - self.optimum_current_index;
            let back_res = self.optimum[index].back_prev;
            self.optimum_current_index = self.optimum[index].pos_prev;
            return (len_res, back_res);
        }
        self.optimum_current_index = 0;
        self.optimum_end_index = 0;

        let (len_main, num_distance_pairs);
        if !self.longest_match_was_found {
            let (len, pairs) = self.read_match_distances();
            len_main = len;
            num_distance_pairs = pairs;
        } else {
            len_main = self.longest_match_length;
            num_distance_pairs = self.num_distance_pairs;
            self.longest_match_was_found = false;
        }

        let mut num_available_bytes = self.match_finder.get_num_available_bytes() + 1;
        if num_available_bytes < 2 {
            return (1, 0xFFFF_FFFF);
        }
        if num_available_bytes > MATCH_MAX_LEN {
            num_available_bytes = MATCH_MAX_LEN;
        }

        let mut rep_max_index = 0u32;
        for i in 0..NUM_REP_DISTANCES {
            self.reps[i as usize] = self.rep_distances[i as usize];
            self.rep_lens[i as usize] = self.match_finder.get_match_len(
                -1,
                self.reps[i as usize],
                MATCH_MAX_LEN,
            );
            if self.rep_lens[i as usize] > self.rep_lens[rep_max_index as usize] {
                rep_max_index = i;
            }
        }
        if self.rep_lens[rep_max_index as usize] >= self.num_fast_bytes {
            let len_res = self.rep_lens[rep_max_index as usize];
            self.move_pos(len_res - 1);
            return (len_res, rep_max_index);
        }

        if len_main >= self.num_fast_bytes {
            let back_res = self
                .match_distances
                .get((num_distance_pairs - 1) as usize)
                .copied()
                .unwrap_or(0)
                + NUM_REP_DISTANCES;
            self.move_pos(len_main - 1);
            return (len_main, back_res);
        }

        let mut current_byte = self.match_finder.get_index_byte(-1);
        let mut match_byte = self.match_finder.get_index_byte(
            0u32
                .wrapping_sub(self.rep_distances[0])
                .wrapping_sub(1)
                .wrapping_sub(1) as i32,
        );

        if len_main < 2
            && current_byte != match_byte
            && self.rep_lens[rep_max_index as usize] < 2
        {
            return (1, 0xFFFF_FFFF);
        }

        self.optimum[0].state = self.state;

        let mut pos_state = position & self.pos_state_mask;

        let sub_coder = self
            .literal_encoder
            .get_sub_coder_index(position, self.previous_byte);
        let literal_price = self
            .literal_encoder
            .coder(sub_coder)
            .get_price(!self.state.is_char_state(), match_byte, current_byte);
        self.optimum[1].price = self
            .is_match
            .get(
                ((self.state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize,
            )
            .map(|encoder| encoder.get_price0())
            .unwrap_or(0)
            + literal_price;
        self.optimum[1].make_as_char();

        let mut match_price = self
            .is_match
            .get(
                ((self.state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize,
            )
            .map(|encoder| encoder.get_price1())
            .unwrap_or(0);
        let mut rep_match_price = match_price
            + self
                .is_rep
                .get(self.state.index as usize)
                .map(|encoder| encoder.get_price1())
                .unwrap_or(0);

        if match_byte == current_byte {
            let short_rep_price = rep_match_price + self.get_rep_len1_price(self.state, pos_state);
            if short_rep_price < self.optimum[1].price {
                self.optimum[1].price = short_rep_price;
                self.optimum[1].make_as_short_rep();
            }
        }

        let mut len_end = if len_main >= self.rep_lens[rep_max_index as usize] {
            len_main
        } else {
            self.rep_lens[rep_max_index as usize]
        };

        if len_end < 2 {
            let back_res = self.optimum[1].back_prev;
            return (1, back_res);
        }

        self.optimum[1].pos_prev = 0;

        self.optimum[0].backs0 = self.reps[0];
        self.optimum[0].backs1 = self.reps[1];
        self.optimum[0].backs2 = self.reps[2];
        self.optimum[0].backs3 = self.reps[3];

        let mut len = len_end;
        loop {
            self.optimum[len as usize].price = K_IFINITY_PRICE;
            len = len.wrapping_sub(1);
            if len < 2 {
                break;
            }
        }

        for i in 0..NUM_REP_DISTANCES {
            let mut rep_len = self.rep_lens[i as usize];
            if rep_len < 2 {
                continue;
            }
            let price = rep_match_price + self.get_pure_rep_price(i, self.state, pos_state);
            loop {
                let cur_and_len_price =
                    price + self.rep_match_len_encoder.get_price(rep_len - 2, pos_state);
                {
                    let optimum = &mut self.optimum[rep_len as usize];
                    if cur_and_len_price < optimum.price {
                        optimum.price = cur_and_len_price;
                        optimum.pos_prev = 0;
                        optimum.back_prev = i;
                        optimum.prev1_is_char = false;
                    }
                }
                rep_len = rep_len.wrapping_sub(1);
                if rep_len < 2 {
                    break;
                }
            }
        }

        let mut normal_match_price = match_price
            + self
                .is_rep
                .get(self.state.index as usize)
                .map(|encoder| encoder.get_price0())
                .unwrap_or(0);

        len = if self.rep_lens[0] >= 2 {
            self.rep_lens[0] + 1
        } else {
            2
        };
        if len <= len_main {
            let mut offs = 0u32;
            while len
                > self
                    .match_distances
                    .get(offs as usize)
                    .copied()
                    .unwrap_or(0)
            {
                offs += 2;
            }
            loop {
                let distance = self
                    .match_distances
                    .get((offs + 1) as usize)
                    .copied()
                    .unwrap_or(0);
                let cur_and_len_price =
                    normal_match_price + self.get_pos_len_price(distance, len, pos_state);
                {
                    let optimum = &mut self.optimum[len as usize];
                    if cur_and_len_price < optimum.price {
                        optimum.price = cur_and_len_price;
                        optimum.pos_prev = 0;
                        optimum.back_prev = distance + NUM_REP_DISTANCES;
                        optimum.prev1_is_char = false;
                    }
                }
                if len == self.match_distances.get(offs as usize).copied().unwrap_or(0) {
                    offs += 2;
                    if offs == num_distance_pairs {
                        break;
                    }
                }
                len += 1;
            }
        }

        let mut cur = 0u32;

        loop {
            cur += 1;
            if cur == len_end {
                return self.backward(cur);
            }
            let (mut new_len, mut num_distance_pairs) = self.read_match_distances();
            if new_len >= self.num_fast_bytes {
                self.num_distance_pairs = num_distance_pairs;
                self.longest_match_length = new_len;
                self.longest_match_was_found = true;
                return self.backward(cur);
            }
            position += 1;
            let mut pos_prev = self.optimum[cur as usize].pos_prev;
            let mut state;
            if self.optimum[cur as usize].prev1_is_char {
                pos_prev = pos_prev.wrapping_sub(1);
                if self.optimum[cur as usize].prev2 {
                    state = self.optimum[self.optimum[cur as usize].pos_prev2 as usize].state;
                    if self.optimum[cur as usize].back_prev2 < NUM_REP_DISTANCES {
                        state.update_rep();
                    } else {
                        state.update_match();
                    }
                } else {
                    state = self.optimum[pos_prev as usize].state;
                }
                state.update_char();
            } else {
                state = self.optimum[pos_prev as usize].state;
            }
            if pos_prev == cur.wrapping_sub(1) {
                if self.optimum[cur as usize].is_short_rep() {
                    state.update_short_rep();
                } else {
                    state.update_char();
                }
            } else {
                let pos;
                if self.optimum[cur as usize].prev1_is_char && self.optimum[cur as usize].prev2 {
                    pos_prev = self.optimum[cur as usize].pos_prev2;
                    pos = self.optimum[cur as usize].back_prev2;
                    state.update_rep();
                } else {
                    pos = self.optimum[cur as usize].back_prev;
                    if pos < NUM_REP_DISTANCES {
                        state.update_rep();
                    } else {
                        state.update_match();
                    }
                }
                let opt = self.optimum[pos_prev as usize];
                if pos < NUM_REP_DISTANCES {
                    if pos == 0 {
                        self.reps[0] = opt.backs0;
                        self.reps[1] = opt.backs1;
                        self.reps[2] = opt.backs2;
                        self.reps[3] = opt.backs3;
                    } else if pos == 1 {
                        self.reps[0] = opt.backs1;
                        self.reps[1] = opt.backs0;
                        self.reps[2] = opt.backs2;
                        self.reps[3] = opt.backs3;
                    } else if pos == 2 {
                        self.reps[0] = opt.backs2;
                        self.reps[1] = opt.backs0;
                        self.reps[2] = opt.backs1;
                        self.reps[3] = opt.backs3;
                    } else {
                        self.reps[0] = opt.backs3;
                        self.reps[1] = opt.backs0;
                        self.reps[2] = opt.backs1;
                        self.reps[3] = opt.backs2;
                    }
                } else {
                    self.reps[0] = pos - NUM_REP_DISTANCES;
                    self.reps[1] = opt.backs0;
                    self.reps[2] = opt.backs1;
                    self.reps[3] = opt.backs2;
                }
            }
            self.optimum[cur as usize].state = state;
            self.optimum[cur as usize].backs0 = self.reps[0];
            self.optimum[cur as usize].backs1 = self.reps[1];
            self.optimum[cur as usize].backs2 = self.reps[2];
            self.optimum[cur as usize].backs3 = self.reps[3];
            let cur_price = self.optimum[cur as usize].price;

            current_byte = self.match_finder.get_index_byte(-1);
            match_byte = self.match_finder.get_index_byte(
                0u32
                    .wrapping_sub(self.reps[0])
                    .wrapping_sub(1)
                    .wrapping_sub(1) as i32,
            );

            pos_state = position & self.pos_state_mask;

            let previous_byte = self.match_finder.get_index_byte(-2);
            let sub_coder = self
                .literal_encoder
                .get_sub_coder_index(position, previous_byte);
            let literal_price = self
                .literal_encoder
                .coder(sub_coder)
                .get_price(!state.is_char_state(), match_byte, current_byte);
            let cur_and1_price = cur_price
                + self
                    .is_match
                    .get(
                        ((state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize,
                    )
                    .map(|encoder| encoder.get_price0())
                    .unwrap_or(0)
                + literal_price;

            let next_index = (cur + 1) as usize;
            let mut next_is_char = false;
            if cur_and1_price < self.optimum[next_index].price {
                let optimum = &mut self.optimum[next_index];
                optimum.price = cur_and1_price;
                optimum.pos_prev = cur;
                optimum.make_as_char();
                next_is_char = true;
            }

            match_price = cur_price
                + self
                    .is_match
                    .get(
                        ((state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize,
                    )
                    .map(|encoder| encoder.get_price1())
                    .unwrap_or(0);
            rep_match_price = match_price
                + self
                    .is_rep
                    .get(state.index as usize)
                    .map(|encoder| encoder.get_price1())
                    .unwrap_or(0);

            if match_byte == current_byte
                && !(self.optimum[next_index].pos_prev < cur
                    && self.optimum[next_index].back_prev == 0)
            {
                let short_rep_price = rep_match_price + self.get_rep_len1_price(state, pos_state);
                if short_rep_price <= self.optimum[next_index].price {
                    let optimum = &mut self.optimum[next_index];
                    optimum.price = short_rep_price;
                    optimum.pos_prev = cur;
                    optimum.make_as_short_rep();
                    next_is_char = true;
                }
            }

            let mut num_available_bytes_full = self.match_finder.get_num_available_bytes() + 1;
            num_available_bytes_full = (K_NUM_OPTS - 1 - cur).min(num_available_bytes_full);
            num_available_bytes = num_available_bytes_full;

            if num_available_bytes < 2 {
                continue;
            }
            if num_available_bytes > self.num_fast_bytes {
                num_available_bytes = self.num_fast_bytes;
            }
            if !next_is_char && match_byte != current_byte {
                let t = (num_available_bytes_full - 1).min(self.num_fast_bytes);
                let len_test2 = self.match_finder.get_match_len(0, self.reps[0], t);
                if len_test2 >= 2 {
                    let mut state2 = state;
                    state2.update_char();
                    let pos_state_next = (position + 1) & self.pos_state_mask;
                    let next_rep_match_price = cur_and1_price
                        + self
                            .is_match
                            .get(
                                ((state2.index << NUM_POS_STATES_BITS_MAX) + pos_state_next)
                                    as usize,
                            )
                            .map(|encoder| encoder.get_price1())
                            .unwrap_or(0)
                        + self
                            .is_rep
                            .get(state2.index as usize)
                            .map(|encoder| encoder.get_price1())
                            .unwrap_or(0);
                    let offset = cur + 1 + len_test2;
                    while len_end < offset {
                        len_end += 1;
                        self.optimum[len_end as usize].price = K_IFINITY_PRICE;
                    }
                    let cur_and_len_price = next_rep_match_price
                        + self.get_rep_price(0, len_test2, state2, pos_state_next);
                    {
                        let optimum = &mut self.optimum[offset as usize];
                        if cur_and_len_price < optimum.price {
                            optimum.price = cur_and_len_price;
                            optimum.pos_prev = cur + 1;
                            optimum.back_prev = 0;
                            optimum.prev1_is_char = true;
                            optimum.prev2 = false;
                        }
                    }
                }
            }

            let mut start_len = 2u32;

            for rep_index in 0..NUM_REP_DISTANCES {
                let mut len_test = self.match_finder.get_match_len(
                    -1,
                    self.reps[rep_index as usize],
                    num_available_bytes,
                );
                if len_test < 2 {
                    continue;
                }
                let len_test_temp = len_test;
                loop {
                    while len_end < cur + len_test {
                        len_end += 1;
                        self.optimum[len_end as usize].price = K_IFINITY_PRICE;
                    }
                    let cur_and_len_price = rep_match_price
                        + self.get_rep_price(rep_index, len_test, state, pos_state);
                    {
                        let optimum = &mut self.optimum[(cur + len_test) as usize];
                        if cur_and_len_price < optimum.price {
                            optimum.price = cur_and_len_price;
                            optimum.pos_prev = cur;
                            optimum.back_prev = rep_index;
                            optimum.prev1_is_char = false;
                        }
                    }
                    len_test = len_test.wrapping_sub(1);
                    if len_test < 2 {
                        break;
                    }
                }
                len_test = len_test_temp;

                if rep_index == 0 {
                    start_len = len_test + 1;
                }

                if len_test < num_available_bytes_full {
                    let t = (num_available_bytes_full - 1 - len_test).min(self.num_fast_bytes);
                    let len_test2 = self.match_finder.get_match_len(
                        len_test as i32,
                        self.reps[rep_index as usize],
                        t,
                    );
                    if len_test2 >= 2 {
                        let mut state2 = state;
                        state2.update_rep();
                        let mut pos_state_next = (position + len_test) & self.pos_state_mask;
                        let previous_byte = self.match_finder.get_index_byte(len_test as i32 - 2);
                        let sub_coder = self
                            .literal_encoder
                            .get_sub_coder_index(position + len_test, previous_byte);
                        let match_byte = self.match_finder.get_index_byte(
                            len_test as i32
                                - (self.reps[rep_index as usize].wrapping_add(1) as i32)
                                - 1,
                        );
                        let current_byte = self.match_finder.get_index_byte(len_test as i32 - 1);
                        let literal_price = self
                            .literal_encoder
                            .coder(sub_coder)
                            .get_price(true, match_byte, current_byte);
                        let cur_and_len_char_price = rep_match_price
                            + self.get_rep_price(rep_index, len_test, state, pos_state)
                            + self
                                .is_match
                                .get(
                                    ((state2.index << NUM_POS_STATES_BITS_MAX) + pos_state_next)
                                        as usize,
                                )
                                .map(|encoder| encoder.get_price0())
                                .unwrap_or(0)
                            + literal_price;
                        state2.update_char();
                        pos_state_next = (position + len_test + 1) & self.pos_state_mask;
                        let next_match_price = cur_and_len_char_price
                            + self
                                .is_match
                                .get(
                                    ((state2.index << NUM_POS_STATES_BITS_MAX) + pos_state_next)
                                        as usize,
                                )
                                .map(|encoder| encoder.get_price1())
                                .unwrap_or(0);
                        let next_rep_match_price = next_match_price
                            + self
                                .is_rep
                                .get(state2.index as usize)
                                .map(|encoder| encoder.get_price1())
                                .unwrap_or(0);

                        let offset = len_test + 1 + len_test2;
                        while len_end < cur + offset {
                            len_end += 1;
                            self.optimum[len_end as usize].price = K_IFINITY_PRICE;
                        }
                        let cur_and_len_price = next_rep_match_price
                            + self.get_rep_price(0, len_test2, state2, pos_state_next);
                        {
                            let optimum = &mut self.optimum[(cur + offset) as usize];
                            if cur_and_len_price < optimum.price {
                                optimum.price = cur_and_len_price;
                                optimum.pos_prev = cur + len_test + 1;
                                optimum.back_prev = 0;
                                optimum.prev1_is_char = true;
                                optimum.prev2 = true;
                                optimum.pos_prev2 = cur;
                                optimum.back_prev2 = rep_index;
                            }
                        }
                    }
                }
            }

            if new_len > num_available_bytes {
                new_len = num_available_bytes;
                num_distance_pairs = 0;
                while new_len
                    > self
                        .match_distances
                        .get(num_distance_pairs as usize)
                        .copied()
                        .unwrap_or(0)
                {
                    num_distance_pairs += 2;
                }
                if let Some(slot) = self.match_distances.get_mut(num_distance_pairs as usize) {
                    *slot = new_len;
                }
                num_distance_pairs += 2;
            }
            if new_len >= start_len {
                normal_match_price = match_price
                    + self
                        .is_rep
                        .get(state.index as usize)
                        .map(|encoder| encoder.get_price0())
                        .unwrap_or(0);
                while len_end < cur + new_len {
                    len_end += 1;
                    self.optimum[len_end as usize].price = K_IFINITY_PRICE;
                }

                let mut offs = 0u32;
                while start_len
                    > self
                        .match_distances
                        .get(offs as usize)
                        .copied()
                        .unwrap_or(0)
                {
                    offs += 2;
                }

                let mut len_test = start_len;
                loop {
                    let cur_back = self
                        .match_distances
                        .get((offs + 1) as usize)
                        .copied()
                        .unwrap_or(0);
                    let mut cur_and_len_price =
                        normal_match_price + self.get_pos_len_price(cur_back, len_test, pos_state);
                    {
                        let optimum = &mut self.optimum[(cur + len_test) as usize];
                        if cur_and_len_price < optimum.price {
                            optimum.price = cur_and_len_price;
                            optimum.pos_prev = cur;
                            optimum.back_prev = cur_back + NUM_REP_DISTANCES;
                            optimum.prev1_is_char = false;
                        }
                    }

                    if len_test == self.match_distances.get(offs as usize).copied().unwrap_or(0) {
                        if len_test < num_available_bytes_full {
                            let t =
                                (num_available_bytes_full - 1 - len_test).min(self.num_fast_bytes);
                            let len_test2 =
                                self.match_finder.get_match_len(len_test as i32, cur_back, t);
                            if len_test2 >= 2 {
                                let mut state2 = state;
                                state2.update_match();
                                let mut pos_state_next = (position + len_test) & self.pos_state_mask;
                                let previous_byte =
                                    self.match_finder.get_index_byte(len_test as i32 - 2);
                                let sub_coder = self.literal_encoder.get_sub_coder_index(
                                    position + len_test,
                                    previous_byte,
                                );
                                let match_byte = self.match_finder.get_index_byte(
                                    len_test as i32 - (cur_back.wrapping_add(1) as i32) - 1,
                                );
                                let current_byte =
                                    self.match_finder.get_index_byte(len_test as i32 - 1);
                                let literal_price = self
                                    .literal_encoder
                                    .coder(sub_coder)
                                    .get_price(true, match_byte, current_byte);
                                let cur_and_len_char_price = cur_and_len_price
                                    + self
                                        .is_match
                                        .get(
                                            ((state2.index << NUM_POS_STATES_BITS_MAX)
                                                + pos_state_next)
                                                as usize,
                                        )
                                        .map(|encoder| encoder.get_price0())
                                        .unwrap_or(0)
                                    + literal_price;
                                state2.update_char();
                                pos_state_next = (position + len_test + 1) & self.pos_state_mask;
                                let next_match_price = cur_and_len_char_price
                                    + self
                                        .is_match
                                        .get(
                                            ((state2.index << NUM_POS_STATES_BITS_MAX)
                                                + pos_state_next)
                                                as usize,
                                        )
                                        .map(|encoder| encoder.get_price1())
                                        .unwrap_or(0);
                                let next_rep_match_price = next_match_price
                                    + self
                                        .is_rep
                                        .get(state2.index as usize)
                                        .map(|encoder| encoder.get_price1())
                                        .unwrap_or(0);

                                let offset = len_test + 1 + len_test2;
                                while len_end < cur + offset {
                                    len_end += 1;
                                    self.optimum[len_end as usize].price = K_IFINITY_PRICE;
                                }
                                cur_and_len_price = next_rep_match_price
                                    + self.get_rep_price(0, len_test2, state2, pos_state_next);
                                {
                                    let optimum = &mut self.optimum[(cur + offset) as usize];
                                    if cur_and_len_price < optimum.price {
                                        optimum.price = cur_and_len_price;
                                        optimum.pos_prev = cur + len_test + 1;
                                        optimum.back_prev = 0;
                                        optimum.prev1_is_char = true;
                                        optimum.prev2 = true;
                                        optimum.pos_prev2 = cur;
                                        optimum.back_prev2 = cur_back + NUM_REP_DISTANCES;
                                    }
                                }
                            }
                        }
                        offs += 2;
                        if offs == num_distance_pairs {
                            break;
                        }
                    }
                    len_test += 1;
                }
            }
        }
    }

    fn code_one_block(&mut self) -> (i64, i64, bool) {
        let mut in_size = 0i64;
        let mut out_size = 0i64;
        let mut finished = true;

        if let Some(stream) = self.in_stream.take() {
            self.match_finder.set_stream(stream);
            self.match_finder.init();
            self.need_release_mf_stream = true;
            if self.train_size > 0 {
                self.match_finder.skip(self.train_size);
            }
        }

        if self.finished {
            return (in_size, out_size, finished);
        }
        self.finished = true;

        let progress_pos_value_prev = self.now_pos64;
        if self.now_pos64 == 0 {
            if self.match_finder.get_num_available_bytes() == 0 {
                self.flush(self.now_pos64 as u32);
                return (in_size, out_size, finished);
            }
            self.read_match_distances();
            let pos_state = (self.now_pos64 as u32) & self.pos_state_mask;
            if let Some(encoder) = self.is_match.get_mut(
                ((self.state.index << NUM_POS_STATES_BITS_MAX) + pos_state) as usize,
            ) {
                encoder.encode(&mut self.range_encoder, 0);
            }
            self.state.update_char();
            let cur_byte = self
                .match_finder
                .get_index_byte(0u32.wrapping_sub(self.additional_offset) as i32);
            let sub_coder = self
                .literal_encoder
                .get_sub_coder_index(self.now_pos64 as u32, self.previous_byte);
            self.literal_encoder
                .coder_mut(sub_coder)
                .encode(&mut self.range_encoder, cur_byte);
            self.previous_byte = cur_byte;
            self.additional_offset = self.additional_offset.wrapping_sub(1);
            self.now_pos64 += 1;
        }
        if self.match_finder.get_num_available_bytes() == 0 {
            self.flush(self.now_pos64 as u32);
            return (in_size, out_size, finished);
        }
        loop {
            let (len, pos) = self.get_optimum(self.now_pos64 as u32);

            let pos_state = (self.now_pos64 as u32) & self.pos_state_mask;
            let complex_state = (self.state.index << NUM_POS_STATES_BITS_MAX) + pos_state;
            if len == 1 && pos == 0xFFFF_FFFF {
                if let Some(encoder) = self.is_match.get_mut(complex_state as usize) {
                    encoder.encode(&mut self.range_encoder, 0);
                }
                let cur_byte = self
                    .match_finder
                    .get_index_byte(0u32.wrapping_sub(self.additional_offset) as i32);
                let sub_coder = self
                    .literal_encoder
                    .get_sub_coder_index(self.now_pos64 as u32, self.previous_byte);
                if !self.state.is_char_state() {
                    let match_byte = self.match_finder.get_index_byte(
                        0u32
                            .wrapping_sub(self.rep_distances[0])
                            .wrapping_sub(1)
                            .wrapping_sub(self.additional_offset) as i32,
                    );
                    self.literal_encoder.coder_mut(sub_coder).encode_matched(
                        &mut self.range_encoder,
                        match_byte,
                        cur_byte,
                    );
                } else {
                    self.literal_encoder
                        .coder_mut(sub_coder)
                        .encode(&mut self.range_encoder, cur_byte);
                }
                self.previous_byte = cur_byte;
                self.state.update_char();
            } else {
                if let Some(encoder) = self.is_match.get_mut(complex_state as usize) {
                    encoder.encode(&mut self.range_encoder, 1);
                }
                if pos < NUM_REP_DISTANCES {
                    if let Some(encoder) = self.is_rep.get_mut(self.state.index as usize) {
                        encoder.encode(&mut self.range_encoder, 1);
                    }
                    if pos == 0 {
                        if let Some(encoder) = self.is_rep_g0.get_mut(self.state.index as usize) {
                            encoder.encode(&mut self.range_encoder, 0);
                        }
                        if let Some(encoder) = self.is_rep0_long.get_mut(complex_state as usize) {
                            if len == 1 {
                                encoder.encode(&mut self.range_encoder, 0);
                            } else {
                                encoder.encode(&mut self.range_encoder, 1);
                            }
                        }
                    } else {
                        if let Some(encoder) = self.is_rep_g0.get_mut(self.state.index as usize) {
                            encoder.encode(&mut self.range_encoder, 1);
                        }
                        if pos == 1 {
                            if let Some(encoder) = self.is_rep_g1.get_mut(self.state.index as usize) {
                                encoder.encode(&mut self.range_encoder, 0);
                            }
                        } else {
                            if let Some(encoder) = self.is_rep_g1.get_mut(self.state.index as usize) {
                                encoder.encode(&mut self.range_encoder, 1);
                            }
                            if let Some(encoder) = self.is_rep_g2.get_mut(self.state.index as usize) {
                                encoder.encode(&mut self.range_encoder, pos - 2);
                            }
                        }
                    }
                    if len == 1 {
                        self.state.update_short_rep();
                    } else {
                        self.rep_match_len_encoder.encode(
                            &mut self.range_encoder,
                            len - MATCH_MIN_LEN,
                            pos_state,
                        );
                        self.state.update_rep();
                    }
                    let distance = self.rep_distances.get(pos as usize).copied().unwrap_or(0);
                    if pos != 0 {
                        let mut i = pos;
                        while i >= 1 {
                            let previous = self.rep_distances.get((i - 1) as usize).copied().unwrap_or(0);
                            if let Some(slot) = self.rep_distances.get_mut(i as usize) {
                                *slot = previous;
                            }
                            i -= 1;
                        }
                        self.rep_distances[0] = distance;
                    }
                } else {
                    if let Some(encoder) = self.is_rep.get_mut(self.state.index as usize) {
                        encoder.encode(&mut self.range_encoder, 0);
                    }
                    self.state.update_match();
                    self.len_encoder.encode(
                        &mut self.range_encoder,
                        len - MATCH_MIN_LEN,
                        pos_state,
                    );
                    let pos = pos - NUM_REP_DISTANCES;
                    let pos_slot = get_pos_slot(pos);
                    let len_to_pos_state = get_len_to_pos_state(len);
                    if let Some(encoder) = self.pos_slot_encoder.get_mut(len_to_pos_state as usize) {
                        encoder.encode(&mut self.range_encoder, pos_slot);
                    }

                    if pos_slot >= START_POS_MODEL_INDEX {
                        let footer_bits = ((pos_slot >> 1) - 1) as i32;
                        let base_val = (2 | (pos_slot & 1)) << footer_bits;
                        let pos_reduced = pos - base_val;

                        if pos_slot < END_POS_MODEL_INDEX {
                            BitTreeEncoder::reverse_encode_at(
                                &mut self.pos_encoders,
                                base_val.wrapping_sub(pos_slot).wrapping_sub(1),
                                &mut self.range_encoder,
                                footer_bits,
                                pos_reduced,
                            );
                        } else {
                            self.range_encoder.encode_direct_bits(
                                pos_reduced >> NUM_ALIGN_BITS,
                                footer_bits - NUM_ALIGN_BITS,
                            );
                            self.pos_align_encoder.reverse_encode(
                                &mut self.range_encoder,
                                pos_reduced & ALIGN_MASK,
                            );
                            self.align_price_count += 1;
                        }
                    }
                    let distance = pos;
                    let mut i = NUM_REP_DISTANCES - 1;
                    while i >= 1 {
                        let previous = self.rep_distances.get((i - 1) as usize).copied().unwrap_or(0);
                        if let Some(slot) = self.rep_distances.get_mut(i as usize) {
                            *slot = previous;
                        }
                        i -= 1;
                    }
                    self.rep_distances[0] = distance;
                    self.match_price_count += 1;
                }
                self.previous_byte = self.match_finder.get_index_byte(
                    len.wrapping_sub(1).wrapping_sub(self.additional_offset) as i32,
                );
            }
            self.additional_offset = self.additional_offset.wrapping_sub(len);
            self.now_pos64 += len as i64;
            if self.additional_offset == 0 {
                if self.match_price_count >= (1 << 7) {
                    self.fill_distances_prices();
                }
                if self.align_price_count >= ALIGN_TABLE_SIZE {
                    self.fill_align_prices();
                }
                in_size = self.now_pos64;
                out_size = self.range_encoder.get_processed_size_add();
                if self.match_finder.get_num_available_bytes() == 0 {
                    self.flush(self.now_pos64 as u32);
                    return (in_size, out_size, finished);
                }

                if self.now_pos64 - progress_pos_value_prev >= (1 << 12) {
                    self.finished = false;
                    finished = false;
                    return (in_size, out_size, finished);
                }
            }
        }
    }
}

fn find_match_finder(s: &str) -> Option<EMatchFinderType> {
    match s {
        "BT2" => Some(EMatchFinderType::BT2),
        "BT4" => Some(EMatchFinderType::BT4),
        _ => None,
    }
}

impl ISetCoderProperties for Encoder {
    fn set_coder_properties(
        &mut self,
        prop_ids: &[CoderPropID],
        properties: &[CoderProperty],
    ) -> Result<(), InvalidParamException> {
        for (index, property) in properties.iter().enumerate() {
            let prop_id = prop_ids.get(index).copied().ok_or(InvalidParamException)?;
            match prop_id {
                CoderPropID::NumFastBytes => {
                    let num_fast_bytes = property.as_i32().ok_or(InvalidParamException)?;
                    if num_fast_bytes < 5 || num_fast_bytes > MATCH_MAX_LEN as i32 {
                        return Err(InvalidParamException);
                    }
                    self.num_fast_bytes = num_fast_bytes as u32;
                }
                CoderPropID::Algorithm => {}
                CoderPropID::MatchFinder => {
                    let name = property.as_str().ok_or(InvalidParamException)?;
                    let match_finder_type =
                        find_match_finder(&name.to_uppercase()).ok_or(InvalidParamException)?;
                    let previous = self.match_finder_type;
                    self.match_finder_type = match_finder_type;
                    if previous != match_finder_type {
                        self.dictionary_size_prev = 0xFFFF_FFFF;
                        let mut match_finder = BinTree::new();
                        match_finder.set_type(match match_finder_type {
                            EMatchFinderType::BT2 => 2,
                            EMatchFinderType::BT4 => 4,
                        });
                        self.match_finder = Box::new(match_finder);
                    }
                }
                CoderPropID::DictionarySize => {
                    const K_DIC_LOG_SIZE_MAX_COMPRESS: u32 = 30;
                    let dictionary_size = property.as_i32().ok_or(InvalidParamException)?;
                    if dictionary_size < 1
                        || dictionary_size as u32 > (1u32 << K_DIC_LOG_SIZE_MAX_COMPRESS)
                    {
                        return Err(InvalidParamException);
                    }
                    let mut dic_log_size = 0u32;
                    while dic_log_size < K_DIC_LOG_SIZE_MAX_COMPRESS {
                        if dictionary_size as u32 <= (1u32 << dic_log_size) {
                            break;
                        }
                        dic_log_size += 1;
                    }
                    self.dictionary_size = dictionary_size as u32;
                    self.dist_table_size = dic_log_size * 2;
                }
                CoderPropID::PosStateBits => {
                    let value = property.as_i32().ok_or(InvalidParamException)?;
                    if value < 0 || value > NUM_POS_STATES_BITS_ENCODING_MAX {
                        return Err(InvalidParamException);
                    }
                    self.pos_state_bits = value;
                    self.pos_state_mask = (1u32 << value) - 1;
                }
                CoderPropID::LitPosBits => {
                    let value = property.as_i32().ok_or(InvalidParamException)?;
                    if value < 0 || value > super::lzma_base::NUM_LIT_POS_STATES_BITS_ENCODING_MAX as i32
                    {
                        return Err(InvalidParamException);
                    }
                    self.num_literal_pos_state_bits = value;
                }
                CoderPropID::LitContextBits => {
                    let value = property.as_i32().ok_or(InvalidParamException)?;
                    if value < 0 || value > super::lzma_base::NUM_LIT_CONTEXT_BITS_MAX as i32 {
                        return Err(InvalidParamException);
                    }
                    self.num_literal_context_bits = value;
                }
                CoderPropID::EndMarker => {
                    let value = property.as_bool().ok_or(InvalidParamException)?;
                    self.set_write_end_marker_mode(value);
                }
                _ => return Err(InvalidParamException),
            }
        }
        Ok(())
    }
}

impl IWriteCoderProperties for Encoder {
    fn write_coder_properties(&mut self, out_stream: &mut Vec<u8>) {
        self.properties[0] = ((self.pos_state_bits * 5 + self.num_literal_pos_state_bits) * 9
            + self.num_literal_context_bits) as u8;
        for i in 0..4 {
            self.properties[1 + i] = ((self.dictionary_size >> (8 * i)) & 0xFF) as u8;
        }
        out_stream.extend_from_slice(&self.properties);
    }
}

impl ICoder for Encoder {
    fn code(
        &mut self,
        in_stream: Vec<u8>,
        out_stream: &mut Vec<u8>,
        in_size: i64,
        out_size: i64,
        progress: Option<&mut dyn ICodeProgress>,
    ) -> Result<(), SteamError> {
        self.need_release_mf_stream = false;

        let buffer = std::mem::take(out_stream);
        self.range_encoder.set_stream(buffer);

        let result = self.run(in_stream, in_size, out_size, progress);

        *out_stream = self.range_encoder.release_stream();
        self.release_streams();

        result
    }
}

impl Encoder {
    fn run(
        &mut self,
        in_stream: Vec<u8>,
        _in_size: i64,
        _out_size: i64,
        mut progress: Option<&mut dyn ICodeProgress>,
    ) -> Result<(), SteamError> {
        if let Err(error) = self.set_streams(in_stream) {
            return Err(error);
        }

        loop {
            let (processed_in_size, processed_out_size, finished) = self.code_one_block();
            if finished {
                break;
            }
            if let Some(progress) = progress.as_mut() {
                progress.set_progress(processed_in_size, processed_out_size);
            }
        }
        Ok(())
    }

    /// The compressed bytes produced by the last call to
    /// [`ICoder::code`].
    pub fn take_output(&mut self) -> Vec<u8> {
        self.range_encoder.release_stream()
    }
}



