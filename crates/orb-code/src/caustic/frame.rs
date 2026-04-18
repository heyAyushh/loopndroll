use nalgebra::{DMatrix, DVector};

use crate::caustic::{
    codeword::INTERNAL_CODEWORD_BITS, linear::matrix_from_columns,
    parameters::CausticProofParameters,
};

const FRONT_HEMISPHERE_THRESHOLD: f64 = -0.18;
const ORIENTATION_SIGNATURE: [f64; 3] = [1.0, -0.42, 0.31];
const PAYLOAD_SCALES: [f64; 4] = [0.18, 0.21, 0.24, 0.28];
const PAYLOAD_HALO_RATIO: f64 = 2.35;
const PAYLOAD_HALO_WEIGHT: f64 = 0.58;
const PAYLOAD_CODE_SCALE: f64 = 0.11;
const MINIMUM_BASIS_NORM: f64 = 1e-8;
const SPLITMIX_INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MULTIPLIER_A: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MULTIPLIER_B: u64 = 0x94D0_49BB_1331_11EB;
const PACKING_SEED: u64 = 0x6f72_622d_6361_7573;

#[derive(Debug, Clone, Copy)]
pub struct SphereDirection {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl SphereDirection {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        let length = (x * x + y * y + z * z).sqrt().max(f64::EPSILON);
        Self {
            x: x / length,
            y: y / length,
            z: z / length,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum OrientationFunction {
    AzimuthSine,
    AzimuthCosine,
    Notch,
}

#[derive(Debug, Clone, Copy)]
struct PayloadAtomDefinition {
    center: SphereDirection,
    scale: f64,
    halo_scale: f64,
}

#[derive(Debug, Clone)]
pub struct WaveletFrame {
    pub sphere_samples: Vec<SphereDirection>,
    pub orientation_matrix: DMatrix<f64>,
    pub payload_matrix: DMatrix<f64>,
    pub packing_matrix: DMatrix<f64>,
    pub orientation_signature: DVector<f64>,
    pub payload_code_scale: f64,
    orientation_transform: DMatrix<f64>,
    payload_transform: DMatrix<f64>,
    orientation_functions: Vec<OrientationFunction>,
    payload_atoms: Vec<PayloadAtomDefinition>,
}

impl WaveletFrame {
    pub fn sample_orientation_row(&self, direction: SphereDirection) -> DVector<f64> {
        let sample_index = self.nearest_sample_index(direction);
        self.orientation_matrix
            .row(sample_index)
            .transpose()
            .into_owned()
    }

    pub fn sample_payload_row(&self, direction: SphereDirection) -> DVector<f64> {
        let sample_index = self.nearest_sample_index(direction);
        self.payload_matrix
            .row(sample_index)
            .transpose()
            .into_owned()
    }

    pub fn evaluate_orientation_basis(&self, direction: SphereDirection) -> DVector<f64> {
        let raw_values = DVector::from_iterator(
            self.orientation_functions.len(),
            self.orientation_functions
                .iter()
                .map(|function| evaluate_orientation_raw(*function, direction)),
        );
        self.orientation_transform.transpose() * raw_values
    }

    pub fn evaluate_payload_basis(&self, direction: SphereDirection) -> DVector<f64> {
        let mut raw_values =
            DVector::zeros(self.orientation_functions.len() + self.payload_atoms.len());
        for (index, function) in self.orientation_functions.iter().enumerate() {
            raw_values[index] = evaluate_orientation_raw(*function, direction);
        }
        for (index, atom) in self.payload_atoms.iter().enumerate() {
            raw_values[self.orientation_functions.len() + index] =
                evaluate_payload_raw(*atom, direction);
        }

        self.payload_transform.transpose() * raw_values
    }

    pub fn payload_image_bit_matrix(&self, payload_to_image: &DMatrix<f64>) -> DMatrix<f64> {
        payload_to_image * (&self.packing_matrix * self.payload_code_scale)
    }

    pub fn orientation_signal(&self) -> DVector<f64> {
        &self.orientation_matrix * &self.orientation_signature
    }

    fn nearest_sample_index(&self, direction: SphereDirection) -> usize {
        let mut best_index = 0;
        let mut best_alignment = f64::NEG_INFINITY;

        for (sample_index, sample) in self.sphere_samples.iter().enumerate() {
            let alignment = dot_direction(*sample, direction);
            if alignment > best_alignment {
                best_alignment = alignment;
                best_index = sample_index;
            }
        }

        best_index
    }
}

pub fn build_payload_frame(parameters: &CausticProofParameters) -> WaveletFrame {
    let sphere_samples = fibonacci_sphere(parameters.sphere_sample_count);
    let orientation_functions = vec![
        OrientationFunction::AzimuthSine,
        OrientationFunction::AzimuthCosine,
        OrientationFunction::Notch,
    ];
    let raw_orientation_matrix =
        build_raw_orientation_matrix(&sphere_samples, &orientation_functions);
    let (orientation_matrix, orientation_transform) =
        orthonormalize_orientation(&raw_orientation_matrix);

    let payload_atoms = build_payload_atoms(parameters, &sphere_samples);
    let raw_payload_matrix = build_raw_payload_matrix(&sphere_samples, &payload_atoms);
    let (payload_matrix, payload_transform) = orthonormalize_payload(
        &raw_payload_matrix,
        &orientation_matrix,
        &orientation_transform,
    );

    let packing_matrix =
        build_packing_matrix(parameters.payload_atom_count, INTERNAL_CODEWORD_BITS);
    let orientation_signature = DVector::from_column_slice(&ORIENTATION_SIGNATURE);

    WaveletFrame {
        sphere_samples,
        orientation_matrix,
        payload_matrix,
        packing_matrix,
        orientation_signature,
        payload_code_scale: PAYLOAD_CODE_SCALE,
        orientation_transform,
        payload_transform,
        orientation_functions,
        payload_atoms,
    }
}

pub fn rotate_about_y(direction: SphereDirection, yaw_radians: f64) -> SphereDirection {
    let cosine = yaw_radians.cos();
    let sine = yaw_radians.sin();
    SphereDirection::new(
        cosine * direction.x + sine * direction.z,
        direction.y,
        -sine * direction.x + cosine * direction.z,
    )
}

fn fibonacci_sphere(sample_count: usize) -> Vec<SphereDirection> {
    let mut samples = Vec::with_capacity(sample_count);
    let sample_count_f64 = sample_count as f64;
    let golden_angle = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());

    for index in 0..sample_count {
        let index_f64 = index as f64;
        let y = 1.0 - ((2.0 * index_f64 + 1.0) / sample_count_f64);
        let radial = (1.0 - y * y).sqrt().max(0.0);
        let theta = golden_angle * index_f64;
        let x = theta.cos() * radial;
        let z = theta.sin() * radial;
        samples.push(SphereDirection::new(x, y, z));
    }

    samples
}

fn build_raw_orientation_matrix(
    sphere_samples: &[SphereDirection],
    orientation_functions: &[OrientationFunction],
) -> DMatrix<f64> {
    let mut columns = Vec::with_capacity(orientation_functions.len());
    for function in orientation_functions {
        let column = DVector::from_iterator(
            sphere_samples.len(),
            sphere_samples
                .iter()
                .map(|direction| evaluate_orientation_raw(*function, *direction)),
        );
        columns.push(column);
    }

    matrix_from_columns(&columns)
}

fn build_payload_atoms(
    parameters: &CausticProofParameters,
    sphere_samples: &[SphereDirection],
) -> Vec<PayloadAtomDefinition> {
    sphere_samples
        .iter()
        .copied()
        .filter(|direction| direction.z >= FRONT_HEMISPHERE_THRESHOLD)
        .enumerate()
        .take(parameters.payload_atom_count)
        .map(|(index, center)| {
            let scale = PAYLOAD_SCALES[index % PAYLOAD_SCALES.len()];
            PayloadAtomDefinition {
                center,
                scale,
                halo_scale: scale * PAYLOAD_HALO_RATIO,
            }
        })
        .collect()
}

fn build_raw_payload_matrix(
    sphere_samples: &[SphereDirection],
    payload_atoms: &[PayloadAtomDefinition],
) -> DMatrix<f64> {
    let mut columns = Vec::with_capacity(payload_atoms.len());
    for atom in payload_atoms {
        let column = DVector::from_iterator(
            sphere_samples.len(),
            sphere_samples
                .iter()
                .map(|direction| evaluate_payload_raw(*atom, *direction)),
        );
        columns.push(column);
    }

    matrix_from_columns(&columns)
}

fn orthonormalize_orientation(
    raw_orientation_matrix: &DMatrix<f64>,
) -> (DMatrix<f64>, DMatrix<f64>) {
    let mut basis_columns: Vec<DVector<f64>> = Vec::with_capacity(raw_orientation_matrix.ncols());
    let mut coefficient_columns: Vec<DVector<f64>> =
        Vec::with_capacity(raw_orientation_matrix.ncols());

    for column_index in 0..raw_orientation_matrix.ncols() {
        let mut sample_vector = raw_orientation_matrix.column(column_index).into_owned();
        let mut coefficient_vector = DVector::zeros(raw_orientation_matrix.ncols());
        coefficient_vector[column_index] = 1.0;

        for basis_index in 0..basis_columns.len() {
            let projection = basis_columns[basis_index].dot(&sample_vector);
            sample_vector -= basis_columns[basis_index].clone() * projection;
            coefficient_vector -= coefficient_columns[basis_index].clone() * projection;
        }

        let norm = sample_vector.norm();
        assert!(
            norm > MINIMUM_BASIS_NORM,
            "orientation basis should remain linearly independent"
        );
        sample_vector /= norm;
        coefficient_vector /= norm;
        basis_columns.push(sample_vector);
        coefficient_columns.push(coefficient_vector);
    }

    (
        matrix_from_columns(&basis_columns),
        matrix_from_columns(&coefficient_columns),
    )
}

fn orthonormalize_payload(
    raw_payload_matrix: &DMatrix<f64>,
    orientation_matrix: &DMatrix<f64>,
    orientation_transform: &DMatrix<f64>,
) -> (DMatrix<f64>, DMatrix<f64>) {
    let orientation_raw_count = orientation_transform.nrows();
    let payload_raw_count = raw_payload_matrix.ncols();
    let augmented_raw_count = orientation_raw_count + payload_raw_count;
    let mut orientation_coefficient_columns = Vec::with_capacity(orientation_matrix.ncols());

    for orientation_index in 0..orientation_matrix.ncols() {
        let mut coefficient_vector = DVector::zeros(augmented_raw_count);
        for row_index in 0..orientation_raw_count {
            coefficient_vector[row_index] = orientation_transform[(row_index, orientation_index)];
        }
        orientation_coefficient_columns.push(coefficient_vector);
    }

    let mut basis_columns: Vec<DVector<f64>> = Vec::with_capacity(payload_raw_count);
    let mut coefficient_columns: Vec<DVector<f64>> = Vec::with_capacity(payload_raw_count);

    for payload_index in 0..payload_raw_count {
        let mut sample_vector = raw_payload_matrix.column(payload_index).into_owned();
        let mut coefficient_vector = DVector::zeros(augmented_raw_count);
        coefficient_vector[orientation_raw_count + payload_index] = 1.0;

        for orientation_index in 0..orientation_matrix.ncols() {
            let projection = orientation_matrix
                .column(orientation_index)
                .dot(&sample_vector);
            sample_vector -= orientation_matrix.column(orientation_index).into_owned() * projection;
            coefficient_vector -=
                orientation_coefficient_columns[orientation_index].clone() * projection;
        }

        for basis_index in 0..basis_columns.len() {
            let projection = basis_columns[basis_index].dot(&sample_vector);
            sample_vector -= basis_columns[basis_index].clone() * projection;
            coefficient_vector -= coefficient_columns[basis_index].clone() * projection;
        }

        let norm = sample_vector.norm();
        assert!(
            norm > MINIMUM_BASIS_NORM,
            "payload basis should remain linearly independent"
        );
        sample_vector /= norm;
        coefficient_vector /= norm;
        basis_columns.push(sample_vector);
        coefficient_columns.push(coefficient_vector);
    }

    (
        matrix_from_columns(&basis_columns),
        matrix_from_columns(&coefficient_columns),
    )
}

fn evaluate_orientation_raw(function: OrientationFunction, direction: SphereDirection) -> f64 {
    match function {
        OrientationFunction::AzimuthSine => direction.x,
        OrientationFunction::AzimuthCosine => direction.z,
        OrientationFunction::Notch => direction.x * direction.x - direction.z * direction.z,
    }
}

fn evaluate_payload_raw(atom: PayloadAtomDefinition, direction: SphereDirection) -> f64 {
    let local_alignment = dot_direction(direction, atom.center);
    let local_lobe = ((local_alignment - 1.0) / (atom.scale * atom.scale)).exp();
    let halo_lobe = ((local_alignment - 1.0) / (atom.halo_scale * atom.halo_scale)).exp();
    local_lobe - PAYLOAD_HALO_WEIGHT * halo_lobe
}

fn build_packing_matrix(row_count: usize, column_count: usize) -> DMatrix<f64> {
    let mut columns: Vec<DVector<f64>> = Vec::with_capacity(column_count);

    for column_index in 0..column_count {
        let seed = PACKING_SEED ^ (column_index as u64 + 1).wrapping_mul(SPLITMIX_INCREMENT);
        let mut column = DVector::from_fn(row_count, |row_index, _| {
            let row_seed = (row_index as u64 + 1).wrapping_mul(SPLITMIX_MULTIPLIER_A);
            splitmix_unit_interval(seed ^ row_seed) * 2.0 - 1.0
        });

        for existing_column in &columns {
            let projection = existing_column.dot(&column);
            column -= existing_column.clone() * projection;
        }

        let norm = column.norm();
        assert!(
            norm > MINIMUM_BASIS_NORM,
            "packing matrix columns should remain linearly independent"
        );
        column /= norm;
        columns.push(column);
    }

    matrix_from_columns(&columns)
}

fn splitmix_unit_interval(mut state: u64) -> f64 {
    state = state.wrapping_add(SPLITMIX_INCREMENT);
    state = (state ^ (state >> 30)).wrapping_mul(SPLITMIX_MULTIPLIER_A);
    state = (state ^ (state >> 27)).wrapping_mul(SPLITMIX_MULTIPLIER_B);
    state ^= state >> 31;

    let mantissa = state >> 11;
    mantissa as f64 / ((1_u64 << 53) as f64)
}

fn dot_direction(left: SphereDirection, right: SphereDirection) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}
