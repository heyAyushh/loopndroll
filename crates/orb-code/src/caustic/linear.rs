use nalgebra::{DMatrix, DVector, SymmetricEigen};

const EIGENVALUE_FLOOR: f64 = 0.0;

pub fn dot(left: &DVector<f64>, right: &DVector<f64>) -> f64 {
    left.dot(right)
}

pub fn l2_normalize(vector: &DVector<f64>) -> DVector<f64> {
    let norm = vector.norm();
    if norm <= f64::EPSILON {
        return vector.clone();
    }

    vector / norm
}

pub fn mean_center(vector: &DVector<f64>) -> DVector<f64> {
    let mean = vector.iter().sum::<f64>() / vector.len() as f64;
    vector.map(|value| value - mean)
}

pub fn max_abs_inner_product(left: &DMatrix<f64>, right: &DMatrix<f64>) -> f64 {
    if left.ncols() == 0 || right.ncols() == 0 {
        return 0.0;
    }

    let products = left.transpose() * right;
    products.iter().map(|value| value.abs()).fold(0.0, f64::max)
}

pub fn gram_matrix(matrix: &DMatrix<f64>) -> DMatrix<f64> {
    matrix.transpose() * matrix
}

pub fn spectral_bounds(matrix: &DMatrix<f64>) -> (f64, f64) {
    if matrix.nrows() == 0 || matrix.ncols() == 0 {
        return (0.0, 0.0);
    }

    let eigen = SymmetricEigen::new(matrix.clone());
    let minimum = eigen
        .eigenvalues
        .iter()
        .fold(f64::INFINITY, |current, value| current.min(*value))
        .max(EIGENVALUE_FLOOR);
    let maximum = eigen
        .eigenvalues
        .iter()
        .fold(f64::NEG_INFINITY, |current, value| current.max(*value))
        .max(EIGENVALUE_FLOOR);

    (minimum, maximum)
}

pub fn inverse_symmetric(matrix: &DMatrix<f64>) -> DMatrix<f64> {
    matrix
        .clone()
        .try_inverse()
        .expect("symmetric matrix should be invertible for the proof model")
}

pub fn matrix_from_columns(columns: &[DVector<f64>]) -> DMatrix<f64> {
    if columns.is_empty() {
        return DMatrix::zeros(0, 0);
    }

    DMatrix::from_columns(columns)
}
