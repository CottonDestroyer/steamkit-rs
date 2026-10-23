//! Bit tree encoders from the LZMA SDK (`RangeCoderBitTree`).

use super::range_coder::Encoder;
use super::range_coder_bit::BitEncoder;

/// A tree of binary probability models, addressed by the bits of a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitTreeEncoder {
    models: Vec<BitEncoder>,
    num_bit_levels: u32,
}

impl BitTreeEncoder {
    /// Creates a tree with `num_bit_levels` levels.
    pub fn new(num_bit_levels: u32) -> Self {
        Self {
            models: vec![BitEncoder::new(); 1usize << num_bit_levels],
            num_bit_levels,
        }
    }

    /// The number of levels of the tree.
    pub fn num_bit_levels(&self) -> u32 {
        self.num_bit_levels
    }

    /// Resets every model of the tree.
    pub fn init(&mut self) {
        for model in self.models.iter_mut().skip(1) {
            model.init();
        }
    }

    /// Encodes a symbol, most significant bit first.
    pub fn encode(&mut self, range_encoder: &mut Encoder, symbol: u32) {
        let mut m = 1u32;
        let mut bit_index = self.num_bit_levels as i32;
        while bit_index > 0 {
            bit_index -= 1;
            let bit = (symbol >> bit_index) & 1;
            self.models[m as usize].encode(range_encoder, bit);
            m = (m << 1) | bit;
        }
    }

    /// Encodes a symbol, least significant bit first.
    pub fn reverse_encode(&mut self, range_encoder: &mut Encoder, symbol: u32) {
        let mut m = 1u32;
        let mut symbol = symbol;
        for _ in 0..self.num_bit_levels {
            let bit = symbol & 1;
            self.models[m as usize].encode(range_encoder, bit);
            m = (m << 1) | bit;
            symbol >>= 1;
        }
    }

    /// The price of encoding a symbol, most significant bit first.
    pub fn get_price(&self, symbol: u32) -> u32 {
        let mut price = 0u32;
        let mut m = 1u32;
        let mut bit_index = self.num_bit_levels as i32;
        while bit_index > 0 {
            bit_index -= 1;
            let bit = (symbol >> bit_index) & 1;
            price += self.models[m as usize].get_price(bit);
            m = (m << 1) + bit;
        }
        price
    }

    /// The price of encoding a symbol, least significant bit first.
    pub fn reverse_get_price(&self, symbol: u32) -> u32 {
        let mut price = 0u32;
        let mut m = 1u32;
        let mut symbol = symbol;
        let mut i = self.num_bit_levels as i32;
        while i > 0 {
            let bit = symbol & 1;
            symbol >>= 1;
            price += self.models[m as usize].get_price(bit);
            m = (m << 1) | bit;
            i -= 1;
        }
        price
    }

    /// The price of encoding a symbol with an external tree, least significant
    /// bit first.
    pub fn reverse_get_price_at(
        models: &[BitEncoder],
        start_index: u32,
        num_bit_levels: i32,
        symbol: u32,
    ) -> u32 {
        let mut price = 0u32;
        let mut m = 1u32;
        let mut symbol = symbol;
        let mut i = num_bit_levels;
        while i > 0 {
            let bit = symbol & 1;
            symbol >>= 1;
            price += models[start_index.wrapping_add(m) as usize].get_price(bit);
            m = (m << 1) | bit;
            i -= 1;
        }
        price
    }

    /// Encodes a symbol with an external tree, least significant bit first.
    pub fn reverse_encode_at(
        models: &mut [BitEncoder],
        start_index: u32,
        range_encoder: &mut Encoder,
        num_bit_levels: i32,
        symbol: u32,
    ) {
        let mut m = 1u32;
        let mut symbol = symbol;
        for _ in 0..num_bit_levels {
            let bit = symbol & 1;
            models[start_index.wrapping_add(m) as usize].encode(range_encoder, bit);
            m = (m << 1) | bit;
            symbol >>= 1;
        }
    }
}
