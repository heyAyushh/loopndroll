#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausticProofParameters {
    pub orientation_atom_count: usize,
    pub payload_atom_count: usize,
    pub sphere_sample_count: usize,
    pub image_sample_side: usize,
    pub max_yaw_steps: usize,
    pub monte_carlo_samples: usize,
    pub shortlist_size: usize,
    pub low_confidence_bits: usize,
}

impl Default for CausticProofParameters {
    fn default() -> Self {
        Self {
            orientation_atom_count: 3,
            payload_atom_count: 136,
            sphere_sample_count: 384,
            image_sample_side: 18,
            max_yaw_steps: 12,
            monte_carlo_samples: 64,
            shortlist_size: 8,
            low_confidence_bits: 8,
        }
    }
}
