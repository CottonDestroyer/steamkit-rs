//! Shared test helpers. Each file under `tests/` is its own crate, so shared
//! code lives in this `common` directory and is pulled in with `mod common;`.

use std::path::PathBuf;
use std::sync::Arc;

use steamkit::steam::steam_client::steam_client::SteamClient;

/// The directory that holds the binary test assets.
pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// The full path of a test asset.
pub fn data_file(name: &str) -> PathBuf {
    data_dir().join(name)
}

/// Reads a test asset into memory.
pub fn read_data_file(name: &str) -> Vec<u8> {
    let path = data_file(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read test data file {}: {error}", path.display()))
}

/// A client for tests.
///
/// Sending fails without a connection (`SteamError::Connection`), but the job
/// manager, callback queue and handler registry all work without one.
pub fn connected_steam_client() -> Arc<SteamClient> {
    SteamClient::new()
}

/// Same as [`connected_steam_client`], with a custom identifier.
pub fn connected_steam_client_with_identifier(identifier: &str) -> Arc<SteamClient> {
    SteamClient::with_identifier(identifier)
}

/// SHA-1 (FIPS 180-4).
///
/// Computed here so the test crate doesn't need a `sha1` dependency.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for (index, word) in w.iter_mut().enumerate().take(16) {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                block[offset],
                block[offset + 1],
                block[offset + 2],
                block[offset + 3],
            ]);
        }
        for index in 16..80 {
            w[index] = (w[index - 3] ^ w[index - 8] ^ w[index - 14] ^ w[index - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);

        for (index, word) in w.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);

            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut digest = [0u8; 20];
    for (index, word) in h.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// SHA-256 (FIPS 180-4).
///
/// Computed here so the test crate doesn't need a `sha2` dependency.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in w.iter_mut().enumerate().take(16) {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                block[offset],
                block[offset + 1],
                block[offset + 2],
                block[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut v = h;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let temp1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let temp2 = s0.wrapping_add(maj);

            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(temp1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = temp1.wrapping_add(temp2);
        }

        for (index, value) in v.iter().enumerate() {
            h[index] = h[index].wrapping_add(*value);
        }
    }

    let mut digest = [0u8; 32];
    for (index, word) in h.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// AES-256 encryption, used to build the input of `CryptoHelper::symmetric_decrypt`.
///
/// The `aes`, `cbc` and `ecb` crates are dependencies of the library but not of
/// the test crate, so a minimal AES-256 encryptor lives here.
pub struct Aes256 {
    round_keys: [[u8; 16]; 15],
}

const fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut product = 0u8;
    let mut round = 0;
    while round < 8 {
        if b & 1 != 0 {
            product ^= a;
        }
        let high = a & 0x80;
        a <<= 1;
        if high != 0 {
            a ^= 0x1B;
        }
        b >>= 1;
        round += 1;
    }
    product
}

const fn build_sbox() -> [u8; 256] {
    let mut sbox = [0u8; 256];
    let mut index = 0usize;
    while index < 256 {
        let mut inverse = 0u8;
        if index != 0 {
            let mut candidate = 1usize;
            while candidate < 256 {
                if gf_mul(index as u8, candidate as u8) == 1 {
                    inverse = candidate as u8;
                    break;
                }
                candidate += 1;
            }
        }

        sbox[index] = inverse
            ^ inverse.rotate_left(1)
            ^ inverse.rotate_left(2)
            ^ inverse.rotate_left(3)
            ^ inverse.rotate_left(4)
            ^ 0x63;

        index += 1;
    }
    sbox
}

const SBOX: [u8; 256] = build_sbox();

const fn xtime(value: u8) -> u8 {
    let high = value & 0x80;
    let shifted = value << 1;
    if high != 0 {
        shifted ^ 0x1B
    } else {
        shifted
    }
}

impl Aes256 {
    /// Expands a 32 byte key into the 15 round keys of AES-256.
    pub fn new(key: &[u8; 32]) -> Self {
        const NK: usize = 8;
        const NR: usize = 14;

        let mut words = [[0u8; 4]; 4 * (NR + 1)];

        for (index, word) in words.iter_mut().enumerate().take(NK) {
            word.copy_from_slice(&key[index * 4..index * 4 + 4]);
        }

        let mut rcon = 1u8;

        for index in NK..4 * (NR + 1) {
            let mut temp = words[index - 1];

            if index % NK == 0 {
                temp = [
                    SBOX[temp[1] as usize] ^ rcon,
                    SBOX[temp[2] as usize],
                    SBOX[temp[3] as usize],
                    SBOX[temp[0] as usize],
                ];
                rcon = xtime(rcon);
            } else if index % NK == 4 {
                temp = [
                    SBOX[temp[0] as usize],
                    SBOX[temp[1] as usize],
                    SBOX[temp[2] as usize],
                    SBOX[temp[3] as usize],
                ];
            }

            for byte in 0..4 {
                words[index][byte] = words[index - NK][byte] ^ temp[byte];
            }
        }

        let mut round_keys = [[0u8; 16]; NR + 1];
        for (round, round_key) in round_keys.iter_mut().enumerate() {
            for column in 0..4 {
                round_key[column * 4..column * 4 + 4].copy_from_slice(&words[round * 4 + column]);
            }
        }

        Aes256 { round_keys }
    }

    /// Encrypts a single 16 byte block.
    pub fn encrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut state = *block;

        add_round_key(&mut state, &self.round_keys[0]);

        for round in 1..14 {
            for byte in state.iter_mut() {
                *byte = SBOX[*byte as usize];
            }
            shift_rows(&mut state);
            mix_columns(&mut state);
            add_round_key(&mut state, &self.round_keys[round]);
        }

        for byte in state.iter_mut() {
            *byte = SBOX[*byte as usize];
        }
        shift_rows(&mut state);
        add_round_key(&mut state, &self.round_keys[14]);

        state
    }

    /// Encrypts with AES-256 in ECB mode, without padding.
    pub fn encrypt_ecb(&self, data: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len());
        for block in data.chunks(16) {
            let mut buffer = [0u8; 16];
            buffer.copy_from_slice(block);
            output.extend_from_slice(&self.encrypt_block(&buffer));
        }
        output
    }

    /// Encrypts with AES-256 in CBC mode using PKCS#7 padding.
    pub fn encrypt_cbc_pkcs7(&self, data: &[u8], iv: &[u8; 16]) -> Vec<u8> {
        let padding = 16 - (data.len() % 16);
        let mut buffer = data.to_vec();
        buffer.extend(std::iter::repeat(padding as u8).take(padding));

        let mut previous = *iv;
        let mut output = Vec::with_capacity(buffer.len());

        for block in buffer.chunks(16) {
            let mut chunk = [0u8; 16];
            chunk.copy_from_slice(block);
            for (index, byte) in chunk.iter_mut().enumerate() {
                *byte ^= previous[index];
            }
            previous = self.encrypt_block(&chunk);
            output.extend_from_slice(&previous);
        }

        output
    }
}

fn add_round_key(state: &mut [u8; 16], round_key: &[u8; 16]) {
    for (byte, key_byte) in state.iter_mut().zip(round_key.iter()) {
        *byte ^= key_byte;
    }
}

fn shift_rows(state: &mut [u8; 16]) {
    let original = *state;

    for row in 1..4 {
        for column in 0..4 {
            state[column * 4 + row] = original[((column + row) % 4) * 4 + row];
        }
    }
}

fn mix_columns(state: &mut [u8; 16]) {
    for column in 0..4 {
        let offset = column * 4;
        let a0 = state[offset];
        let a1 = state[offset + 1];
        let a2 = state[offset + 2];
        let a3 = state[offset + 3];

        state[offset] = xtime(a0) ^ (xtime(a1) ^ a1) ^ a2 ^ a3;
        state[offset + 1] = a0 ^ xtime(a1) ^ (xtime(a2) ^ a2) ^ a3;
        state[offset + 2] = a0 ^ a1 ^ xtime(a2) ^ (xtime(a3) ^ a3);
        state[offset + 3] = (xtime(a0) ^ a0) ^ a1 ^ a2 ^ xtime(a3);
    }
}
