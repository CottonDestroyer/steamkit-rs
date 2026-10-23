//! Adler-32 checksums (RFC 1950, <https://www.rfc-editor.org/rfc/rfc1950.html>).

/// Largest prime smaller than 65536.
const BASE: u32 = 65521;

/// `NMAX` is the largest `n` such that `255n(n+1)/2 + (n+1)(BASE-1) <= 2^32-1`.
const NMAX: usize = 5552;

/// Adler-32 checksums.
#[derive(Debug)]
pub struct Adler32;

impl Adler32 {
    /// The Adler-32 of `buffer`, continuing from `adler`: pass `1` to start a
    /// new checksum, or a previous result to continue one. Uses wrapping
    /// arithmetic.
    pub fn calculate(adler: u32, buffer: &[u8]) -> u32 {
        let mut s1 = adler & 0xFFFF;
        let mut s2 = (adler >> 16) & 0xFFFF;

        let mut buffer = buffer;

        while !buffer.is_empty() {
            // Fold in at most NMAX bytes before reducing modulo BASE, so the
            // sums can't overflow.
            let k = std::cmp::min(buffer.len(), NMAX);
            let mut remaining = k;

            while remaining >= 16 {
                for &byte in &buffer[..16] {
                    s1 = s1.wrapping_add(u32::from(byte));
                    s2 = s2.wrapping_add(s1);
                }

                buffer = &buffer[16..];
                remaining -= 16;
            }

            for &byte in &buffer[..remaining] {
                s1 = s1.wrapping_add(u32::from(byte));
                s2 = s2.wrapping_add(s1);
            }

            buffer = &buffer[remaining..];
            s1 %= BASE;
            s2 %= BASE;
        }

        (s2 << 16) | s1
    }
}
