use crate::{OrbError, OrbId, Result};

pub const MARKER_VERSION: u8 = 1;
pub const TRACK_COUNT: usize = 3;
pub const SECTOR_COUNT: usize = 56;
pub const SYNC_SECTOR_COUNT: usize = 4;
pub const TOTAL_CELL_COUNT: usize = TRACK_COUNT * SECTOR_COUNT;
pub const VERSION_BIT_COUNT: usize = 4;
pub const FLAG_BIT_COUNT: usize = 8;
pub const SEED_BIT_COUNT: usize = 128;
pub const CRC_BIT_COUNT: usize = 16;
pub const PAYLOAD_BIT_COUNT: usize = (SECTOR_COUNT - SYNC_SECTOR_COUNT) * TRACK_COUNT;
pub const SYNC_BIT_COUNT: usize = SYNC_SECTOR_COUNT * TRACK_COUNT;

const FLAG_VALUE: u8 = 0;
const SYNC_PATTERN: [u8; SYNC_BIT_COUNT] = [1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPayload {
    pub version: u8,
    pub orb_id: OrbId,
}

pub fn encode_marker_cells(orb_id: &OrbId) -> Result<[u8; TOTAL_CELL_COUNT]> {
    let mut payload_bits = Vec::with_capacity(PAYLOAD_BIT_COUNT);
    append_integer_bits(
        &mut payload_bits,
        u128::from(MARKER_VERSION),
        VERSION_BIT_COUNT,
    );
    append_integer_bits(&mut payload_bits, u128::from(FLAG_VALUE), FLAG_BIT_COUNT);
    append_seed_bits(&mut payload_bits, &orb_id.seed_bytes());
    let crc = crc16_bits(&payload_bits);
    append_integer_bits(&mut payload_bits, u128::from(crc), CRC_BIT_COUNT);

    if payload_bits.len() != PAYLOAD_BIT_COUNT {
        return Err(OrbError::PayloadTooLarge);
    }

    let mut cells = [0_u8; TOTAL_CELL_COUNT];
    cells[..SYNC_BIT_COUNT].copy_from_slice(&SYNC_PATTERN);
    cells[SYNC_BIT_COUNT..].copy_from_slice(&payload_bits);
    Ok(cells)
}

pub fn decode_marker_cells(cells: &[u8]) -> Result<DecodedPayload> {
    if cells.len() != TOTAL_CELL_COUNT {
        return Err(OrbError::MalformedPayload);
    }

    let mut candidates = Vec::with_capacity(SECTOR_COUNT * 2);
    for invert in [false, true] {
        for rotation in 0..SECTOR_COUNT {
            candidates.push(RotationCandidate {
                rotation,
                invert,
                sync_distance: rotation_sync_distance(cells, rotation, invert),
            });
        }
    }
    candidates.sort_by_key(|candidate| candidate.sync_distance);

    for candidate in &candidates {
        if candidate.sync_distance > 2 {
            break;
        }
        if let Ok(decoded_payload) = decode_candidate(cells, *candidate) {
            return Ok(decoded_payload);
        }
    }

    let best_distance = candidates
        .into_iter()
        .map(|candidate| candidate.sync_distance)
        .min()
        .unwrap_or(u32::MAX);
    if best_distance == u32::MAX || best_distance > 2 {
        return Err(OrbError::SyncPatternNotFound);
    }
    Err(OrbError::PayloadDecode {
        details: "sync found but payload bits did not validate".to_string(),
    })
}

pub fn sector_track_bit(cells: &[u8; TOTAL_CELL_COUNT], sector: usize, track: usize) -> u8 {
    cells[sector * TRACK_COUNT + track]
}

#[derive(Debug, Clone, Copy)]
struct RotationCandidate {
    rotation: usize,
    invert: bool,
    sync_distance: u32,
}

fn decode_candidate(cells: &[u8], candidate: RotationCandidate) -> Result<DecodedPayload> {
    let payload_bits = rotated_payload_bits(cells, candidate.rotation, candidate.invert);
    let version = bits_to_integer(&payload_bits[..VERSION_BIT_COUNT]) as u8;
    let expected_crc =
        bits_to_integer(&payload_bits[PAYLOAD_BIT_COUNT - CRC_BIT_COUNT..PAYLOAD_BIT_COUNT]) as u16;
    let actual_crc = crc16_bits(&payload_bits[..PAYLOAD_BIT_COUNT - CRC_BIT_COUNT]);
    if actual_crc != expected_crc {
        return Err(OrbError::ChecksumMismatch);
    }
    if version != MARKER_VERSION {
        return Err(OrbError::UnsupportedPayloadVersion { found: version });
    }

    let seed_bits_start = VERSION_BIT_COUNT + FLAG_BIT_COUNT;
    let seed_bits_end = seed_bits_start + SEED_BIT_COUNT;
    let mut seed = [0_u8; 16];
    for (index, chunk) in payload_bits[seed_bits_start..seed_bits_end]
        .chunks_exact(8)
        .enumerate()
    {
        seed[index] = bits_to_integer(chunk) as u8;
    }

    Ok(DecodedPayload {
        version,
        orb_id: OrbId::from_seed_bytes(&seed),
    })
}

fn rotated_payload_bits(cells: &[u8], rotation: usize, invert: bool) -> Vec<u8> {
    let mut bits = Vec::with_capacity(PAYLOAD_BIT_COUNT);
    for sector in SYNC_SECTOR_COUNT..SECTOR_COUNT {
        let rotated_sector = (sector + rotation) % SECTOR_COUNT;
        for track in 0..TRACK_COUNT {
            let mut bit = cells[rotated_sector * TRACK_COUNT + track];
            if invert {
                bit ^= 1;
            }
            bits.push(bit);
        }
    }
    bits
}

fn rotation_sync_distance(cells: &[u8], rotation: usize, invert: bool) -> u32 {
    let mut distance = 0_u32;
    for sector in 0..SYNC_SECTOR_COUNT {
        let rotated_sector = (sector + rotation) % SECTOR_COUNT;
        for track in 0..TRACK_COUNT {
            let mut bit = cells[rotated_sector * TRACK_COUNT + track];
            if invert {
                bit ^= 1;
            }
            let expected = SYNC_PATTERN[sector * TRACK_COUNT + track];
            distance += u32::from(bit != expected);
        }
    }
    distance
}

fn append_seed_bits(bits: &mut Vec<u8>, seed: &[u8; 16]) {
    for &byte in seed {
        append_integer_bits(bits, u128::from(byte), 8);
    }
}

fn append_integer_bits(bits: &mut Vec<u8>, value: u128, bit_count: usize) {
    for shift in (0..bit_count).rev() {
        bits.push(((value >> shift) & 1) as u8);
    }
}

fn bits_to_integer(bits: &[u8]) -> u128 {
    bits.iter()
        .fold(0_u128, |value, bit| (value << 1) | u128::from(*bit & 1))
}

fn crc16_bits(bits: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for &bit in bits {
        let xor = ((crc >> 15) as u8) ^ (bit & 1);
        crc <<= 1;
        if xor != 0 {
            crc ^= 0x1021;
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use crate::{OrbId, derive_orb_id};

    use super::{
        MARKER_VERSION, SYNC_PATTERN, TOTAL_CELL_COUNT, decode_marker_cells, encode_marker_cells,
    };

    #[test]
    fn canonical_orb_id_round_trips_seed_bytes() {
        let orb_id = OrbId::parse("orb1_100680ad546ce6a577f42f52df33b4cf").expect("orb id");
        assert_eq!(
            orb_id.seed_bytes(),
            derive_orb_id("https://example.com").seed_bytes()
        );
    }

    #[test]
    fn marker_cells_encode_and_decode() {
        let orb_id = derive_orb_id("orb-ring");
        let cells = encode_marker_cells(&orb_id).expect("encode cells");
        assert_eq!(cells.len(), TOTAL_CELL_COUNT);
        assert_eq!(&cells[..SYNC_PATTERN.len()], &SYNC_PATTERN);

        let decoded = decode_marker_cells(&cells).expect("decode cells");
        assert_eq!(decoded.version, MARKER_VERSION);
        assert_eq!(decoded.orb_id, orb_id);
    }

    #[test]
    fn marker_cells_decode_after_rotation() {
        let orb_id = derive_orb_id("rotation");
        let cells = encode_marker_cells(&orb_id).expect("encode cells");
        let mut rotated = [0_u8; TOTAL_CELL_COUNT];
        for sector in 0..56 {
            let source_sector = (sector + 11) % 56;
            for track in 0..3 {
                rotated[sector * 3 + track] = cells[source_sector * 3 + track];
            }
        }

        let decoded = decode_marker_cells(&rotated).expect("decode rotated cells");
        assert_eq!(decoded.orb_id, orb_id);
    }
}
