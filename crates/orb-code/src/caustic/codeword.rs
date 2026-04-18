use sha2::{Digest, Sha256};

use crate::caustic::types::InternalCodeword;

const CHECKSUM_BITS: usize = 16;
const PARITY_SOURCE_BITS: usize = PUBLIC_ID_BITS + CHECKSUM_BITS;
const PARITY_BITS: usize = INTERNAL_CODEWORD_BITS - PARITY_SOURCE_BITS;
const PARITY_TAPS_PER_ROW: usize = 17;
const PARITY_SEED: u64 = 0x6361_7573_7469_632d;
const SPLITMIX_INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MULTIPLIER_A: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MULTIPLIER_B: u64 = 0x94D0_49BB_1331_11EB;

pub const PUBLIC_ID_BITS: usize = 64;
pub const INTERNAL_CODEWORD_BITS: usize = 128;

pub fn encode_internal_codeword(public_id: u64) -> InternalCodeword {
    let mut bits = Vec::with_capacity(INTERNAL_CODEWORD_BITS);
    bits.extend(u64_to_bits(public_id));
    bits.extend(checksum_bits(public_id));
    let protected_prefix = bits.clone();
    bits.extend(parity_bits(&protected_prefix));

    InternalCodeword { public_id, bits }
}

pub fn internal_codeword_signs(codeword: &InternalCodeword) -> Vec<f64> {
    codeword
        .bits
        .iter()
        .map(|bit| if *bit { 1.0 } else { -1.0 })
        .collect()
}

pub fn public_id_from_signs(signs: &[f64]) -> u64 {
    let mut public_id = 0_u64;
    for value in signs.iter().take(PUBLIC_ID_BITS) {
        public_id <<= 1;
        if *value >= 0.0 {
            public_id |= 1;
        }
    }

    public_id
}

fn u64_to_bits(value: u64) -> impl Iterator<Item = bool> {
    (0..PUBLIC_ID_BITS)
        .rev()
        .map(move |shift| ((value >> shift) & 1) == 1)
}

fn checksum_bits(public_id: u64) -> impl Iterator<Item = bool> {
    let digest = Sha256::digest(public_id.to_be_bytes());
    (0..CHECKSUM_BITS).map(move |index| {
        let byte = digest[index / 8];
        let bit_index = 7 - (index % 8);
        ((byte >> bit_index) & 1) == 1
    })
}

fn parity_bits(source_bits: &[bool]) -> impl Iterator<Item = bool> + '_ {
    (0..PARITY_BITS).map(|row_index| {
        let mut state = PARITY_SEED ^ ((row_index as u64 + 1).wrapping_mul(SPLITMIX_INCREMENT));
        let mut parity = false;

        for tap_index in 0..PARITY_TAPS_PER_ROW {
            let mixed = splitmix64(state ^ tap_index as u64);
            let source_index = ((mixed as usize) + row_index + tap_index) % PARITY_SOURCE_BITS;
            parity ^= source_bits[source_index];
            state = mixed;
        }

        parity ^= source_bits[row_index % PARITY_SOURCE_BITS];
        parity
    })
}

fn splitmix64(mut state: u64) -> u64 {
    state = state.wrapping_add(SPLITMIX_INCREMENT);
    state = (state ^ (state >> 30)).wrapping_mul(SPLITMIX_MULTIPLIER_A);
    state = (state ^ (state >> 27)).wrapping_mul(SPLITMIX_MULTIPLIER_B);
    state ^ (state >> 31)
}
