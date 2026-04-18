use nalgebra::{DMatrix, DVector};

use crate::caustic::{
    codeword::INTERNAL_CODEWORD_BITS,
    frame::{SphereDirection, WaveletFrame, rotate_about_y},
    linear::{inverse_symmetric, l2_normalize, mean_center},
    parameters::CausticProofParameters,
};

const ACTIVE_RADIUS: f64 = 0.97;
const SHELL_DIRECTION_COUNT: usize = 3;
const DECODER_REGULARIZATION: f64 = 1e-3;
const DIRECT_READOUT_WEIGHT: f64 = 2.8;
const NEIGHBOR_READOUT_WEIGHT: f64 = 0.35;
const SHELL_MIX_WEIGHT: f64 = 0.10;

#[derive(Debug, Clone)]
pub struct ChannelSnapshot {
    pub active_pixels: Vec<(f64, f64)>,
    pub orientation_image: DVector<f64>,
    pub normalized_orientation_image: DVector<f64>,
    pub bit_image_matrix: DMatrix<f64>,
    pub bit_decoder_matrix: DMatrix<f64>,
    pub yaw_index: usize,
    pub yaw_radians: f64,
}

impl ChannelSnapshot {
    pub fn render_payload_from_signs(&self, signs: &DVector<f64>) -> DVector<f64> {
        &self.bit_image_matrix * signs
    }

    pub fn render_candidate_image(&self, signs: &DVector<f64>) -> DVector<f64> {
        &self.orientation_image + self.render_payload_from_signs(signs)
    }

    pub fn estimate_payload_logits(&self, observation: &DVector<f64>) -> DVector<f64> {
        let (_, _, orientation_residual) =
            fit_affine_to_reference(observation, &self.orientation_image);
        &self.bit_decoder_matrix * orientation_residual
    }
}

pub fn build_channel_snapshot(
    frame: &WaveletFrame,
    parameters: &CausticProofParameters,
    yaw_index: usize,
) -> ChannelSnapshot {
    let active_pixels = build_active_pixels(parameters.image_sample_side);
    let yaw_radians = yaw_index as f64 * (std::f64::consts::TAU / parameters.max_yaw_steps as f64);
    let image_length = active_pixels.len();
    let mut orientation_image = DVector::zeros(image_length);
    let mut payload_to_image = DMatrix::zeros(image_length, parameters.payload_atom_count);

    for (row_index, (u, v)) in active_pixels.iter().enumerate() {
        let shell_paths = shell_directions(*u, *v, yaw_radians);
        let shell_weights = shell_weights(*u, *v);

        let mut orientation_value = 0.0;
        let mut payload_row = DVector::zeros(parameters.payload_atom_count);

        for path_index in 0..SHELL_DIRECTION_COUNT {
            let orientation_values = frame.sample_orientation_row(shell_paths[path_index]);
            let payload_values = frame.sample_payload_row(shell_paths[path_index]);
            orientation_value +=
                shell_weights[path_index] * orientation_values.dot(&frame.orientation_signature);
            payload_row += payload_values * shell_weights[path_index];
        }

        orientation_image[row_index] = orientation_value;
        for column_index in 0..parameters.payload_atom_count {
            payload_to_image[(row_index, column_index)] =
                payload_row[column_index] * SHELL_MIX_WEIGHT;
        }
    }

    add_direct_readout(&mut payload_to_image);

    let bit_image_matrix = frame.payload_image_bit_matrix(&payload_to_image);
    let bit_gram = bit_image_matrix.transpose() * &bit_image_matrix
        + DMatrix::identity(INTERNAL_CODEWORD_BITS, INTERNAL_CODEWORD_BITS)
            * DECODER_REGULARIZATION;
    let bit_decoder_matrix = inverse_symmetric(&bit_gram) * bit_image_matrix.transpose();
    let normalized_orientation_image = normalize_image(&orientation_image);

    ChannelSnapshot {
        active_pixels,
        orientation_image,
        normalized_orientation_image,
        bit_image_matrix,
        bit_decoder_matrix,
        yaw_index,
        yaw_radians,
    }
}

pub fn normalize_image(image: &DVector<f64>) -> DVector<f64> {
    l2_normalize(&mean_center(image))
}

pub fn fit_affine_to_reference(
    observation: &DVector<f64>,
    reference: &DVector<f64>,
) -> (f64, f64, DVector<f64>) {
    let observation_mean = observation.iter().sum::<f64>() / observation.len() as f64;
    let reference_mean = reference.iter().sum::<f64>() / reference.len() as f64;
    let centered_observation = observation.map(|value| value - observation_mean);
    let centered_reference = reference.map(|value| value - reference_mean);
    let denominator = centered_reference.dot(&centered_reference).max(1e-9);
    let gain = centered_observation.dot(&centered_reference) / denominator;
    let bias = observation_mean - gain * reference_mean;
    let fitted = reference * gain + DVector::from_element(reference.len(), bias);
    let residual = observation - fitted;

    (gain, bias, residual)
}

fn build_active_pixels(image_side: usize) -> Vec<(f64, f64)> {
    let mut pixels = Vec::new();
    let image_side_f64 = image_side as f64;

    for y_index in 0..image_side {
        for x_index in 0..image_side {
            let u = ((x_index as f64 + 0.5) / image_side_f64 - 0.5) * 2.0;
            let v = -((y_index as f64 + 0.5) / image_side_f64 - 0.5) * 2.0;
            if u * u + v * v <= ACTIVE_RADIUS * ACTIVE_RADIUS {
                pixels.push((u, v));
            }
        }
    }

    pixels
}

fn shell_directions(u: f64, v: f64, yaw_radians: f64) -> [SphereDirection; SHELL_DIRECTION_COUNT] {
    let radial = u * u + v * v;
    [
        rotate_about_y(SphereDirection::new(u, v, 1.0 - 0.32 * radial), yaw_radians),
        rotate_about_y(
            SphereDirection::new(
                0.82 * u + 0.28 * (1.0 - radial),
                0.76 * v - 0.18,
                0.96 - 0.22 * radial,
            ),
            yaw_radians,
        ),
        rotate_about_y(
            SphereDirection::new(
                -0.61 * u + 0.14,
                0.84 * v + 0.10 * (1.0 - radial),
                0.88 - 0.17 * radial,
            ),
            yaw_radians,
        ),
    ]
}

fn shell_weights(u: f64, v: f64) -> [f64; SHELL_DIRECTION_COUNT] {
    let radial = u * u + v * v;
    [
        0.66 + 0.24 * (1.0 - radial),
        0.23 + 0.07 * (u + 1.0) * 0.5,
        0.15 + 0.10 * (1.0 - v.abs()),
    ]
}

fn add_direct_readout(payload_to_image: &mut DMatrix<f64>) {
    let image_length = payload_to_image.nrows();
    let payload_count = payload_to_image.ncols();
    let stride = image_length as f64 / payload_count as f64;

    for payload_index in 0..payload_count {
        let anchor = (((payload_index as f64) + 0.5) * stride)
            .floor()
            .min((image_length - 1) as f64) as usize;
        payload_to_image[(anchor, payload_index)] += DIRECT_READOUT_WEIGHT;
        if anchor > 0 {
            payload_to_image[(anchor - 1, payload_index)] += NEIGHBOR_READOUT_WEIGHT;
        }
        if anchor + 1 < image_length {
            payload_to_image[(anchor + 1, payload_index)] += NEIGHBOR_READOUT_WEIGHT;
        }
    }
}
