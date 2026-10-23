//! A small, allocation-free LZMA decoder for raw streams whose uncompressed size
//! is known up front. The output buffer doubles as the dictionary, so the whole
//! thing is decoded in one call.

use crate::error::SteamError;

const STATE_COUNT: usize = 12;
const LITERAL_STATE_COUNT: usize = 7;
const POS_STATE_BITS_MAX: usize = 4;
const LITERAL_CODER_SIZE: usize = 0x300;

const END_POS_MODEL_INDEX: u32 = 14;
const FULL_DISTANCE_COUNT: u32 = 1 << (END_POS_MODEL_INDEX >> 1);
const ALIGN_BITS: u32 = 4;

const LENGTH_CHOICE: usize = 0;
const LENGTH_CHOICE2: usize = 1;
const LENGTH_LOW: usize = 2;
const LENGTH_MID: usize = LENGTH_LOW + (8 << POS_STATE_BITS_MAX);
const LENGTH_HIGH: usize = LENGTH_MID + (8 << POS_STATE_BITS_MAX);
const LENGTH_CODER_SIZE: usize = LENGTH_HIGH + 256;

const IS_MATCH: usize = 0;
const IS_REP: usize = IS_MATCH + (STATE_COUNT << POS_STATE_BITS_MAX);
const IS_REP_G0: usize = IS_REP + STATE_COUNT;
const IS_REP_G1: usize = IS_REP_G0 + STATE_COUNT;
const IS_REP_G2: usize = IS_REP_G1 + STATE_COUNT;
const IS_REP0_LONG: usize = IS_REP_G2 + STATE_COUNT;
const POS_SLOT: usize = IS_REP0_LONG + (STATE_COUNT << POS_STATE_BITS_MAX);
const SPEC_POS: usize = POS_SLOT + (4 << 6);
const ALIGN: usize = SPEC_POS + FULL_DISTANCE_COUNT as usize - END_POS_MODEL_INDEX as usize;
const MATCH_LENGTH: usize = ALIGN + (1 << ALIGN_BITS) as usize;
const REP_LENGTH: usize = MATCH_LENGTH + LENGTH_CODER_SIZE;
const LITERAL: usize = REP_LENGTH + LENGTH_CODER_SIZE;

/// Decodes raw LZMA streams whose uncompressed size is known up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LzmaDecoder;

impl LzmaDecoder {
    /// Size of the properties that precede an LZMA stream: one byte for
    /// lc/lp/pb and four for the dictionary size.
    pub const PROPERTIES_SIZE: usize = 5;

    /// Decodes a raw LZMA stream into `output`, after an optional preset
    /// dictionary at its start.
    ///
    /// * `properties` - the 5 property bytes.
    /// * `input` - the compressed stream.
    /// * `output` - starts with the preset dictionary; the rest receives the
    ///   uncompressed bytes.
    /// * `preset_dictionary_length` - how many bytes at the start of `output`
    ///   matches can refer to before anything is decoded.
    pub fn decode(
        properties: &[u8],
        input: &[u8],
        output: &mut [u8],
        preset_dictionary_length: usize,
    ) -> Result<(), SteamError> {
        if preset_dictionary_length > output.len() {
            return Err(SteamError::InvalidArgument(
                "the preset dictionary does not fit in the output buffer".to_string(),
            ));
        }

        if properties.len() < Self::PROPERTIES_SIZE {
            return Err(SteamError::Decompression(
                "LZMA properties are too short".to_string(),
            ));
        }

        let properties_byte = properties[0];

        if properties_byte >= 9 * 5 * 5 {
            return Err(SteamError::Decompression(
                "Invalid LZMA properties".to_string(),
            ));
        }

        let lc = (properties_byte % 9) as usize;
        let lp = (properties_byte / 9 % 5) as usize;
        let pb = (properties_byte / (9 * 5)) as usize;

        let probability_count = LITERAL + (LITERAL_CODER_SIZE << (lc + lp));
        let mut probabilities = vec![0u16; probability_count];

        decode_stream(
            &mut probabilities,
            input,
            output,
            preset_dictionary_length,
            lc,
            lp,
            pb,
        )
    }
}

fn get_state_after_literal(state: usize) -> usize {
    if state < 4 {
        return 0;
    }
    if state < 10 {
        return state - 3;
    }
    state - 6
}

fn decode_stream(
    probabilities: &mut [u16],
    input: &[u8],
    output: &mut [u8],
    position: usize,
    lc: usize,
    lp: usize,
    pb: usize,
) -> Result<(), SteamError> {
    for probability in probabilities.iter_mut() {
        *probability = RangeDecoder::INITIAL_PROBABILITY;
    }

    let mut range_decoder = RangeDecoder::new(input)?;

    let pos_state_mask = (1usize << pb) - 1;
    let literal_pos_mask = (1usize << lp) - 1;
    let literal_shift = 8 - lc;

    let mut state = 0usize;
    let mut rep0 = 0u32;
    let mut rep1 = 0u32;
    let mut rep2 = 0u32;
    let mut rep3 = 0u32;
    let mut position = position;

    while position < output.len() {
        let pos_state = position & pos_state_mask;
        let state_index = (state << POS_STATE_BITS_MAX) + pos_state;

        if range_decoder.decode_bit(&mut probabilities[IS_MATCH + state_index])? == 0 {
            let previous_byte = if position > 0 { output[position - 1] } else { 0 };
            let literal_state = ((position & literal_pos_mask) << lc)
                + ((previous_byte as u32) >> literal_shift) as usize;
            let start = LITERAL + LITERAL_CODER_SIZE * literal_state;

            if state < LITERAL_STATE_COUNT {
                let byte = range_decoder
                    .decode_literal(&mut probabilities[start..start + LITERAL_CODER_SIZE])?;
                output[position] = byte;
            } else {
                let match_index = position
                    .checked_sub(rep0 as usize + 1)
                    .ok_or_else(|| {
                        SteamError::Decompression(
                            "LZMA match distance is outside of the decoded data".to_string(),
                        )
                    })?;
                let match_byte = *output.get(match_index).ok_or_else(|| {
                    SteamError::Decompression(
                        "LZMA match distance is outside of the decoded data".to_string(),
                    )
                })?;
                let byte = range_decoder.decode_matched_literal(
                    &mut probabilities[start..start + LITERAL_CODER_SIZE],
                    match_byte,
                )?;
                output[position] = byte;
            }

            position += 1;
            state = get_state_after_literal(state);
            continue;
        }

        let length;

        if range_decoder.decode_bit(&mut probabilities[IS_REP + state])? == 0 {
            rep3 = rep2;
            rep2 = rep1;
            rep1 = rep0;

            length = range_decoder.decode_length(
                &mut probabilities[MATCH_LENGTH..MATCH_LENGTH + LENGTH_CODER_SIZE],
                pos_state,
            )?;
            state = if state < LITERAL_STATE_COUNT { 7 } else { 10 };
            rep0 = range_decoder.decode_distance(&mut *probabilities, length)?;

            if rep0 == u32::MAX {
                return Err(SteamError::Decompression(
                    "LZMA stream ended before the expected uncompressed size".to_string(),
                ));
            }

            if rep0 as usize >= position {
                return Err(SteamError::Decompression(
                    "LZMA match distance is outside of the decoded data".to_string(),
                ));
            }
        } else {
            if position == 0 {
                return Err(SteamError::Decompression(
                    "LZMA stream starts with a repeated match".to_string(),
                ));
            }

            if range_decoder.decode_bit(&mut probabilities[IS_REP_G0 + state])? == 0 {
                if range_decoder.decode_bit(&mut probabilities[IS_REP0_LONG + state_index])? == 0 {
                    let source = position
                        .checked_sub(rep0 as usize + 1)
                        .ok_or_else(|| {
                            SteamError::Decompression(
                                "LZMA match distance is outside of the decoded data".to_string(),
                            )
                        })?;
                    let byte = *output.get(source).ok_or_else(|| {
                        SteamError::Decompression(
                            "LZMA match distance is outside of the decoded data".to_string(),
                        )
                    })?;
                    output[position] = byte;
                    position += 1;
                    state = if state < LITERAL_STATE_COUNT { 9 } else { 11 };
                    continue;
                }
            } else {
                let distance;

                if range_decoder.decode_bit(&mut probabilities[IS_REP_G1 + state])? == 0 {
                    distance = rep1;
                } else {
                    if range_decoder.decode_bit(&mut probabilities[IS_REP_G2 + state])? == 0 {
                        distance = rep2;
                    } else {
                        distance = rep3;
                        rep3 = rep2;
                    }

                    rep2 = rep1;
                }

                rep1 = rep0;
                rep0 = distance;
            }

            length = range_decoder.decode_length(
                &mut probabilities[REP_LENGTH..REP_LENGTH + LENGTH_CODER_SIZE],
                pos_state,
            )?;
            state = if state < LITERAL_STATE_COUNT { 8 } else { 11 };
        }

        let match_length = ((length as usize) + 2).min(output.len() - position);
        let match_distance = rep0 as usize + 1;

        if match_distance >= match_length {
            let source = position - match_distance;
            output.copy_within(source..source + match_length, position);
        } else if match_distance == 1 {
            let byte = output[position - 1];
            for slot in output[position..position + match_length].iter_mut() {
                *slot = byte;
            }
        } else {
            for offset in 0..match_length {
                output[position + offset] = output[position + offset - match_distance];
            }
        }

        position += match_length;
    }

    Ok(())
}

struct RangeDecoder<'a> {
    input: &'a [u8],
    position: usize,
    range: u32,
    code: u32,
}

impl<'a> RangeDecoder<'a> {
    const INITIAL_PROBABILITY: u16 = 1 << (RangeDecoder::BIT_MODEL_TOTAL_BITS - 1);

    const INITIAL_BYTES: usize = 5;
    const BIT_MODEL_TOTAL_BITS: u32 = 11;
    const MOVE_BITS: u32 = 5;
    const TOP_VALUE: u32 = 1 << 24;
    const PROBABILITY_ZERO_OFFSET: u32 = (1u32 << RangeDecoder::MOVE_BITS)
        .wrapping_sub(1)
        .wrapping_sub(1u32 << RangeDecoder::BIT_MODEL_TOTAL_BITS);

    fn new(input: &'a [u8]) -> Result<Self, SteamError> {
        if input.len() < Self::INITIAL_BYTES {
            return Err(SteamError::Decompression(
                "LZMA stream is truncated".to_string(),
            ));
        }

        Ok(Self {
            input,
            position: Self::INITIAL_BYTES,
            range: u32::MAX,
            code: u32::from_be_bytes([input[1], input[2], input[3], input[4]]),
        })
    }

    fn decode_bit(&mut self, probability: &mut u16) -> Result<u32, SteamError> {
        let prob = *probability as u32;
        let bound = (self.range >> Self::BIT_MODEL_TOTAL_BITS) * prob;
        let bit;

        if self.code < bound {
            self.range = bound;
            *probability =
                (prob + (((1u32 << Self::BIT_MODEL_TOTAL_BITS) - prob) >> Self::MOVE_BITS)) as u16;
            bit = 0;
        } else {
            self.range -= bound;
            self.code -= bound;
            *probability = (prob - (prob >> Self::MOVE_BITS)) as u16;
            bit = 1;
        }

        if self.range < Self::TOP_VALUE {
            self.normalize()?;
        }

        Ok(bit)
    }

    /// Same as [`Self::decode_bit`], but selects the results with masks
    /// instead of branching.
    fn decode_tree_bit(&mut self, probability: &mut u16) -> Result<u32, SteamError> {
        let prob = *probability as u32;
        let bound = (self.range >> Self::BIT_MODEL_TOTAL_BITS) * prob;

        let zero_mask = 0u32.wrapping_sub(
            (((self.code as u64).wrapping_sub(bound as u64)) >> 63) as u32,
        );
        let one_mask = !zero_mask;

        self.range = (bound & zero_mask) | (self.range.wrapping_sub(bound) & one_mask);
        self.code = self.code.wrapping_sub(bound & one_mask);

        let adjusted = prob.wrapping_add(zero_mask & Self::PROBABILITY_ZERO_OFFSET);
        *probability = prob.wrapping_sub(adjusted >> Self::MOVE_BITS) as u16;

        if self.range < Self::TOP_VALUE {
            self.normalize()?;
        }

        Ok(one_mask & 1)
    }

    fn decode_literal(&mut self, probabilities: &mut [u16]) -> Result<u8, SteamError> {
        let mut symbol = 1u32;

        while symbol < 0x100 {
            let bit = self.decode_tree_bit(&mut probabilities[symbol as usize])?;
            symbol = (symbol << 1) | bit;
        }

        Ok(symbol as u8)
    }

    fn decode_matched_literal(
        &mut self,
        probabilities: &mut [u16],
        match_byte: u8,
    ) -> Result<u8, SteamError> {
        let mut symbol = 1u32;
        let mut match_byte = match_byte as u32;
        let mut offset = 0x100u32;

        while symbol < 0x100 {
            match_byte <<= 1;

            let previous_offset = offset;
            offset &= match_byte;

            let index = (previous_offset + offset + symbol) as usize;
            let bit = self.decode_tree_bit(&mut probabilities[index])?;
            symbol = (symbol << 1) | bit;

            // Leaves 0x100 only when the decoded bit equals the match bit.
            // `bit - 1` has to wrap to 0xFFFFFFFF when the bit is zero.
            offset ^= previous_offset & bit.wrapping_sub(1);
        }

        Ok(symbol as u8)
    }

    /// Decodes a match length, minus the minimum match length of 2.
    fn decode_length(&mut self, probabilities: &mut [u16], pos_state: usize) -> Result<u32, SteamError> {
        if self.decode_bit(&mut probabilities[LENGTH_CHOICE])? == 0 {
            return self.decode_bit_tree(&mut probabilities[LENGTH_LOW + (pos_state << 3)..], 3);
        }

        if self.decode_bit(&mut probabilities[LENGTH_CHOICE2])? == 0 {
            return Ok(8
                + self.decode_bit_tree(&mut probabilities[LENGTH_MID + (pos_state << 3)..], 3)?);
        }

        Ok(16 + self.decode_bit_tree(&mut probabilities[LENGTH_HIGH..], 8)?)
    }

    /// Decodes a match distance, minus one. Returns `u32::MAX` for the end
    /// marker.
    fn decode_distance(&mut self, probabilities: &mut [u16], length: u32) -> Result<u32, SteamError> {
        let length_state = length.min(3);
        let pos_slot = self.decode_bit_tree(
            &mut probabilities[POS_SLOT + ((length_state as usize) << 6)..],
            6,
        )?;

        if pos_slot < 4 {
            return Ok(pos_slot);
        }

        let direct_bits = (pos_slot >> 1) - 1;
        let mut distance = (2 | (pos_slot & 1)) << direct_bits;

        if pos_slot < END_POS_MODEL_INDEX {
            let start = SPEC_POS + distance as usize - pos_slot as usize - 1;
            return Ok(distance
                + self.decode_reverse_bit_tree(&mut probabilities[start..], direct_bits as usize)?);
        }

        distance += self.decode_direct_bits((direct_bits - ALIGN_BITS) as usize)? << ALIGN_BITS;
        Ok(distance
            + self.decode_reverse_bit_tree(&mut probabilities[ALIGN..], ALIGN_BITS as usize)?)
    }

    fn decode_bit_tree(&mut self, probabilities: &mut [u16], bit_count: usize) -> Result<u32, SteamError> {
        let mut symbol = 1u32;

        for _ in 0..bit_count {
            let bit = self.decode_tree_bit(&mut probabilities[symbol as usize])?;
            symbol = (symbol << 1) | bit;
        }

        Ok(symbol - (1u32 << bit_count))
    }

    fn decode_reverse_bit_tree(
        &mut self,
        probabilities: &mut [u16],
        bit_count: usize,
    ) -> Result<u32, SteamError> {
        let mut index = 1u32;
        let mut symbol = 0u32;

        for i in 0..bit_count {
            let bit = self.decode_tree_bit(&mut probabilities[index as usize])?;
            index = (index << 1) | bit;
            symbol |= bit << i;
        }

        Ok(symbol)
    }

    fn decode_direct_bits(&mut self, bit_count: usize) -> Result<u32, SteamError> {
        let mut result = 0u32;

        for _ in 0..bit_count {
            self.range >>= 1;

            let mask = 0u32.wrapping_sub((self.code.wrapping_sub(self.range)) >> 31);
            self.code = self.code.wrapping_sub(self.range & !mask);
            result = (result << 1) | (!mask & 1);

            if self.range < Self::TOP_VALUE {
                self.normalize()?;
            }
        }

        Ok(result)
    }

    fn normalize(&mut self) -> Result<(), SteamError> {
        if self.position >= self.input.len() {
            return Err(SteamError::Decompression(
                "LZMA stream is truncated".to_string(),
            ));
        }

        self.range <<= 8;
        self.code = (self.code << 8) | self.input[self.position] as u32;
        self.position += 1;
        Ok(())
    }
}
