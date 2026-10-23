//! A small QR code generator, so the samples don't need an extra dependency
//! just to show a QR code.
//!
//! Only the part of the spec that's needed is implemented: byte mode with
//! error correction level `L`, versions 1 to 10. The output follows
//! ISO/IEC 18004 and scans with any QR reader.

/// The properties of a QR code version with the error correction level `L`.
struct VersionInfo {
    /// The total number of codewords of the version.
    total_codewords: usize,

    /// The number of error correction codewords of every block.
    ec_codewords: usize,

    /// The number of blocks of the first group.
    group1_blocks: usize,

    /// The number of data codewords of the blocks of the first group.
    group1_data_codewords: usize,

    /// The number of blocks of the second group.
    group2_blocks: usize,

    /// The number of data codewords of the blocks of the second group.
    group2_data_codewords: usize,
}

/// The properties of the QR code versions 1 up to 10 with the error correction
/// level `L`.
const VERSIONS: [VersionInfo; 10] = [
    VersionInfo {
        total_codewords: 26,
        ec_codewords: 7,
        group1_blocks: 1,
        group1_data_codewords: 19,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 44,
        ec_codewords: 10,
        group1_blocks: 1,
        group1_data_codewords: 34,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 70,
        ec_codewords: 15,
        group1_blocks: 1,
        group1_data_codewords: 55,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 100,
        ec_codewords: 20,
        group1_blocks: 1,
        group1_data_codewords: 80,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 134,
        ec_codewords: 26,
        group1_blocks: 1,
        group1_data_codewords: 108,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 172,
        ec_codewords: 18,
        group1_blocks: 2,
        group1_data_codewords: 68,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 196,
        ec_codewords: 20,
        group1_blocks: 2,
        group1_data_codewords: 78,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 242,
        ec_codewords: 24,
        group1_blocks: 2,
        group1_data_codewords: 97,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 292,
        ec_codewords: 30,
        group1_blocks: 2,
        group1_data_codewords: 116,
        group2_blocks: 0,
        group2_data_codewords: 0,
    },
    VersionInfo {
        total_codewords: 346,
        ec_codewords: 18,
        group1_blocks: 2,
        group1_data_codewords: 68,
        group2_blocks: 2,
        group2_data_codewords: 69,
    },
];

/// The coordinates of the alignment patterns of the QR code versions 1 up to 10.
const ALIGNMENT_POSITIONS: [&[usize]; 10] = [
    &[],
    &[6, 18],
    &[6, 22],
    &[6, 26],
    &[6, 30],
    &[6, 34],
    &[6, 22, 38],
    &[6, 24, 42],
    &[6, 26, 46],
    &[6, 28, 50],
];

/// The number of remainder bits of the QR code versions 1 up to 10.
const REMAINDER_BITS: [usize; 10] = [0, 7, 7, 7, 7, 7, 0, 0, 0, 0];

/// The penalty of a run of five or more modules of the same color.
const PENALTY_N1: u32 = 3;

/// The penalty of every block of 2x2 modules of the same color.
const PENALTY_N2: u32 = 3;

/// The penalty of every finder like pattern.
const PENALTY_N3: u32 = 40;

/// The penalty of every 5% the ratio of the dark modules deviates from 50%.
const PENALTY_N4: u32 = 10;

/// A QR code.
#[derive(Clone, Debug)]
pub struct QrCode {
    /// The width and the height of the code, in modules.
    size: usize,

    /// The modules of the code; `true` means that the module is dark.
    modules: Vec<Vec<bool>>,

    /// The modules that carry a function pattern and are never masked.
    reserved: Vec<Vec<bool>>,
}

impl QrCode {
    /// Encodes the given text into a QR code.
    pub fn encode(text: &str) -> Result<QrCode, String> {
        let payload = text.as_bytes();
        let version = choose_version(payload.len())?;
        let info = &VERSIONS[version - 1];

        let data_codewords = info.group1_blocks * info.group1_data_codewords
            + info.group2_blocks * info.group2_data_codewords;

        let codewords = interleave(&encode_payload(payload, version, data_codewords), info);

        let size = 17 + 4 * version;
        let mut code = QrCode {
            size,
            modules: vec![vec![false; size]; size],
            reserved: vec![vec![false; size]; size],
        };

        code.draw_function_patterns(version);
        code.place_codewords(&codewords, REMAINDER_BITS[version - 1]);

        let mask = code.choose_mask();
        code.apply_mask(mask);
        code.draw_format_information(mask);

        if version >= 7 {
            code.draw_version_information(version);
        }

        Ok(code)
    }

    /// Renders the code as an SVG document.
    ///
    /// The terminal art uses block characters, which a Windows console shows
    /// as `?` because of the OEM code page, so it can't be scanned there. The
    /// SVG can be opened in a browser and scanned off the screen.
    pub fn to_svg(&self) -> String {
        const MODULE: usize = 8;
        const QUIET: usize = 4;

        let side = (self.size + QUIET * 2) * MODULE;
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{side}\" height=\"{side}\" viewBox=\"0 0 {side} {side}\">\n"
        );
        svg.push_str(&format!(
            "  <rect width=\"{side}\" height=\"{side}\" fill=\"#ffffff\"/>\n"
        ));

        for row in 0..self.size {
            for column in 0..self.size {
                if self.modules[row][column] {
                    let x = (column + QUIET) * MODULE;
                    let y = (row + QUIET) * MODULE;
                    svg.push_str(&format!(
                        "  <rect x=\"{x}\" y=\"{y}\" width=\"{MODULE}\" height=\"{MODULE}\" fill=\"#000000\"/>\n"
                    ));
                }
            }
        }

        svg.push_str("</svg>\n");
        svg
    }

    /// Renders the code as text art. Every module is two characters wide so
    /// the code stays square.
    pub fn to_ascii_art(&self) -> String {
        let mut art = String::with_capacity(self.size * (self.size * 2 + 1));

        for row in 0..self.size {
            for column in 0..self.size {
                art.push_str(if self.modules[row][column] {
                    "██"
                } else {
                    "  "
                });
            }
            art.push('\n');
        }

        art
    }

    /// Sets a module that carries a function pattern.
    fn set_function(&mut self, row: usize, column: usize, dark: bool) {
        self.modules[row][column] = dark;
        self.reserved[row][column] = true;
    }

    /// Draws the finder patterns, the separators, the timing patterns, the
    /// alignment patterns and the dark module.
    fn draw_function_patterns(&mut self, version: usize) {
        let size = self.size;

        for (row, column) in [(0usize, 0usize), (0, size - 7), (size - 7, 0)] {
            for dr in 0..7usize {
                for dc in 0..7usize {
                    let dark = dr == 0
                        || dr == 6
                        || dc == 0
                        || dc == 6
                        || (dr >= 2 && dr <= 4 && dc >= 2 && dc <= 4);
                    self.set_function(row + dr, column + dc, dark);
                }
            }
        }

        // the separators that surround the finder patterns
        for index in 0..=7 {
            self.set_function(7, index, false);
            self.set_function(index, 7, false);
            self.set_function(7, size - 1 - index, false);
            self.set_function(index, size - 8, false);
            self.set_function(size - 8, index, false);
            self.set_function(size - 1 - index, 7, false);
        }

        // the timing patterns
        for index in 8..size - 8 {
            let dark = index % 2 == 0;
            self.set_function(6, index, dark);
            self.set_function(index, 6, dark);
        }

        // the alignment patterns
        let positions = ALIGNMENT_POSITIONS[version - 1];
        if positions.len() >= 2 {
            for &row in positions {
                for &column in positions {
                    let overlaps_finder = (row <= 8 && column <= 8)
                        || (row <= 8 && column >= size - 9)
                        || (row >= size - 9 && column <= 8);
                    if overlaps_finder {
                        continue;
                    }

                    for dr in 0..5usize {
                        for dc in 0..5usize {
                            let distance = dr.abs_diff(2).max(dc.abs_diff(2));
                            self.set_function(row - 2 + dr, column - 2 + dc, distance != 1);
                        }
                    }
                }
            }
        }

        // the dark module
        self.set_function(size - 8, 8, true);

        // the areas that carry the format information
        for index in 0..=5 {
            self.set_function(index, 8, false);
        }
        self.set_function(7, 8, false);
        self.set_function(8, 8, false);
        self.set_function(8, 7, false);
        for index in 9..15 {
            self.set_function(8, 14 - index, false);
        }
        for index in 0..8 {
            self.set_function(8, size - 1 - index, false);
        }
        for index in 8..15 {
            self.set_function(size - 15 + index, 8, false);
        }

        // the areas that carry the version information
        if version >= 7 {
            for index in 0..18 {
                let a = size - 11 + index % 3;
                let b = index / 3;

                self.set_function(b, a, false);
                self.set_function(a, b, false);
            }
        }
    }

    /// Places the codewords into the free modules of the code.
    fn place_codewords(&mut self, codewords: &[u8], remainder_bits: usize) {
        let mut bits: Vec<bool> = Vec::with_capacity(codewords.len() * 8 + remainder_bits);

        for byte in codewords {
            for shift in (0..8).rev() {
                bits.push((byte >> shift) & 1 == 1);
            }
        }
        bits.extend(std::iter::repeat(false).take(remainder_bits));

        let size = self.size;
        let mut index = 0usize;
        let mut right = size as isize - 1;

        while right >= 1 {
            if right == 6 {
                right = 5;
            }

            for vertical in 0..size {
                for offset in 0..2isize {
                    let column = (right - offset) as usize;
                    let upward = ((right + 1) & 2) == 0;
                    let row = if upward { size - 1 - vertical } else { vertical };

                    if !self.reserved[row][column] {
                        self.modules[row][column] = bits.get(index).copied().unwrap_or(false);
                        index += 1;
                    }
                }
            }

            right -= 2;
        }
    }

    /// Applies the given mask to the modules that carry data.
    fn apply_mask(&mut self, mask: u32) {
        for row in 0..self.size {
            for column in 0..self.size {
                if self.reserved[row][column] {
                    continue;
                }

                if mask_applies(mask, row, column) {
                    self.modules[row][column] = !self.modules[row][column];
                }
            }
        }
    }

    /// Chooses the mask that produces the code with the lowest penalty.
    fn choose_mask(&self) -> u32 {
        let mut best_mask = 0;
        let mut best_penalty = u32::MAX;

        for mask in 0..8u32 {
            let mut candidate = self.clone();
            candidate.apply_mask(mask);
            candidate.draw_format_information(mask);
            let penalty = candidate.penalty();

            if penalty < best_penalty {
                best_penalty = penalty;
                best_mask = mask;
            }
        }

        best_mask
    }

    /// Writes the format information of the code.
    fn draw_format_information(&mut self, mask: u32) {
        let bits = u32::from(format_information(mask));
        let size = self.size;

        for index in 0..=5 {
            self.set_function(index, 8, bit_of(bits, index));
        }
        self.set_function(7, 8, bit_of(bits, 6));
        self.set_function(8, 8, bit_of(bits, 7));
        self.set_function(8, 7, bit_of(bits, 8));
        for index in 9..15 {
            self.set_function(8, 14 - index, bit_of(bits, index));
        }

        for index in 0..8 {
            self.set_function(8, size - 1 - index, bit_of(bits, index));
        }
        for index in 8..15 {
            self.set_function(size - 15 + index, 8, bit_of(bits, index));
        }
    }

    /// Writes the version information of the code.
    fn draw_version_information(&mut self, version: usize) {
        let bits = version_information(version as u32);
        let size = self.size;

        for index in 0..18 {
            let dark = bit_of(bits, index);
            let a = size - 11 + index % 3;
            let b = index / 3;

            self.set_function(b, a, dark);
            self.set_function(a, b, dark);
        }
    }

    /// The penalty of the code; the lower the better.
    fn penalty(&self) -> u32 {
        let mut penalty = 0;

        for row in 0..self.size {
            penalty += self.penalty_of_line(&self.modules[row]);
        }

        for column in 0..self.size {
            let line: Vec<bool> = (0..self.size).map(|row| self.modules[row][column]).collect();
            penalty += self.penalty_of_line(&line);
        }

        for row in 0..self.size - 1 {
            for column in 0..self.size - 1 {
                let color = self.modules[row][column];
                if color == self.modules[row][column + 1]
                    && color == self.modules[row + 1][column]
                    && color == self.modules[row + 1][column + 1]
                {
                    penalty += PENALTY_N2;
                }
            }
        }

        let mut dark = 0usize;
        for row in 0..self.size {
            for column in 0..self.size {
                if self.modules[row][column] {
                    dark += 1;
                }
            }
        }

        let total = self.size * self.size;
        let percent = dark * 100 / total;
        let deviation = percent.abs_diff(50) / 5;
        penalty += PENALTY_N4 * deviation as u32;

        penalty
    }

    /// The penalty of a single row or column.
    fn penalty_of_line(&self, line: &[bool]) -> u32 {
        let mut penalty = 0;

        let mut run = 1usize;
        for index in 1..line.len() {
            if line[index] == line[index - 1] {
                run += 1;
            } else {
                if run >= 5 {
                    penalty += PENALTY_N1 + (run - 5) as u32;
                }
                run = 1;
            }
        }
        if run >= 5 {
            penalty += PENALTY_N1 + (run - 5) as u32;
        }

        for index in 0..line.len().saturating_sub(10) {
            let window = &line[index..index + 11];
            let pattern_a = [
                true, false, true, true, true, false, true, false, false, false, false,
            ];
            let pattern_b = [
                false, false, false, false, true, false, true, true, true, false, true,
            ];

            if window == pattern_a || window == pattern_b {
                penalty += PENALTY_N3;
            }
        }

        penalty
    }
}

/// Picks the smallest version that can hold the given number of bytes.
fn choose_version(length: usize) -> Result<usize, String> {
    for version in 1..=VERSIONS.len() {
        let info = &VERSIONS[version - 1];
        let capacity = info.group1_blocks * info.group1_data_codewords
            + info.group2_blocks * info.group2_data_codewords;
        let header_bits = if version <= 9 { 12 } else { 20 };

        if length * 8 + header_bits <= capacity * 8 {
            return Ok(version);
        }
    }

    Err(format!(
        "the data is too long for a QR code ({length} bytes, at most {} bytes are supported)",
        VERSIONS[VERSIONS.len() - 1]
            .group1_blocks
            .wrapping_mul(VERSIONS[VERSIONS.len() - 1].group1_data_codewords)
            .wrapping_add(
                VERSIONS[VERSIONS.len() - 1]
                    .group2_blocks
                    .wrapping_mul(VERSIONS[VERSIONS.len() - 1].group2_data_codewords)
            )
            .saturating_sub(3)
    ))
}

/// Encodes the payload into the data codewords of the code.
fn encode_payload(payload: &[u8], version: usize, capacity: usize) -> Vec<u8> {
    let mut bits: Vec<u8> = Vec::with_capacity(capacity * 8);

    // the byte mode indicator
    bits.extend_from_slice(&[0, 1, 0, 0]);

    // the character count indicator
    let count = payload.len() as u16;
    let count_bits = if version <= 9 { 8 } else { 16 };
    for shift in (0..count_bits).rev() {
        bits.push(((count >> shift) & 1) as u8);
    }

    // the payload itself
    for byte in payload {
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }

    // the terminator
    let terminator = 4usize.min(capacity * 8 - bits.len());
    bits.extend(std::iter::repeat(0).take(terminator));

    // the padding to a byte boundary
    while bits.len() % 8 != 0 {
        bits.push(0);
    }

    // the padding bytes
    let mut padding = 0xECu8;
    while bits.len() / 8 < capacity {
        for shift in (0..8).rev() {
            bits.push((padding >> shift) & 1);
        }
        padding = if padding == 0xEC { 0x11 } else { 0xEC };
    }

    let mut codewords = Vec::with_capacity(capacity);
    for chunk in bits.chunks(8) {
        let mut byte = 0u8;
        for (index, bit) in chunk.iter().enumerate() {
            byte |= bit << (7 - index);
        }
        codewords.push(byte);
    }

    codewords
}

/// Splits the data codewords into blocks, computes their error correction
/// codewords and interleaves everything.
fn interleave(codewords: &[u8], info: &VersionInfo) -> Vec<u8> {
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(info.group1_blocks + info.group2_blocks);
    let mut offset = 0usize;

    for block in 0..info.group1_blocks + info.group2_blocks {
        let length = if block < info.group1_blocks {
            info.group1_data_codewords
        } else {
            info.group2_data_codewords
        };

        blocks.push(codewords[offset..offset + length].to_vec());
        offset += length;
    }

    let ec_blocks: Vec<Vec<u8>> = blocks
        .iter()
        .map(|block| error_correction(block, info.ec_codewords))
        .collect();

    let mut result = Vec::with_capacity(info.total_codewords);
    let longest = blocks.iter().map(|block| block.len()).max().unwrap_or(0);

    for index in 0..longest {
        for block in &blocks {
            if index < block.len() {
                result.push(block[index]);
            }
        }
    }

    for index in 0..info.ec_codewords {
        for block in &ec_blocks {
            result.push(block[index]);
        }
    }

    result
}

/// Computes the Reed-Solomon error correction codewords of a block.
fn error_correction(data: &[u8], length: usize) -> Vec<u8> {
    let generator = generator_polynomial(length);
    let mut remainder = vec![0u8; data.len() + length];
    remainder[..data.len()].copy_from_slice(data);

    for index in 0..data.len() {
        let factor = remainder[index];
        if factor == 0 {
            continue;
        }

        for (offset, coefficient) in generator.iter().enumerate().skip(1) {
            remainder[index + offset] ^= field_mul(factor, *coefficient);
        }
    }

    remainder[data.len()..].to_vec()
}

/// Builds the generator polynomial of the Reed-Solomon code.
fn generator_polynomial(length: usize) -> Vec<u8> {
    let mut polynomial = vec![1u8];
    let mut root = 1u8;

    for _ in 0..length {
        let mut next = vec![0u8; polynomial.len() + 1];

        for (index, coefficient) in polynomial.iter().enumerate() {
            next[index] ^= *coefficient;
            next[index + 1] ^= field_mul(*coefficient, root);
        }

        polynomial = next;
        root = field_mul(root, 2);
    }

    polynomial
}

/// Multiplies two elements of the Galois field GF(256).
fn field_mul(mut left: u8, mut right: u8) -> u8 {
    let mut result = 0u8;

    while right != 0 {
        if right & 1 != 0 {
            result ^= left;
        }
        right >>= 1;

        let carry = left & 0x80;
        left <<= 1;
        if carry != 0 {
            left ^= 0x1D;
        }
    }

    result
}

/// Whether the given mask flips the module at the given position.
fn mask_applies(mask: u32, row: usize, column: usize) -> bool {
    let row = row as u32;
    let column = column as u32;

    match mask {
        0 => (row + column) % 2 == 0,
        1 => row % 2 == 0,
        2 => column % 3 == 0,
        3 => (row + column) % 3 == 0,
        4 => (row / 2 + column / 3) % 2 == 0,
        5 => (row * column) % 2 + (row * column) % 3 == 0,
        6 => ((row * column) % 2 + (row * column) % 3) % 2 == 0,
        7 => ((row + column) % 2 + (row * column) % 3) % 2 == 0,
        _ => false,
    }
}

/// Builds the 15 bits of the format information.
fn format_information(mask: u32) -> u16 {
    // the error correction level `L` is encoded as `01`
    let data = (1u16 << 3) | mask as u16;
    let mut remainder = data;

    for _ in 0..10 {
        remainder = (remainder << 1) ^ ((remainder >> 9) * 0x537);
    }

    ((data << 10) | remainder) ^ 0x5412
}

/// Builds the 18 bits of the version information.
fn version_information(version: u32) -> u32 {
    let mut remainder = version;

    for _ in 0..12 {
        remainder = (remainder << 1) ^ ((remainder >> 11) * 0x1F25);
    }

    (version << 12) | remainder
}

/// Reads a single bit of a bit field.
fn bit_of(bits: u32, index: usize) -> bool {
    (bits >> index) & 1 == 1
}
