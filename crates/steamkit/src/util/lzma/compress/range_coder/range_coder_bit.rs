//! Binary probability models from the LZMA SDK (`RangeCoderBit`).
//!
//! Probabilities are updated with the usual 7-Zip formula
//! (`prob += (2048 - prob) >> 5` or `prob -= prob >> 5`). The price table
//! matches the SDK exactly, since the encoder's choices depend on it.

use super::range_coder::Encoder;

/// Static table of bit prices.
const PROB_PRICES: [u32; 512] = build_prob_prices();

const NUM_BITS: i32 = 9;

const fn build_prob_prices() -> [u32; 512] {
    let mut table = [0u32; 512];
    let mut i = NUM_BITS - 1;
    while i >= 0 {
        let start = 1u32 << (NUM_BITS - i - 1);
        let end = 1u32 << (NUM_BITS - i);
        let mut j = start;
        while j < end {
            table[j as usize] = ((i as u32) << 6) + (((end - j) << 6) >> (NUM_BITS - i - 1));
            j += 1;
        }
        i -= 1;
    }
    table
}

/// A single binary probability model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BitEncoder {
    prob: u32,
}

impl BitEncoder {
    /// The number of bits used to represent a probability.
    pub const NUM_BIT_MODEL_TOTAL_BITS: i32 = 11;

    /// The total number of probability units.
    pub const BIT_MODEL_TOTAL: u32 = 1 << Self::NUM_BIT_MODEL_TOTAL_BITS;

    /// The number of bits the probability is shifted by on every update.
    pub const NUM_MOVE_BITS: i32 = 5;

    /// The number of bits the probability is reduced by when pricing.
    pub const NUM_MOVE_REDUCING_BITS: i32 = 2;

    /// The shift applied to the price of a bit.
    pub const NUM_BIT_PRICE_SHIFT_BITS: i32 = 6;

    /// Creates an uninitialised model (probability zero).
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the probability to its initial value.
    pub fn init(&mut self) {
        self.prob = Self::BIT_MODEL_TOTAL >> 1;
    }

    /// The current probability.
    pub fn prob(&self) -> u32 {
        self.prob
    }

    /// Updates the probability for the given symbol.
    pub fn update_model(&mut self, symbol: u32) {
        if symbol == 0 {
            self.prob += (Self::BIT_MODEL_TOTAL - self.prob) >> Self::NUM_MOVE_BITS;
        } else {
            self.prob -= self.prob >> Self::NUM_MOVE_BITS;
        }
    }

    /// Encodes one bit with this model and updates it.
    pub fn encode(&mut self, encoder: &mut Encoder, symbol: u32) {
        let new_bound = (encoder.range >> Self::NUM_BIT_MODEL_TOTAL_BITS) * self.prob;
        if symbol == 0 {
            encoder.range = new_bound;
            self.prob += (Self::BIT_MODEL_TOTAL - self.prob) >> Self::NUM_MOVE_BITS;
        } else {
            encoder.low = encoder.low.wrapping_add(new_bound as u64);
            encoder.range -= new_bound;
            self.prob -= self.prob >> Self::NUM_MOVE_BITS;
        }
        if encoder.range < Encoder::TOP_VALUE {
            encoder.range <<= 8;
            encoder.shift_low();
        }
    }

    /// The price of encoding `symbol`.
    pub fn get_price(&self, symbol: u32) -> u32 {
        let index = (self.prob.wrapping_sub(symbol) ^ ((symbol as i32).wrapping_neg() as u32))
            & (Self::BIT_MODEL_TOTAL - 1);
        PROB_PRICES[(index >> Self::NUM_MOVE_REDUCING_BITS) as usize]
    }

    /// The price of encoding a zero bit.
    pub fn get_price0(&self) -> u32 {
        PROB_PRICES[(self.prob >> Self::NUM_MOVE_REDUCING_BITS) as usize]
    }

    /// The price of encoding a one bit.
    pub fn get_price1(&self) -> u32 {
        PROB_PRICES[((Self::BIT_MODEL_TOTAL - self.prob) >> Self::NUM_MOVE_REDUCING_BITS) as usize]
    }
}
