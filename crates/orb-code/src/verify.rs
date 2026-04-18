use crate::{
    DEFAULT_VERIFY_THRESHOLD, GenerateOrbRequest, Result, VerificationResult, generate_orb_image,
    render::{core_fingerprint_radius, orb_layout},
    scan::{OrbGeometry, scan_orb_image_with_geometry},
};

pub fn verify_orb_image(image_bytes: &[u8]) -> Result<VerificationResult> {
    let decoded_orb = scan_orb_image_with_geometry(image_bytes)?;
    let input_image = image::load_from_memory(image_bytes)?.to_rgba8();
    let generated = generate_orb_image(&GenerateOrbRequest::new(
        decoded_orb.scan_result.orb_id.clone(),
    ))?;
    let (expected_center_x, expected_center_y, expected_radius) =
        orb_layout(generated.image.width());

    let input_fingerprint = extract_core_fingerprint(&input_image, decoded_orb.geometry);
    let expected_fingerprint = extract_core_fingerprint(
        &generated.image,
        OrbGeometry {
            center_x: expected_center_x,
            center_y: expected_center_y,
            radius: expected_radius,
        },
    );
    let distance = fingerprint_distance(&input_fingerprint, &expected_fingerprint);
    Ok(VerificationResult {
        orb_id: decoded_orb.scan_result.orb_id,
        version: decoded_orb.scan_result.version,
        is_match: distance <= DEFAULT_VERIFY_THRESHOLD,
        distance,
        threshold: DEFAULT_VERIFY_THRESHOLD,
    })
}

fn extract_core_fingerprint(image: &image::RgbaImage, geometry: OrbGeometry) -> Vec<u8> {
    let sample_radius = core_fingerprint_radius(geometry.radius);
    let mut samples = Vec::with_capacity(21);

    for grid_y in 0..5 {
        for grid_x in 0..5 {
            let normalized_x = (f64::from(grid_x) - 2.0) / 2.0;
            let normalized_y = (f64::from(grid_y) - 2.0) / 2.0;
            if normalized_x * normalized_x + normalized_y * normalized_y > 1.0 {
                continue;
            }

            let sample_x = (geometry.center_x + normalized_x * sample_radius).round() as i32;
            let sample_y = (geometry.center_y + normalized_y * sample_radius).round() as i32;
            let mut sum = 0.0;
            let mut count = 0.0;
            for offset_y in -2..=2 {
                for offset_x in -2..=2 {
                    let x = (sample_x + offset_x).clamp(0, image.width() as i32 - 1) as u32;
                    let y = (sample_y + offset_y).clamp(0, image.height() as i32 - 1) as u32;
                    let pixel = image.get_pixel(x, y);
                    sum += (f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2])) / 3.0;
                    count += 1.0;
                }
            }
            samples.push(sum / count);
        }
    }

    let minimum = samples
        .iter()
        .copied()
        .fold(f64::INFINITY, |current, sample| current.min(sample));
    let maximum = samples
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, |current, sample| current.max(sample));
    let range = (maximum - minimum).max(1.0);

    samples
        .into_iter()
        .map(|sample| (((sample - minimum) / range) * 7.0).round() as u8)
        .collect()
}

fn fingerprint_distance(left: &[u8], right: &[u8]) -> u32 {
    left.iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| u32::from(left_value.abs_diff(*right_value)))
        .sum()
}
