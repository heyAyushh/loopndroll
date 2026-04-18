use std::{fmt::Write as _, fs, path::PathBuf};

use orb_code::caustic::{CausticProofParameters, generate_proof_report};

const REPORT_PATH: &str = "docs/reports/2026-04-18-pure-caustic-orb-proof-report.md";

fn main() {
    let parameters = CausticProofParameters::default();
    let report = generate_proof_report(&parameters);
    let markdown = render_markdown(&parameters, &report);
    let report_path = PathBuf::from(REPORT_PATH);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).expect("create report directory");
    }
    fs::write(&report_path, &markdown).expect("write report");
    println!("{markdown}");
}

fn render_markdown(
    parameters: &CausticProofParameters,
    report: &orb_code::caustic::ProofReport,
) -> String {
    let mut markdown = String::new();
    writeln!(&mut markdown, "# Pure Caustic Orb Proof Report").unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Parameters").unwrap();
    writeln!(
        &mut markdown,
        "- orientation atoms: {}",
        parameters.orientation_atom_count
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- payload atoms: {}",
        parameters.payload_atom_count
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- sphere samples: {}",
        parameters.sphere_sample_count
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- image sample side: {}",
        parameters.image_sample_side
    )
    .unwrap();
    writeln!(&mut markdown, "- yaw steps: {}", parameters.max_yaw_steps).unwrap();
    writeln!(
        &mut markdown,
        "- monte carlo samples: {}",
        parameters.monte_carlo_samples
    )
    .unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Structural Certificate").unwrap();
    writeln!(
        &mut markdown,
        "- max orientation/payload inner product: {:.6e}",
        report
            .structural_certificate
            .max_orientation_payload_inner_product
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- frame lower bound: {:.6}",
        report.structural_certificate.frame_lower_bound
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- frame upper bound: {:.6}",
        report.structural_certificate.frame_upper_bound
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- coefficient margin: {:.6}",
        report.structural_certificate.coefficient_margin
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- image operator lower bound: {:.6}",
        report.structural_certificate.image_operator_lower_bound
    )
    .unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Decoder Metrics").unwrap();
    writeln!(&mut markdown, "- image margin: {:.6}", report.image_margin).unwrap();
    writeln!(
        &mut markdown,
        "- shortlist true hit rate: {:.6}",
        report.shortlist_true_hit_rate
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- stage two success rate: {:.6}",
        report.stage_two_success_rate
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- residual variance: {:.6}",
        report.residual_variance
    )
    .unwrap();
    writeln!(
        &mut markdown,
        "- block error upper bound: {:.6e}",
        report.block_error_upper_bound
    )
    .unwrap();
    writeln!(&mut markdown, "- tested samples: {}", report.tested_samples).unwrap();
    writeln!(&mut markdown).unwrap();
    writeln!(&mut markdown, "## Assumptions").unwrap();
    for assumption in &report.assumptions {
        writeln!(&mut markdown, "- {}", assumption).unwrap();
    }

    markdown
}
