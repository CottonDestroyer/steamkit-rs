//! Adler-32 tests.
//!
//! The library has a single scalar implementation, so the tests meant for
//! SIMD paths compare it against the simple [`reference_scalar`] below.

use rand::{RngCore, SeedableRng};
use steamkit::util::adler32::Adler32;

/// The largest prime smaller than 65536.
const BASE: u32 = 65521;

/// Reference Adler-32 that seeds `a` with the whole seed and `b` with zero.
/// That matches the library for the seeds used here (`0` and `1`).
fn reference_implementation(seed: u32, input: &[u8]) -> u32 {
    let mut a = seed;
    let mut b = 0u32;

    for &byte in input {
        a = (a + u32::from(byte)) % BASE;
        b = (b + a) % BASE;
    }

    a | (b << 16)
}

/// A straightforward, byte at a time Adler-32, used as the baseline.
fn reference_scalar(seed: u32, input: &[u8]) -> u32 {
    let mut s1 = seed & 0xFFFF;
    let mut s2 = (seed >> 16) & 0xFFFF;

    for &byte in input {
        s1 = (s1 + u32::from(byte)) % BASE;
        s2 = (s2 + s1) % BASE;
    }

    (s2 << 16) | s1
}

/// Checks the library against [`reference_scalar`].
fn assert_all_paths_match_scalar(seed: u32, data: &[u8]) {
    let expected = reference_scalar(seed, data);
    assert_eq!(expected, Adler32::calculate(seed, data));
}

/// Lengths around the SIMD block sizes and the 5552 byte overflow limit.
const VECTORIZED_LENGTHS: &[usize] = &[
    0,
    1,
    15,
    16,
    17,
    31,
    32,
    33,
    63,
    64,
    65,
    100,
    5552 - 32 - 1,
    5552 - 32,
    5552 - 16,
    5552 - 1,
    5552,
    5552 + 1,
    5552 + 31,
    5552 + 32,
    173 * 32 - 1,
    173 * 32,
    173 * 32 + 1,
    173 * 32 + 31,
    2 * 173 * 32 - 1,
    2 * 173 * 32,
    2 * 173 * 32 + 1,
    65536,
    1024 * 1024,
    1024 * 1024 + 7,
];

#[test]
fn returns_correct_when_empty_0() {
    assert_eq!(0u32, Adler32::calculate(0, &[]));
}

#[test]
fn returns_correct_when_empty_1() {
    assert_eq!(1u32, Adler32::calculate(1, &[]));
}

#[test]
fn returns_correct_when_empty_2() {
    assert_eq!(2u32, Adler32::calculate(2, &[]));
}

#[test]
fn matches_reference_0() {
    matches_reference(0);
}

#[test]
fn matches_reference_8() {
    matches_reference(8);
}

#[test]
fn matches_reference_15() {
    matches_reference(15);
}

#[test]
fn matches_reference_16() {
    matches_reference(16);
}

#[test]
fn matches_reference_17() {
    matches_reference(17);
}

#[test]
fn matches_reference_215() {
    matches_reference(215);
}

#[test]
fn matches_reference_1024() {
    matches_reference(1024);
}

#[test]
fn matches_reference_1024_plus_15() {
    matches_reference(1024 + 15);
}

#[test]
fn matches_reference_2034() {
    matches_reference(2034);
}

#[test]
fn matches_reference_4096() {
    matches_reference(4096);
}

#[test]
fn matches_reference_5552_minus_1() {
    matches_reference(5552 - 1);
}

#[test]
fn matches_reference_5552() {
    matches_reference(5552);
}

#[test]
fn matches_reference_5552_plus_1() {
    matches_reference(5552 + 1);
}

#[test]
fn matches_reference_5552_plus_16() {
    matches_reference(5552 + 16);
}

#[test]
fn matches_reference_5552_times_2() {
    matches_reference(5552 * 2);
}

/// Compares the library with [`reference_implementation`] on random data.
fn matches_reference(length: usize) {
    const SEED: u32 = 0;

    let mut data = vec![0u8; length];
    rand::thread_rng().fill_bytes(&mut data);

    let expected = reference_implementation(SEED, &data);
    let actual = Adler32::calculate(SEED, &data);

    assert_eq!(expected, actual);
}

#[test]
fn matches_known_vectors_a() {
    matches_known_vectors(1, "a", 0x0062_0062);
}

#[test]
fn matches_known_vectors_wikipedia() {
    matches_known_vectors(1, "Wikipedia", 0x11E6_0398);
}

#[test]
fn matches_known_vectors_digits() {
    matches_known_vectors(1, "123456789", 0x091E_01DE);
}

#[test]
fn matches_known_vectors_sentence() {
    matches_known_vectors(
        1,
        "SteamKit is good software, you should use it. :)",
        0xAF81_10E6,
    );
}

/// Checks one known Adler-32 value.
fn matches_known_vectors(seed: u32, input: &str, expected: u32) {
    let data = input.as_bytes();
    let actual = Adler32::calculate(seed, data);
    assert_eq!(expected, actual);
}

#[test]
fn no_overflow_with_max_bytes() {
    let data = vec![0xFFu8; 5552];

    let expected = reference_implementation(1, &data);
    let actual = Adler32::calculate(1, &data);
    assert_eq!(expected, actual);
}

#[test]
fn vectorized_matches_scalar() {
    for &length in VECTORIZED_LENGTHS {
        let mut random = rand::rngs::StdRng::seed_from_u64(length as u64);
        let mut data = vec![0u8; length];
        rand::RngCore::fill_bytes(&mut random, &mut data);

        let seed = ((rand::Rng::gen_range(&mut random, 0..65521u32) << 16)
            | rand::Rng::gen_range(&mut random, 0..65521u32))
            & 0xFFFF_FFFF;

        assert_all_paths_match_scalar(seed, &data);
        assert_all_paths_match_scalar(0, &data);
        assert_all_paths_match_scalar(1, &data);
    }
}

#[test]
fn vectorized_matches_scalar_on_random_slices() {
    let mut random = rand::rngs::StdRng::seed_from_u64(1950);
    let mut data = vec![0u8; 20000];
    rand::RngCore::fill_bytes(&mut random, &mut data);

    for _ in 0..500 {
        let start = rand::Rng::gen_range(&mut random, 0..64usize);
        let length = rand::Rng::gen_range(&mut random, 0..(data.len() - start));
        let seed = ((rand::Rng::gen_range(&mut random, 0..65521u32) << 16)
            | rand::Rng::gen_range(&mut random, 0..65521u32))
            & 0xFFFF_FFFF;

        assert_all_paths_match_scalar(seed, &data[start..start + length]);
    }
}

#[test]
fn vectorized_no_overflow_with_max_bytes_fff0fff0() {
    vectorized_no_overflow_with_max_bytes(0xFFF0_FFF0);
}

#[test]
fn vectorized_no_overflow_with_max_bytes_ffffffff() {
    vectorized_no_overflow_with_max_bytes(0xFFFF_FFFF);
}

/// All-0xFF input long enough to overflow a naive accumulator.
fn vectorized_no_overflow_with_max_bytes(seed: u32) {
    let data = vec![0xFFu8; 5552 * 4 + 31];

    assert_all_paths_match_scalar(seed, &data);
    assert_eq!(
        reference_implementation(1, &data),
        Adler32::calculate(1, &data)
    );
}
