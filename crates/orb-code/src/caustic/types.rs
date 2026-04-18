#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalCodeword {
    pub public_id: u64,
    pub bits: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct StructuralCertificate {
    pub max_orientation_payload_inner_product: f64,
    pub frame_lower_bound: f64,
    pub frame_upper_bound: f64,
    pub coefficient_margin: f64,
    pub image_operator_lower_bound: f64,
}

#[derive(Debug, Clone)]
pub struct ProofReport {
    pub structural_certificate: StructuralCertificate,
    pub image_margin: f64,
    pub shortlist_true_hit_rate: f64,
    pub stage_two_success_rate: f64,
    pub block_error_upper_bound: f64,
    pub residual_variance: f64,
    pub tested_samples: usize,
    pub assumptions: Vec<String>,
}
