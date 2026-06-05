use image::{GrayImage, imageops::FilterType};

use crate::{
    OrbError, Result, ScanResult,
    payload::{SECTOR_COUNT, TOTAL_CELL_COUNT, TRACK_COUNT, decode_marker_cells},
    render::{PAYLOAD_RING_INNER_RATIO, PAYLOAD_RING_OUTER_RATIO, payload_track_radius_ratio},
};

const DETECTION_TARGET_MAX_DIMENSION: u32 = 320;
const COARSE_CENTER_STEP: usize = 6;
const COARSE_RADIUS_STEP: usize = 3;
const COARSE_SAMPLE_COUNT: usize = 72;
const FINE_SAMPLE_COUNT: usize = 112;
const MINIMUM_DETECTION_SCORE: f64 = 24.0;
const MINIMUM_RADIUS_RATIO: f64 = 0.12;
const MAXIMUM_RADIUS_RATIO: f64 = 0.46;
const EDGE_SAMPLE_RADIUS_RATIO: f64 = 0.965;
const INNER_SAMPLE_RADIUS_RATIO: f64 = 0.905;
const OUTER_SAMPLE_RADIUS_RATIO: f64 = 1.025;
const PAYLOAD_VARIANCE_RADIUS_RATIO: f64 = 0.79;
const CELL_ANGLE_SAMPLE_OFFSETS: [f64; 5] = [-0.28, -0.14, 0.0, 0.14, 0.28];
const CELL_RADIAL_SAMPLE_OFFSETS: [f64; 5] = [-0.42, -0.2, 0.0, 0.2, 0.42];
const CELL_DARKEST_SAMPLE_COUNT: usize = 8;
const CELL_DARKEST_WEIGHT: f64 = 0.72;

#[derive(Debug, Clone, Copy)]
pub(crate) struct OrbGeometry {
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct DecodedOrb {
    pub scan_result: ScanResult,
    pub geometry: OrbGeometry,
}

pub fn scan_orb_image(image_bytes: &[u8]) -> Result<ScanResult> {
    Ok(scan_orb_image_with_geometry(image_bytes)?.scan_result)
}

pub(crate) fn scan_orb_image_with_geometry(image_bytes: &[u8]) -> Result<DecodedOrb> {
    let grayscale_image = image::load_from_memory(image_bytes)?.to_luma8();
    let geometry = detect_orb_geometry(&grayscale_image)?;
    try_decode_near_geometry(&grayscale_image, geometry)
}

pub fn scan_orb_image_from_luma8(data: &[u8], width: u32, height: u32) -> Result<ScanResult> {
    Ok(scan_orb_image_from_luma8_with_geometry(data, width, height)?.scan_result)
}

pub(crate) fn scan_orb_image_from_luma8_with_geometry(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<DecodedOrb> {
    // Tightly-packed luma buffer: width*height bytes, one byte per pixel, no padding.
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .ok_or(OrbError::MalformedPayload)?;
    if width == 0 || height == 0 || data.len() != expected_len {
        return Err(OrbError::MalformedPayload);
    }
    let grayscale_image =
        GrayImage::from_raw(width, height, data.to_vec()).ok_or(OrbError::MalformedPayload)?;
    let geometry = detect_orb_geometry(&grayscale_image)?;
    try_decode_near_geometry(&grayscale_image, geometry)
}

fn detect_orb_geometry(image: &GrayImage) -> Result<OrbGeometry> {
    let max_dimension = image.width().max(image.height());
    let scale = (f64::from(DETECTION_TARGET_MAX_DIMENSION) / f64::from(max_dimension)).min(1.0);
    let detection_image = if scale < 1.0 {
        let scaled_width = (f64::from(image.width()) * scale).round().max(1.0) as u32;
        let scaled_height = (f64::from(image.height()) * scale).round().max(1.0) as u32;
        image::imageops::resize(image, scaled_width, scaled_height, FilterType::Triangle)
    } else {
        image.clone()
    };

    let coarse = search_best_circle(
        &detection_image,
        COARSE_CENTER_STEP,
        COARSE_RADIUS_STEP,
        COARSE_SAMPLE_COUNT,
    )
    .filter(|candidate| candidate.score >= MINIMUM_DETECTION_SCORE)
    .ok_or(OrbError::OrbNotFound)?;

    let refined_small = refine_circle(&detection_image, coarse, 10.0, 5.0, FINE_SAMPLE_COUNT)
        .ok_or(OrbError::OrbNotFound)?;
    let coarse_scale = 1.0 / scale;
    let refined_full = CircleCandidate {
        center_x: refined_small.center_x * coarse_scale,
        center_y: refined_small.center_y * coarse_scale,
        radius: refined_small.radius * coarse_scale,
        score: refined_small.score,
    };

    let full_resolution = refine_circle(image, refined_full, 12.0, 6.0, FINE_SAMPLE_COUNT)
        .ok_or(OrbError::OrbNotFound)?;
    if full_resolution.score < MINIMUM_DETECTION_SCORE {
        return Err(OrbError::OrbNotFound);
    }

    Ok(OrbGeometry {
        center_x: full_resolution.center_x,
        center_y: full_resolution.center_y,
        radius: full_resolution.radius,
    })
}

#[derive(Debug, Clone, Copy)]
struct CircleCandidate {
    center_x: f64,
    center_y: f64,
    radius: f64,
    score: f64,
}

fn search_best_circle(
    image: &GrayImage,
    center_step: usize,
    radius_step: usize,
    sample_count: usize,
) -> Option<CircleCandidate> {
    let minimum_dimension = f64::from(image.width().min(image.height()));
    let minimum_radius = minimum_dimension * MINIMUM_RADIUS_RATIO;
    let maximum_radius = minimum_dimension * MAXIMUM_RADIUS_RATIO;
    let mut best_candidate: Option<CircleCandidate> = None;

    let minimum_center_x = minimum_radius.round() as usize;
    let minimum_center_y = minimum_radius.round() as usize;
    let maximum_center_x = image.width().saturating_sub(minimum_radius.round() as u32) as usize;
    let maximum_center_y = image.height().saturating_sub(minimum_radius.round() as u32) as usize;

    let minimum_radius_int = minimum_radius.round() as usize;
    let maximum_radius_int = maximum_radius.round() as usize;

    for center_y in (minimum_center_y..maximum_center_y).step_by(center_step) {
        for center_x in (minimum_center_x..maximum_center_x).step_by(center_step) {
            for radius in (minimum_radius_int..maximum_radius_int).step_by(radius_step) {
                let radius = radius as f64;
                let score = circle_score(
                    image,
                    center_x as f64,
                    center_y as f64,
                    radius,
                    sample_count,
                );
                let candidate = CircleCandidate {
                    center_x: center_x as f64,
                    center_y: center_y as f64,
                    radius,
                    score,
                };
                if best_candidate
                    .map(|current_best| candidate.score > current_best.score)
                    .unwrap_or(true)
                {
                    best_candidate = Some(candidate);
                }
            }
        }
    }

    best_candidate
}

fn refine_circle(
    image: &GrayImage,
    seed: CircleCandidate,
    center_window: f64,
    radius_window: f64,
    sample_count: usize,
) -> Option<CircleCandidate> {
    let mut best_candidate = None;
    let min_radius = (seed.radius - radius_window).max(8.0).round() as i32;
    let max_radius = (seed.radius + radius_window).round() as i32;
    for center_y in (seed.center_y.round() as i32 - center_window.round() as i32)
        ..=(seed.center_y.round() as i32 + center_window.round() as i32)
    {
        for center_x in (seed.center_x.round() as i32 - center_window.round() as i32)
            ..=(seed.center_x.round() as i32 + center_window.round() as i32)
        {
            if center_x <= 0
                || center_y <= 0
                || center_x >= image.width() as i32
                || center_y >= image.height() as i32
            {
                continue;
            }
            for radius in min_radius..=max_radius {
                if radius <= 0 {
                    continue;
                }
                let score = circle_score(
                    image,
                    f64::from(center_x),
                    f64::from(center_y),
                    f64::from(radius),
                    sample_count,
                );
                let candidate = CircleCandidate {
                    center_x: f64::from(center_x),
                    center_y: f64::from(center_y),
                    radius: f64::from(radius),
                    score,
                };
                if best_candidate
                    .map(|current_best: CircleCandidate| candidate.score > current_best.score)
                    .unwrap_or(true)
                {
                    best_candidate = Some(candidate);
                }
            }
        }
    }
    best_candidate
}

fn circle_score(
    image: &GrayImage,
    center_x: f64,
    center_y: f64,
    radius: f64,
    sample_count: usize,
) -> f64 {
    let mut inner_sum = 0.0;
    let mut edge_sum = 0.0;
    let mut outer_sum = 0.0;
    let mut payload_sum = 0.0;
    let mut payload_square_sum = 0.0;

    for sample_index in 0..sample_count {
        let angle = std::f64::consts::TAU * sample_index as f64 / sample_count as f64;
        let cos_angle = angle.cos();
        let sin_angle = angle.sin();
        let edge = sample_luma(
            image,
            center_x + cos_angle * radius * EDGE_SAMPLE_RADIUS_RATIO,
            center_y + sin_angle * radius * EDGE_SAMPLE_RADIUS_RATIO,
        );
        let inner = sample_luma(
            image,
            center_x + cos_angle * radius * INNER_SAMPLE_RADIUS_RATIO,
            center_y + sin_angle * radius * INNER_SAMPLE_RADIUS_RATIO,
        );
        let outer = sample_luma(
            image,
            center_x + cos_angle * radius * OUTER_SAMPLE_RADIUS_RATIO,
            center_y + sin_angle * radius * OUTER_SAMPLE_RADIUS_RATIO,
        );
        let payload = sample_luma(
            image,
            center_x + cos_angle * radius * PAYLOAD_VARIANCE_RADIUS_RATIO,
            center_y + sin_angle * radius * PAYLOAD_VARIANCE_RADIUS_RATIO,
        );

        inner_sum += inner;
        edge_sum += edge;
        outer_sum += outer;
        payload_sum += payload;
        payload_square_sum += payload * payload;
    }

    let sample_count = sample_count as f64;
    let ring_contrast = ((inner_sum + outer_sum) - edge_sum * 2.0) / sample_count;
    let payload_mean = payload_sum / sample_count;
    let payload_variance = (payload_square_sum / sample_count) - payload_mean * payload_mean;
    ring_contrast + payload_variance.max(0.0).sqrt() * 0.55
}

fn sample_marker_intensities(image: &GrayImage, geometry: OrbGeometry) -> [u8; TOTAL_CELL_COUNT] {
    let mut intensities = [0_u8; TOTAL_CELL_COUNT];
    for sector in 0..SECTOR_COUNT {
        for track in 0..TRACK_COUNT {
            intensities[sector * TRACK_COUNT + track] =
                sample_cell_intensity(image, geometry, sector, track);
        }
    }
    intensities
}

fn build_cells_from_intensities(
    intensities: &[u8; TOTAL_CELL_COUNT],
    threshold: u8,
) -> [u8; TOTAL_CELL_COUNT] {
    let mut cells = [0_u8; TOTAL_CELL_COUNT];
    for (index, intensity) in intensities.iter().copied().enumerate() {
        cells[index] = u8::from(intensity < threshold);
    }
    cells
}

fn try_decode_near_geometry(image: &GrayImage, seed_geometry: OrbGeometry) -> Result<DecodedOrb> {
    let center_offsets = [0.0, -2.0, 2.0, -4.0, 4.0];
    let radius_scales = [1.0, 0.99, 1.01, 0.97, 1.03];
    let threshold_offsets = [0_i16, -10, 10, -18, 18];
    let mut last_error = None;

    for &offset_y in &center_offsets {
        for &offset_x in &center_offsets {
            for &radius_scale in &radius_scales {
                let candidate_geometry = OrbGeometry {
                    center_x: seed_geometry.center_x + offset_x,
                    center_y: seed_geometry.center_y + offset_y,
                    radius: seed_geometry.radius * radius_scale,
                };
                let intensities = sample_marker_intensities(image, candidate_geometry);
                let base_threshold = i16::from(otsu_threshold(&intensities));
                for &threshold_offset in &threshold_offsets {
                    let threshold = (base_threshold + threshold_offset).clamp(0, 255) as u8;
                    let marker_cells = build_cells_from_intensities(&intensities, threshold);
                    match decode_marker_cells(&marker_cells) {
                        Ok(decoded_payload) => {
                            return Ok(DecodedOrb {
                                scan_result: ScanResult {
                                    orb_id: decoded_payload.orb_id,
                                    version: decoded_payload.version,
                                },
                                geometry: candidate_geometry,
                            });
                        }
                        Err(error) => last_error = Some(error),
                    }
                }
            }
        }
    }

    Err(last_error.unwrap_or(OrbError::PayloadDecode {
        details: "geometry search exhausted".to_string(),
    }))
}

fn sample_cell_intensity(
    image: &GrayImage,
    geometry: OrbGeometry,
    sector: usize,
    track: usize,
) -> u8 {
    let sector_width = std::f64::consts::TAU / SECTOR_COUNT as f64;
    let center_angle = (sector as f64 + 0.5) * sector_width;
    let radius_ratio = payload_track_radius_ratio(track);
    let track_height_ratio = payload_track_height_ratio();
    let radial_span = track_height_ratio * 0.5;
    let mut samples = [0.0; CELL_ANGLE_SAMPLE_OFFSETS.len() * CELL_RADIAL_SAMPLE_OFFSETS.len()];
    let mut sample_count = 0;

    for angle_offset in CELL_ANGLE_SAMPLE_OFFSETS {
        let angle = center_angle + sector_width * angle_offset;
        for radial_offset in CELL_RADIAL_SAMPLE_OFFSETS {
            let radius = geometry.radius * (radius_ratio + radial_span * radial_offset);
            let sample_x = geometry.center_x + radius * angle.cos();
            let sample_y = geometry.center_y + radius * angle.sin();
            samples[sample_count] = sample_luma(image, sample_x, sample_y);
            sample_count += 1;
        }
    }

    let all_samples = &mut samples[..sample_count];
    all_samples.sort_by(f64::total_cmp);

    let darkest_sum = all_samples
        .iter()
        .take(CELL_DARKEST_SAMPLE_COUNT)
        .sum::<f64>();
    let overall_sum = all_samples.iter().sum::<f64>();
    let darkest_mean = darkest_sum / CELL_DARKEST_SAMPLE_COUNT as f64;
    let overall_mean = overall_sum / sample_count as f64;

    (darkest_mean * CELL_DARKEST_WEIGHT + overall_mean * (1.0 - CELL_DARKEST_WEIGHT)).round() as u8
}

fn payload_track_height_ratio() -> f64 {
    (PAYLOAD_RING_OUTER_RATIO - PAYLOAD_RING_INNER_RATIO) / TRACK_COUNT as f64
}

fn sample_luma(image: &GrayImage, x: f64, y: f64) -> f64 {
    let clamped_x = x.clamp(0.0, f64::from(image.width().saturating_sub(1)));
    let clamped_y = y.clamp(0.0, f64::from(image.height().saturating_sub(1)));
    let left = clamped_x.floor() as u32;
    let top = clamped_y.floor() as u32;
    let right = (left + 1).min(image.width().saturating_sub(1));
    let bottom = (top + 1).min(image.height().saturating_sub(1));
    let x_fraction = clamped_x - f64::from(left);
    let y_fraction = clamped_y - f64::from(top);

    let top_left = f64::from(image.get_pixel(left, top)[0]);
    let top_right = f64::from(image.get_pixel(right, top)[0]);
    let bottom_left = f64::from(image.get_pixel(left, bottom)[0]);
    let bottom_right = f64::from(image.get_pixel(right, bottom)[0]);

    let top_mix = top_left * (1.0 - x_fraction) + top_right * x_fraction;
    let bottom_mix = bottom_left * (1.0 - x_fraction) + bottom_right * x_fraction;
    top_mix * (1.0 - y_fraction) + bottom_mix * y_fraction
}

fn otsu_threshold(values: &[u8]) -> u8 {
    let mut histogram = [0_u32; 256];
    for &value in values {
        histogram[value as usize] += 1;
    }

    let total = values.len() as f64;
    let total_sum = histogram
        .iter()
        .enumerate()
        .map(|(value, count)| value as f64 * f64::from(*count))
        .sum::<f64>();

    let mut threshold = 128_u8;
    let mut best_variance = -1.0;
    let mut weight_background = 0.0;
    let mut sum_background = 0.0;

    for (value, count) in histogram.iter().enumerate() {
        weight_background += f64::from(*count);
        if weight_background == 0.0 {
            continue;
        }
        let weight_foreground = total - weight_background;
        if weight_foreground == 0.0 {
            break;
        }

        sum_background += value as f64 * f64::from(*count);
        let mean_background = sum_background / weight_background;
        let mean_foreground = (total_sum - sum_background) / weight_foreground;
        let between_class_variance =
            weight_background * weight_foreground * (mean_background - mean_foreground).powi(2);
        if between_class_variance > best_variance {
            best_variance = between_class_variance;
            threshold = value as u8;
        }
    }

    threshold
}
