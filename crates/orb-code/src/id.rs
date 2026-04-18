use sha2::{Digest, Sha256};

use crate::{OrbError, Result};

const ORB_ID_PREFIX: &str = "orb1_";
const SEED_BYTE_COUNT: usize = 16;
const SEED_HEX_LENGTH: usize = SEED_BYTE_COUNT * 2;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrbId(String);

impl OrbId {
    pub fn parse(input: impl Into<String>) -> Result<Self> {
        let value = input.into().to_ascii_lowercase();
        let suffix = value
            .strip_prefix(ORB_ID_PREFIX)
            .ok_or(OrbError::InvalidOrbId)?;
        let is_valid = suffix.len() == SEED_HEX_LENGTH
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if !is_valid {
            return Err(OrbError::InvalidOrbId);
        }
        Ok(Self(value))
    }

    pub fn from_seed_bytes(seed: &[u8; SEED_BYTE_COUNT]) -> Self {
        let mut value = String::with_capacity(ORB_ID_PREFIX.len() + SEED_HEX_LENGTH);
        value.push_str(ORB_ID_PREFIX);
        for &byte in seed {
            value.push(nibble_to_hex(byte >> 4));
            value.push(nibble_to_hex(byte & 0x0f));
        }
        Self(value)
    }

    pub fn seed_bytes(&self) -> [u8; SEED_BYTE_COUNT] {
        let suffix = &self.0[ORB_ID_PREFIX.len()..];
        let mut seed = [0_u8; SEED_BYTE_COUNT];
        for (index, chunk) in suffix.as_bytes().chunks_exact(2).enumerate() {
            let high = hex_to_nibble(chunk[0]);
            let low = hex_to_nibble(chunk[1]);
            seed[index] = (high << 4) | low;
        }
        seed
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OrbId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub fn derive_orb_id(data: &str) -> OrbId {
    let digest = Sha256::digest(data.as_bytes());
    let mut seed = [0_u8; SEED_BYTE_COUNT];
    seed.copy_from_slice(&digest[..SEED_BYTE_COUNT]);
    OrbId::from_seed_bytes(&seed)
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'a' + (value - 10)),
        _ => unreachable!("nibble_to_hex expects a 4-bit value"),
    }
}

fn hex_to_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => unreachable!("OrbId::parse guarantees valid hex"),
    }
}
