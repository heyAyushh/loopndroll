pub mod channel;
pub mod codeword;
pub mod frame;
pub mod linear;
pub mod parameters;
pub mod proof;
pub mod types;

pub use codeword::{INTERNAL_CODEWORD_BITS, PUBLIC_ID_BITS, encode_internal_codeword};
pub use parameters::CausticProofParameters;
pub use proof::{CausticProofModel, generate_proof_report};
pub use types::{InternalCodeword, ProofReport, StructuralCertificate};

#[cfg(test)]
mod tests {
    use crate::caustic::{
        CausticProofModel, CausticProofParameters, INTERNAL_CODEWORD_BITS, PUBLIC_ID_BITS,
        encode_internal_codeword, generate_proof_report,
    };

    #[test]
    fn default_proof_parameters_match_the_spec() {
        let parameters = CausticProofParameters::default();

        assert_eq!(PUBLIC_ID_BITS, 64);
        assert_eq!(INTERNAL_CODEWORD_BITS, 128);
        assert!(parameters.payload_atom_count >= INTERNAL_CODEWORD_BITS);
        assert!(parameters.orientation_atom_count > 0);
        assert!(parameters.max_yaw_steps > 0);
    }

    #[test]
    fn orientation_and_payload_subspaces_are_nearly_orthogonal() {
        let model = CausticProofModel::default();
        let certificate = model.structural_certificate();

        assert!(certificate.max_orientation_payload_inner_product < 1e-6);
    }

    #[test]
    fn internal_codeword_is_deterministic_and_length_128() {
        let left = encode_internal_codeword(0x0123_4567_89ab_cdef);
        let right = encode_internal_codeword(0x0123_4567_89ab_cdef);

        assert_eq!(left.bits.len(), 128);
        assert_eq!(left.bits, right.bits);
    }

    #[test]
    fn coefficient_separation_bound_is_positive() {
        let model = CausticProofModel::default();
        let certificate = model.structural_certificate();

        assert!(certificate.coefficient_margin > 0.0);
        assert!(certificate.frame_lower_bound > 0.0);
    }

    #[test]
    fn proof_report_contains_positive_image_margin_and_valid_error_bound() {
        let report = generate_proof_report(&CausticProofParameters::default());

        assert!(report.image_margin > 0.0);
        assert!(report.shortlist_true_hit_rate >= 0.99);
        assert!(report.block_error_upper_bound >= 0.0);
        assert!(report.block_error_upper_bound <= 1.0);
    }
}
