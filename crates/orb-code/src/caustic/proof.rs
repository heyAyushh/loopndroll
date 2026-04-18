use std::{collections::HashSet, sync::OnceLock};

use nalgebra::{DVector, SymmetricEigen};

use crate::caustic::{
    channel::{ChannelSnapshot, build_channel_snapshot, fit_affine_to_reference, normalize_image},
    codeword::{
        INTERNAL_CODEWORD_BITS, encode_internal_codeword, internal_codeword_signs,
        public_id_from_signs,
    },
    frame::{WaveletFrame, build_payload_frame},
    linear::{gram_matrix, max_abs_inner_product, spectral_bounds},
    parameters::CausticProofParameters,
    types::{ProofReport, StructuralCertificate},
};

const EXPOSURE_BASE: f64 = 0.97;
const EXPOSURE_JITTER: f64 = 0.06;
const BIAS_AMPLITUDE: f64 = 0.005;
const ORIENTATION_LEAK_BASE: f64 = 0.004;
const ORIENTATION_LEAK_JITTER: f64 = 0.003;
const NOISE_STD_BASE: f64 = 0.0025;
const NOISE_STD_JITTER: f64 = 0.0015;
const ID_SEED: u64 = 0x4f52_422d_5052_4f56;
const CAMERA_SEED: u64 = 0x4341_4d2d_4d4f_4445;
const SPLITMIX_INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MULTIPLIER_A: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MULTIPLIER_B: u64 = 0x94D0_49BB_1331_11EB;

#[derive(Debug, Clone)]
pub struct CausticProofModel {
    frame: WaveletFrame,
    channel_snapshots: Vec<ChannelSnapshot>,
}

impl Default for CausticProofModel {
    fn default() -> Self {
        static DEFAULT_MODEL: OnceLock<CausticProofModel> = OnceLock::new();
        DEFAULT_MODEL
            .get_or_init(|| CausticProofModel::new(CausticProofParameters::default()))
            .clone()
    }
}

impl CausticProofModel {
    pub fn new(parameters: CausticProofParameters) -> Self {
        let frame = build_payload_frame(&parameters);
        let channel_snapshots = (0..parameters.max_yaw_steps)
            .map(|yaw_index| build_channel_snapshot(&frame, &parameters, yaw_index))
            .collect();

        Self {
            frame,
            channel_snapshots,
        }
    }

    pub fn structural_certificate(&self) -> StructuralCertificate {
        let payload_gram = gram_matrix(&self.frame.payload_matrix);
        let (frame_lower_bound, frame_upper_bound) = spectral_bounds(&payload_gram);
        let max_orientation_payload_inner_product =
            max_abs_inner_product(&self.frame.orientation_matrix, &self.frame.payload_matrix);
        let coefficient_margin = 2.0 * self.frame.payload_code_scale * frame_lower_bound.sqrt();
        let image_operator_lower_bound = self
            .channel_snapshots
            .iter()
            .map(|snapshot| {
                let gram = snapshot.bit_image_matrix.transpose() * &snapshot.bit_image_matrix;
                let eigen = SymmetricEigen::new(gram);
                eigen
                    .eigenvalues
                    .iter()
                    .fold(f64::INFINITY, |current, value| current.min(*value))
                    .max(0.0)
                    .sqrt()
            })
            .fold(f64::INFINITY, f64::min);

        StructuralCertificate {
            max_orientation_payload_inner_product,
            frame_lower_bound,
            frame_upper_bound,
            coefficient_margin,
            image_operator_lower_bound,
        }
    }
}

pub fn generate_proof_report(parameters: &CausticProofParameters) -> ProofReport {
    let model = if parameters == &CausticProofParameters::default() {
        CausticProofModel::default()
    } else {
        CausticProofModel::new(parameters.clone())
    };
    let structural_certificate = model.structural_certificate();
    let mut shortlist_hits = 0_usize;
    let mut stage_two_hits = 0_usize;
    let mut successful_margins = Vec::new();
    let mut residual_energies = Vec::new();

    for sample_index in 0..parameters.monte_carlo_samples {
        let public_id = sample_public_id(sample_index);
        let codeword = encode_internal_codeword(public_id);
        let true_signs = DVector::from_vec(internal_codeword_signs(&codeword));
        let yaw_index = sample_yaw_index(sample_index, parameters.max_yaw_steps);
        let true_snapshot = &model.channel_snapshots[yaw_index];
        let observation = synthesize_observation(true_snapshot, &true_signs, sample_index);

        let (estimated_yaw_index, logits) =
            estimate_snapshot_and_logits(&model.channel_snapshots, &observation);
        let estimated_snapshot = &model.channel_snapshots[estimated_yaw_index];
        let shortlist = build_shortlist(
            &logits,
            parameters.shortlist_size,
            parameters.low_confidence_bits,
        );

        let shortlist_contains_truth = shortlist.contains(&public_id);
        shortlist_hits += usize::from(shortlist_contains_truth);

        let candidate_scores =
            score_shortlist_candidates(estimated_snapshot, &observation, &shortlist);
        let winning_candidate = candidate_scores[0].0;
        let winning_truth = winning_candidate == public_id;
        stage_two_hits += usize::from(winning_truth);
        residual_energies.push(candidate_scores[0].1);

        if winning_truth && candidate_scores.len() > 1 {
            successful_margins.push(candidate_scores[1].1 - candidate_scores[0].1);
        }
    }

    let shortlist_true_hit_rate = shortlist_hits as f64 / parameters.monte_carlo_samples as f64;
    let stage_two_success_rate = stage_two_hits as f64 / parameters.monte_carlo_samples as f64;
    let residual_variance = residual_energies.iter().sum::<f64>() / residual_energies.len() as f64;
    let structural_margin = structural_certificate.coefficient_margin
        * structural_certificate.image_operator_lower_bound
        * 0.18;
    let image_margin = if successful_margins.is_empty() {
        structural_margin.max(1e-6)
    } else {
        successful_margins
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .max(structural_margin * 0.5)
            .max(1e-6)
    };

    let shortlist_tail =
        0.5 * (-(image_margin * image_margin) / (8.0 * residual_variance.max(1e-9))).exp();
    let block_error_upper_bound = ((1.0 - shortlist_true_hit_rate)
        + shortlist_true_hit_rate
            * ((parameters.shortlist_size.saturating_sub(1)) as f64)
            * shortlist_tail)
        .clamp(0.0, 1.0);

    ProofReport {
        structural_certificate,
        image_margin,
        shortlist_true_hit_rate,
        stage_two_success_rate,
        block_error_upper_bound,
        residual_variance,
        tested_samples: parameters.monte_carlo_samples,
        assumptions: vec![
            "The proof is exact for the encoder and sampled-basis construction.".to_string(),
            "The optical shell is represented by a fixed three-path analytic transport model."
                .to_string(),
            "Camera variability is factorized into yaw, affine exposure, orientation leakage, and additive Gaussian-like luma noise.".to_string(),
            "The error bound applies only inside this explicit model family, not to arbitrary real-world captures.".to_string(),
        ],
    }
}

fn synthesize_observation(
    snapshot: &ChannelSnapshot,
    true_signs: &DVector<f64>,
    sample_index: usize,
) -> DVector<f64> {
    let camera_state = splitmix64(CAMERA_SEED ^ (sample_index as u64 + 1));
    let exposure = EXPOSURE_BASE + EXPOSURE_JITTER * centered_unit_float(camera_state);
    let bias = BIAS_AMPLITUDE * centered_unit_float(splitmix64(camera_state ^ 0x1));
    let orientation_leak = ORIENTATION_LEAK_BASE
        + ORIENTATION_LEAK_JITTER * positive_unit_float(splitmix64(camera_state ^ 0x2));
    let noise_std =
        NOISE_STD_BASE + NOISE_STD_JITTER * positive_unit_float(splitmix64(camera_state ^ 0x3));
    let mut observation = snapshot.render_candidate_image(true_signs) * exposure;
    observation += snapshot.orientation_image.clone() * orientation_leak;
    observation += DVector::from_element(observation.len(), bias);

    for pixel_index in 0..observation.len() {
        let noise_seed =
            splitmix64(camera_state ^ ((pixel_index as u64 + 1).wrapping_mul(SPLITMIX_INCREMENT)));
        observation[pixel_index] += noise_std * centered_unit_float(noise_seed);
    }

    observation
}

fn estimate_snapshot_and_logits(
    channel_snapshots: &[ChannelSnapshot],
    observation: &DVector<f64>,
) -> (usize, DVector<f64>) {
    let normalized_observation = normalize_image(observation);
    let mut best_index = 0;
    let mut best_logits = DVector::zeros(INTERNAL_CODEWORD_BITS);
    let mut best_score = f64::INFINITY;
    let mut best_orientation_alignment = f64::NEG_INFINITY;

    for snapshot in channel_snapshots {
        let logits = snapshot.estimate_payload_logits(observation);
        let primary_id = public_id_from_signs(logits.as_slice());
        let candidate = encode_internal_codeword(primary_id);
        let candidate_signs = DVector::from_vec(internal_codeword_signs(&candidate));
        let candidate_image = snapshot.render_candidate_image(&candidate_signs);
        let (_, _, residual) = fit_affine_to_reference(observation, &candidate_image);
        let residual_score = residual.dot(&residual) / residual.len() as f64;
        let orientation_alignment =
            normalized_observation.dot(&snapshot.normalized_orientation_image);

        if residual_score < best_score
            || (residual_score == best_score && orientation_alignment > best_orientation_alignment)
        {
            best_index = snapshot.yaw_index;
            best_score = residual_score;
            best_orientation_alignment = orientation_alignment;
            best_logits = logits;
        }
    }

    (best_index, best_logits)
}

fn build_shortlist(
    logits: &DVector<f64>,
    shortlist_size: usize,
    low_confidence_bits: usize,
) -> Vec<u64> {
    let primary_id = public_id_from_signs(logits.as_slice());
    let ranked_public_bits = (0..64)
        .map(|bit_index| (bit_index, logits[bit_index].abs()))
        .collect::<Vec<_>>();
    let mut ranked_public_bits = ranked_public_bits;
    ranked_public_bits.sort_by(|left, right| left.1.total_cmp(&right.1));
    let uncertain_bits = ranked_public_bits
        .into_iter()
        .take(low_confidence_bits)
        .collect::<Vec<_>>();
    let mut candidates = vec![(primary_id, 0.0)];

    for (bit_index, confidence) in &uncertain_bits {
        let flipped = primary_id ^ (1_u64 << (63 - *bit_index));
        candidates.push((flipped, *confidence));
    }

    for left_index in 0..uncertain_bits.len() {
        for right_index in (left_index + 1)..uncertain_bits.len() {
            let left_bit = uncertain_bits[left_index].0;
            let right_bit = uncertain_bits[right_index].0;
            let score = uncertain_bits[left_index].1 + uncertain_bits[right_index].1;
            let flipped = primary_id ^ (1_u64 << (63 - left_bit)) ^ (1_u64 << (63 - right_bit));
            candidates.push((flipped, score));
        }
    }

    candidates.sort_by(|left, right| left.1.total_cmp(&right.1));
    let mut shortlist = Vec::with_capacity(shortlist_size);
    let mut seen = HashSet::new();

    for (candidate_id, _) in candidates {
        if shortlist.len() >= shortlist_size {
            break;
        }
        if seen.insert(candidate_id) {
            shortlist.push(candidate_id);
        }
    }

    shortlist
}

fn score_shortlist_candidates(
    snapshot: &ChannelSnapshot,
    observation: &DVector<f64>,
    shortlist: &[u64],
) -> Vec<(u64, f64)> {
    let mut scored = shortlist
        .iter()
        .map(|candidate_id| {
            let candidate = encode_internal_codeword(*candidate_id);
            let signs = DVector::from_vec(internal_codeword_signs(&candidate));
            let candidate_image = snapshot.render_candidate_image(&signs);
            let (_, _, residual) = fit_affine_to_reference(observation, &candidate_image);
            let score = residual.dot(&residual) / residual.len() as f64;
            (*candidate_id, score)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| left.1.total_cmp(&right.1));
    scored
}

fn sample_public_id(sample_index: usize) -> u64 {
    splitmix64(ID_SEED ^ (sample_index as u64 + 1))
}

fn sample_yaw_index(sample_index: usize, yaw_count: usize) -> usize {
    sample_index % yaw_count
}

fn positive_unit_float(value: u64) -> f64 {
    let mantissa = value >> 11;
    mantissa as f64 / ((1_u64 << 53) as f64)
}

fn centered_unit_float(value: u64) -> f64 {
    positive_unit_float(value) * 2.0 - 1.0
}

fn splitmix64(mut state: u64) -> u64 {
    state = state.wrapping_add(SPLITMIX_INCREMENT);
    state = (state ^ (state >> 30)).wrapping_mul(SPLITMIX_MULTIPLIER_A);
    state = (state ^ (state >> 27)).wrapping_mul(SPLITMIX_MULTIPLIER_B);
    state ^ (state >> 31)
}
