use image::{Rgba, RgbaImage};

use crate::{
    OrbId, Result,
    payload::{SECTOR_COUNT, TRACK_COUNT, encode_marker_cells, sector_track_bit},
};

const BACKGROUND_BASE_TONE: f64 = 244.0;
const CAMERA_DISTANCE: f64 = 3.0;
const FOV_SCALE: f64 = 0.6;
const MAX_HARMONIC_ORDER: i32 = 4;
const DUST_PRIMARY_FREQUENCY: f64 = 17.0;
const DUST_SECONDARY_FREQUENCY: f64 = 21.0;
const DUST_TERTIARY_FREQUENCY: f64 = 13.5;
const DUST_PRIMARY_SHARPNESS: f64 = 26.0;
const DUST_SECONDARY_SHARPNESS: f64 = 22.0;
const DUST_TERTIARY_SHARPNESS: f64 = 18.0;
const DUST_EDGE_FADE_POWER: f64 = 0.72;
const DUST_CENTER_GLOW_POWER: f64 = 1.2;
const GLASS_BODY_STRENGTH: f64 = 0.14;
const INTERIOR_TRANSMISSION_STRENGTH: f64 = 0.82;
const ENVIRONMENT_REFLECTION_STRENGTH: f64 = 0.24;
const PAYLOAD_TRACK_EDGE_START: f64 = 0.16;
const PAYLOAD_TRACK_EDGE_END: f64 = 0.3;
const PAYLOAD_TRACK_TRAILING_EDGE_START: f64 = 0.7;
const PAYLOAD_TRACK_TRAILING_EDGE_END: f64 = 0.84;
const PRIMARY_SPECULAR_STRENGTH: f64 = 0.84;
const BLOOM_SPECULAR_STRENGTH: f64 = 0.3;
const FILL_SPECULAR_STRENGTH: f64 = 0.2;
const GLASS_BLUE_BOOST: f64 = 24.0;
const GLASS_GREEN_BOOST: f64 = 10.0;
const GLASS_RED_DROP: f64 = 10.0;

const ORB_CENTER_X_RATIO: f64 = 0.5;
const ORB_CENTER_Y_RATIO: f64 = 0.46;
const ORB_RADIUS_RATIO: f64 = 0.31;

pub const OUTER_SILHOUETTE_RING_INNER_RATIO: f64 = 0.957;
pub const OUTER_GUARD_RING_INNER_RATIO: f64 = 0.914;
pub const OUTER_GUARD_RING_OUTER_RATIO: f64 = 0.947;
pub const PAYLOAD_RING_INNER_RATIO: f64 = 0.752;
pub const PAYLOAD_RING_OUTER_RATIO: f64 = 0.848;
pub const INNER_SEPARATOR_RING_INNER_RATIO: f64 = 0.734;
pub const INNER_SEPARATOR_RING_OUTER_RATIO: f64 = 0.752;
pub const CORE_OUTER_RATIO: f64 = 0.728;
pub const CORE_FINGERPRINT_RADIUS_RATIO: f64 = 0.56;

const TRACK_GAP_RATIO: f64 = 0.02;

const SIGNAL_LIGHT: [u8; 3] = [240, 242, 245];
const SILHOUETTE_DARK: [u8; 3] = [104, 108, 116];
const SEPARATOR_LIGHT: [u8; 3] = [244, 245, 247];
const INACTIVE_PATTERN_SHADE: [u8; 3] = [232, 235, 239];
const OUTER_RIM_HIGHLIGHT: [u8; 3] = [156, 162, 171];
const GUARD_GLASS_SHADOW: [u8; 3] = [226, 229, 234];
const GUARD_GLASS_LIGHT: [u8; 3] = [248, 249, 251];
const PAYLOAD_GLASS_SHADOW: [u8; 3] = [228, 232, 237];
const PAYLOAD_GLASS_LIGHT: [u8; 3] = [245, 247, 249];
const ACTIVE_DUST_MID: [u8; 3] = [118, 123, 134];
const ACTIVE_DUST_DARK: [u8; 3] = [68, 72, 82];
const INACTIVE_DUST_LIGHT: [u8; 3] = [249, 250, 251];
const RING_SWEEP_FREQUENCY: f64 = 2.0;
const RING_SWEEP_PHASE: f64 = 0.9;
const RING_GLASS_INNER_REFLECTION_CENTER: f64 = 0.2;
const RING_GLASS_OUTER_REFLECTION_CENTER: f64 = 0.8;
const RING_GLASS_INNER_REFLECTION_WIDTH: f64 = 0.14;
const RING_GLASS_OUTER_REFLECTION_WIDTH: f64 = 0.1;
const ACTIVE_SPINE_WIDTH: f64 = 0.032;
const ACTIVE_SPINE_FEATHER: f64 = 0.026;
const ACTIVE_PARTICLE_WIDTH: f64 = 0.018;
const ACTIVE_PARTICLE_FEATHER: f64 = 0.022;

#[derive(Debug, Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

impl Vec3 {
    const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    fn scale(self, scalar: f64) -> Self {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }

    fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn normalize(self) -> Self {
        let length = self.dot(self).sqrt();
        if length == 0.0 {
            return Self::new(0.0, 0.0, 0.0);
        }
        Self::new(self.x / length, self.y / length, self.z / length)
    }
}

#[derive(Debug, Clone, Copy)]
struct HarmonicCoefficient {
    degree: i32,
    order: i32,
    amplitude: f64,
}

pub fn render_orb_card(orb_id: &OrbId, image_size: u32) -> Result<RgbaImage> {
    let marker_cells = encode_marker_cells(orb_id)?;
    let coefficients = generate_harmonic_coefficients(orb_id.seed_bytes());
    let mut image = RgbaImage::new(image_size, image_size);
    let (center_x, center_y, orb_radius) = orb_layout(image_size);
    let camera = Vec3::new(0.0, 0.0, CAMERA_DISTANCE);
    let sphere_center = Vec3::new(0.0, 0.0, 0.0);

    for pixel_y in 0..image_size {
        for pixel_x in 0..image_size {
            let mut rgba = render_background(pixel_x, pixel_y, center_x, center_y, orb_radius);
            let dx = ((f64::from(pixel_x) + 0.5) - center_x) / orb_radius;
            let dy = ((f64::from(pixel_y) + 0.5) - center_y) / orb_radius;
            let normalized_radius = (dx * dx + dy * dy).sqrt();

            if normalized_radius > 1.0 {
                image.put_pixel(pixel_x, pixel_y, rgba);
                continue;
            }

            rgba = render_signal_zone(dx, dy, normalized_radius, &marker_cells);
            if normalized_radius <= CORE_OUTER_RATIO {
                let ray_direction = Vec3::new(dx * FOV_SCALE, -dy * FOV_SCALE, -1.0).normalize();
                if let Some((near_hit, _)) = ray_sphere(camera, ray_direction, sphere_center, 1.0)
                    .filter(|(near, _)| *near > 0.0)
                {
                    let surface_point = camera.add(ray_direction.scale(near_hit));
                    let surface_normal = surface_point.normalize();
                    let theta = clamp(surface_normal.y, -1.0, 1.0).acos();
                    let phi = surface_normal.z.atan2(surface_normal.x);
                    rgba = render_core_pixel(CorePixelSample {
                        surface_point,
                        surface_normal,
                        theta,
                        phi,
                        camera,
                        ray_direction,
                        coefficients: &coefficients,
                        normalized_core_radius: normalized_radius / CORE_OUTER_RATIO,
                    });
                }
            }

            image.put_pixel(pixel_x, pixel_y, rgba);
        }
    }

    Ok(image)
}

pub fn orb_layout(image_size: u32) -> (f64, f64, f64) {
    (
        f64::from(image_size) * ORB_CENTER_X_RATIO,
        f64::from(image_size) * ORB_CENTER_Y_RATIO,
        f64::from(image_size) * ORB_RADIUS_RATIO,
    )
}

pub fn payload_track_radius_ratio(track_index: usize) -> f64 {
    let total_track_height =
        PAYLOAD_RING_OUTER_RATIO - PAYLOAD_RING_INNER_RATIO - TRACK_GAP_RATIO * 2.0;
    let track_height = total_track_height / TRACK_COUNT as f64;
    let outer_edge =
        PAYLOAD_RING_OUTER_RATIO - (track_index as f64) * (track_height + TRACK_GAP_RATIO);
    outer_edge - track_height * 0.5
}

pub fn core_fingerprint_radius(orb_radius: f64) -> f64 {
    orb_radius * CORE_FINGERPRINT_RADIUS_RATIO
}

fn render_background(
    pixel_x: u32,
    pixel_y: u32,
    center_x: f64,
    center_y: f64,
    orb_radius: f64,
) -> Rgba<u8> {
    let normalized_x = (f64::from(pixel_x) / (center_x * 2.0) - 0.5) * 2.0;
    let normalized_y = (f64::from(pixel_y) / (center_y * 2.1739130435) - 0.5) * 2.0;
    let vignette_distance = (normalized_x * normalized_x + normalized_y * normalized_y).sqrt();
    let vignette = (vignette_distance - 0.55).max(0.0) * 18.0;

    let shadow_center_y = center_y + orb_radius * 1.58;
    let shadow_dx = ((f64::from(pixel_x) + 0.5) - center_x) / (orb_radius * 0.98);
    let shadow_dy = ((f64::from(pixel_y) + 0.5) - shadow_center_y) / (orb_radius * 0.15);
    let shadow_strength = (-2.8 * (shadow_dx * shadow_dx + shadow_dy * shadow_dy)).exp() * 24.0;
    let brightness = clamp(
        BACKGROUND_BASE_TONE - vignette - shadow_strength,
        214.0,
        248.0,
    );
    let channel = brightness.round() as u8;
    Rgba([channel, channel, channel.saturating_add(1), 255])
}

fn render_signal_zone(
    local_x: f64,
    local_y: f64,
    normalized_radius: f64,
    marker_cells: &[u8; SECTOR_COUNT * TRACK_COUNT],
) -> Rgba<u8> {
    let angle = local_y.atan2(local_x);
    let normalized_angle = if angle < 0.0 {
        angle + std::f64::consts::TAU
    } else {
        angle
    };
    let angular_sweep =
        ((normalized_angle * RING_SWEEP_FREQUENCY - RING_SWEEP_PHASE).cos() * 0.5 + 0.5).powf(2.4);

    if normalized_radius >= OUTER_SILHOUETTE_RING_INNER_RATIO {
        let silhouette_local =
            normalize_range(normalized_radius, OUTER_SILHOUETTE_RING_INNER_RATIO, 1.0);
        let rim_glass = clamp(
            0.12 + soft_band(
                silhouette_local,
                0.26,
                RING_GLASS_INNER_REFLECTION_WIDTH,
                0.08,
            ) * 0.32
                + angular_sweep * 0.08,
            0.0,
            1.0,
        );
        return rgba_from_rgb(blend_rgb(SILHOUETTE_DARK, OUTER_RIM_HIGHLIGHT, rim_glass));
    }
    if (OUTER_GUARD_RING_INNER_RATIO..OUTER_GUARD_RING_OUTER_RATIO).contains(&normalized_radius) {
        let guard_local = normalize_range(
            normalized_radius,
            OUTER_GUARD_RING_INNER_RATIO,
            OUTER_GUARD_RING_OUTER_RATIO,
        );
        let guard_glass = blend_rgb(
            GUARD_GLASS_SHADOW,
            GUARD_GLASS_LIGHT,
            glass_band_mix(guard_local, angular_sweep),
        );
        return rgba_from_rgb(guard_glass);
    }
    if (INNER_SEPARATOR_RING_INNER_RATIO..INNER_SEPARATOR_RING_OUTER_RATIO)
        .contains(&normalized_radius)
    {
        let separator_local = normalize_range(
            normalized_radius,
            INNER_SEPARATOR_RING_INNER_RATIO,
            INNER_SEPARATOR_RING_OUTER_RATIO,
        );
        let separator_glass = blend_rgb(
            PAYLOAD_GLASS_SHADOW,
            SIGNAL_LIGHT,
            glass_band_mix(separator_local, angular_sweep),
        );
        return rgba_from_rgb(separator_glass);
    }
    if !(PAYLOAD_RING_INNER_RATIO..PAYLOAD_RING_OUTER_RATIO).contains(&normalized_radius) {
        return rgba_from_rgb(SEPARATOR_LIGHT);
    }

    let sector_position = (normalized_angle / std::f64::consts::TAU) * SECTOR_COUNT as f64;
    let sector = sector_position.floor() as usize % SECTOR_COUNT;
    let sector_local = sector_position - sector_position.floor();

    for track in 0..TRACK_COUNT {
        let (track_inner, track_outer) = payload_track_bounds(track);
        if (track_inner..track_outer).contains(&normalized_radius) {
            let bit = sector_track_bit(marker_cells, sector, track);
            let track_local = (normalized_radius - track_inner) / (track_outer - track_inner);
            let base_glass = blend_rgb(
                PAYLOAD_GLASS_SHADOW,
                PAYLOAD_GLASS_LIGHT,
                glass_band_mix(track_local, angular_sweep),
            );
            let glyph_height = plateau_mask(
                track_local,
                PAYLOAD_TRACK_EDGE_START,
                PAYLOAD_TRACK_EDGE_END,
                PAYLOAD_TRACK_TRAILING_EDGE_START,
                PAYLOAD_TRACK_TRAILING_EDGE_END,
            );
            let glyph_center =
                soft_band(sector_local, 0.5, ACTIVE_SPINE_WIDTH, ACTIVE_SPINE_FEATHER)
                    * glyph_height;
            let glyph_left_rail = soft_band(
                sector_local,
                0.36,
                ACTIVE_PARTICLE_WIDTH,
                ACTIVE_PARTICLE_FEATHER,
            ) * soft_band(track_local, 0.36, 0.055, 0.05)
                * 0.52;
            let glyph_right_rail = soft_band(
                sector_local,
                0.64,
                ACTIVE_PARTICLE_WIDTH,
                ACTIVE_PARTICLE_FEATHER,
            ) * soft_band(track_local, 0.62, 0.06, 0.05)
                * 0.46;
            let glyph_cap_top = soft_band(track_local, 0.22, 0.04, 0.04)
                * soft_band(sector_local, 0.48, 0.05, 0.05)
                * 0.26;
            let glyph_cap_bottom = soft_band(track_local, 0.78, 0.045, 0.04)
                * soft_band(sector_local, 0.52, 0.04, 0.05)
                * 0.22;
            let active_pattern = clamp(
                glyph_center
                    + glyph_left_rail
                    + glyph_right_rail
                    + glyph_cap_top
                    + glyph_cap_bottom,
                0.0,
                1.0,
            );
            let inactive_pattern = clamp(
                glyph_left_rail * 0.22 + glyph_right_rail * 0.22 + angular_sweep * 0.06,
                0.0,
                0.28,
            );

            return if bit == 1 {
                let dust_mid = blend_rgb(base_glass, ACTIVE_DUST_MID, active_pattern * 0.62);
                rgba_from_rgb(blend_rgb(dust_mid, ACTIVE_DUST_DARK, active_pattern * 0.72))
            } else {
                let inactive_dust =
                    blend_rgb(base_glass, INACTIVE_PATTERN_SHADE, inactive_pattern * 0.42);
                rgba_from_rgb(blend_rgb(
                    inactive_dust,
                    INACTIVE_DUST_LIGHT,
                    inactive_pattern * 0.34,
                ))
            };
        }
    }

    rgba_from_rgb(SEPARATOR_LIGHT)
}

fn payload_track_bounds(track_index: usize) -> (f64, f64) {
    let total_track_height =
        PAYLOAD_RING_OUTER_RATIO - PAYLOAD_RING_INNER_RATIO - TRACK_GAP_RATIO * 2.0;
    let track_height = total_track_height / TRACK_COUNT as f64;
    let outer_edge =
        PAYLOAD_RING_OUTER_RATIO - (track_index as f64) * (track_height + TRACK_GAP_RATIO);
    let inner_edge = outer_edge - track_height;
    (inner_edge, outer_edge)
}

struct CorePixelSample<'a> {
    surface_point: Vec3,
    surface_normal: Vec3,
    theta: f64,
    phi: f64,
    camera: Vec3,
    ray_direction: Vec3,
    coefficients: &'a [HarmonicCoefficient],
    normalized_core_radius: f64,
}

fn render_core_pixel(sample: CorePixelSample<'_>) -> Rgba<u8> {
    let CorePixelSample {
        surface_point,
        surface_normal,
        theta,
        phi,
        camera,
        ray_direction,
        coefficients,
        normalized_core_radius,
    } = sample;
    let view_direction = camera.sub(surface_point).normalize();
    let fresnel = (1.0 - surface_normal.dot(view_direction).abs()).powi(4);
    let reflection = surface_normal
        .scale(2.0 * surface_normal.dot(view_direction))
        .sub(view_direction);
    let environment = 0.36 + reflection.y.max(-0.35) * 0.26;

    let refracted_direction = refract_into_sphere(ray_direction, surface_normal, 1.45);
    let inside_point = surface_point.add(refracted_direction.scale(0.0025));
    let mut interior = 0.1;
    if let Some((_, far_hit)) = ray_sphere(
        inside_point,
        refracted_direction,
        Vec3::new(0.0, 0.0, 0.0),
        1.0,
    )
    .filter(|(_, far)| *far > 0.0)
    {
        let steps = 8;
        let mut accumulated = 0.0;
        for step in 0..steps {
            let travel = (f64::from(step) + 0.5) / f64::from(steps) * far_hit;
            let sample_point = inside_point.add(refracted_direction.scale(travel));
            let radius = sample_point.dot(sample_point).sqrt();
            let sample_theta = clamp(sample_point.y / (radius + 1e-9), -1.0, 1.0).acos();
            let sample_phi = sample_point.z.atan2(sample_point.x);
            let harmonic = evaluate_harmonics(coefficients, sample_theta, sample_phi);
            accumulated += harmonic.tanh();
        }
        interior = 0.22 + accumulated / f64::from(steps) * 0.16;
    }

    let surface_value = evaluate_harmonics(coefficients, theta, phi);
    let harmonic_offset_a = evaluate_harmonics(coefficients, theta + 0.14, phi - 0.11);
    let harmonic_offset_b = evaluate_harmonics(coefficients, theta - 0.1, phi + 0.16);
    let ridge_gradient = (surface_value - evaluate_harmonics(coefficients, theta + 0.018, phi))
        .abs()
        + (surface_value - evaluate_harmonics(coefficients, theta, phi + 0.018)).abs();
    let dust_phase_primary = surface_value * DUST_PRIMARY_FREQUENCY + phi * 11.0 + theta * 8.5;
    let dust_phase_secondary =
        harmonic_offset_a * DUST_SECONDARY_FREQUENCY - phi * 13.5 + theta * 9.0;
    let dust_phase_tertiary =
        harmonic_offset_b * DUST_TERTIARY_FREQUENCY + phi * 7.5 - theta * 12.0;
    let dust_primary = thin_filament(dust_phase_primary, DUST_PRIMARY_SHARPNESS);
    let dust_secondary = thin_filament(dust_phase_secondary, DUST_SECONDARY_SHARPNESS);
    let dust_tertiary = thin_filament(dust_phase_tertiary, DUST_TERTIARY_SHARPNESS);
    let dust_window = (1.0 - normalized_core_radius.powf(1.28))
        .max(0.0)
        .powf(DUST_EDGE_FADE_POWER);
    let dust_emphasis = clamp(ridge_gradient * 2.2 + 0.08, 0.0, 1.0);
    let space_dust = (dust_primary * dust_secondary
        + dust_secondary * dust_tertiary * 0.8
        + dust_primary * dust_tertiary * 0.45)
        * dust_window
        * dust_emphasis;
    let center_glow = (1.0 - normalized_core_radius)
        .max(0.0)
        .powf(DUST_CENTER_GLOW_POWER);

    let key_light = Vec3::new(-0.35, 0.78, 0.52).normalize();
    let half_vector = key_light.add(view_direction).normalize();
    let primary_specular =
        surface_normal.dot(half_vector).max(0.0).powi(160) * PRIMARY_SPECULAR_STRENGTH;
    let bloom_specular =
        surface_normal.dot(half_vector).max(0.0).powi(30) * BLOOM_SPECULAR_STRENGTH;

    let fill_light = Vec3::new(0.52, -0.12, 0.84).normalize();
    let fill_half_vector = fill_light.add(view_direction).normalize();
    let fill_specular =
        surface_normal.dot(fill_half_vector).max(0.0).powi(44) * FILL_SPECULAR_STRENGTH;

    let edge_depth = normalized_core_radius.powf(1.6);
    let absorption = (1.0 - edge_depth).powf(1.25) * 0.09;
    let rim = fresnel * 0.58;

    let mut luminance = 0.09;
    luminance += environment * ENVIRONMENT_REFLECTION_STRENGTH;
    luminance += (GLASS_BODY_STRENGTH + interior.max(0.0)) * INTERIOR_TRANSMISSION_STRENGTH;
    luminance += center_glow * 0.08;
    luminance += space_dust * 0.12;
    luminance += primary_specular;
    luminance += bloom_specular;
    luminance += fill_specular;
    luminance += rim;
    luminance -= absorption;
    luminance = clamp(luminance, 0.0, 1.0).powf(0.91);

    let dust_tint = space_dust * GLASS_BLUE_BOOST;
    let glow_tint = center_glow * GLASS_GREEN_BOOST;
    let red_channel = clamp(luminance * 255.0 - GLASS_RED_DROP, 0.0, 255.0).round() as u8;
    let green_channel = clamp(luminance * 255.0 + glow_tint, 0.0, 255.0).round() as u8;
    let blue_channel = clamp(luminance * 255.0 + dust_tint, 0.0, 255.0).round() as u8;
    Rgba([red_channel, green_channel, blue_channel, 255])
}

fn generate_harmonic_coefficients(seed: [u8; 16]) -> Vec<HarmonicCoefficient> {
    let mut state_bytes = [0_u8; 8];
    state_bytes.copy_from_slice(&seed[..8]);
    let mut state = u64::from_be_bytes(state_bytes);
    let coefficient_count = (1..=MAX_HARMONIC_ORDER)
        .map(|degree| (degree * 2 + 1) as usize)
        .sum();
    let mut coefficients = Vec::with_capacity(coefficient_count);
    for degree in 1..=MAX_HARMONIC_ORDER {
        for order in -degree..=degree {
            let random = splitmix64(&mut state);
            let decay = 1.0 / (1.0 + f64::from(degree) * 0.42);
            let amplitude = (random * 2.0 - 1.0) * decay;
            coefficients.push(HarmonicCoefficient {
                degree,
                order,
                amplitude,
            });
        }
    }
    coefficients
}

fn splitmix64(state: &mut u64) -> f64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    f64::from((value & u32::MAX as u64) as u32) / f64::from(u32::MAX)
}

fn evaluate_harmonics(coefficients: &[HarmonicCoefficient], theta: f64, phi: f64) -> f64 {
    coefficients.iter().fold(0.0, |value, coefficient| {
        value
            + coefficient.amplitude
                * real_spherical_harmonic(coefficient.degree, coefficient.order, theta, phi)
    })
}

fn real_spherical_harmonic(degree: i32, order: i32, theta: f64, phi: f64) -> f64 {
    let cos_theta = theta.cos();
    let sin_theta = theta.sin();
    match degree {
        1 => match order {
            -1 => sin_theta * phi.sin(),
            0 => cos_theta,
            1 => sin_theta * phi.cos(),
            _ => 0.0,
        },
        2 => match order {
            -2 => sin_theta * sin_theta * (2.0 * phi).sin(),
            -1 => sin_theta * cos_theta * phi.sin(),
            0 => 3.0 * cos_theta * cos_theta - 1.0,
            1 => sin_theta * cos_theta * phi.cos(),
            2 => sin_theta * sin_theta * (2.0 * phi).cos(),
            _ => 0.0,
        },
        3 => match order {
            -3 => sin_theta.powi(3) * (3.0 * phi).sin(),
            -2 => sin_theta.powi(2) * cos_theta * (2.0 * phi).sin(),
            -1 => sin_theta * (5.0 * cos_theta * cos_theta - 1.0) * phi.sin(),
            0 => 5.0 * cos_theta.powi(3) - 3.0 * cos_theta,
            1 => sin_theta * (5.0 * cos_theta * cos_theta - 1.0) * phi.cos(),
            2 => sin_theta.powi(2) * cos_theta * (2.0 * phi).cos(),
            3 => sin_theta.powi(3) * (3.0 * phi).cos(),
            _ => 0.0,
        },
        4 => {
            let sin_squared = sin_theta * sin_theta;
            let cos_squared = cos_theta * cos_theta;
            match order {
                -4 => sin_squared * sin_squared * (4.0 * phi).sin(),
                -3 => sin_squared * sin_theta * cos_theta * (3.0 * phi).sin(),
                -2 => sin_squared * (7.0 * cos_squared - 1.0) * (2.0 * phi).sin(),
                -1 => sin_theta * (7.0 * cos_squared * cos_theta - 3.0 * cos_theta) * phi.sin(),
                0 => 35.0 * cos_squared * cos_squared - 30.0 * cos_squared + 3.0,
                1 => sin_theta * (7.0 * cos_squared * cos_theta - 3.0 * cos_theta) * phi.cos(),
                2 => sin_squared * (7.0 * cos_squared - 1.0) * (2.0 * phi).cos(),
                3 => sin_squared * sin_theta * cos_theta * (3.0 * phi).cos(),
                4 => sin_squared * sin_squared * (4.0 * phi).cos(),
                _ => 0.0,
            }
        }
        _ => 0.0,
    }
}

fn ray_sphere(origin: Vec3, direction: Vec3, center: Vec3, radius: f64) -> Option<(f64, f64)> {
    let origin_to_center = origin.sub(center);
    let b = origin_to_center.dot(direction);
    let c = origin_to_center.dot(origin_to_center) - radius * radius;
    let discriminant = b * b - c;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    Some((-b - root, -b + root))
}

fn refract_into_sphere(incident: Vec3, normal: Vec3, index_of_refraction: f64) -> Vec3 {
    let cos_incident = -incident.dot(normal);
    let eta = 1.0 / index_of_refraction;
    let k = 1.0 - eta * eta * (1.0 - cos_incident * cos_incident);
    if k < 0.0 {
        return incident;
    }
    incident
        .scale(eta)
        .add(normal.scale(eta * cos_incident - k.sqrt()))
        .normalize()
}

fn thin_filament(phase: f64, sharpness: f64) -> f64 {
    (1.0 - phase.sin().abs()).max(0.0).powf(sharpness)
}

fn plateau_mask(
    value: f64,
    leading_start: f64,
    leading_end: f64,
    trailing_start: f64,
    trailing_end: f64,
) -> f64 {
    smoothstep(leading_start, leading_end, value)
        * (1.0 - smoothstep(trailing_start, trailing_end, value))
}

fn soft_band(value: f64, center: f64, half_width: f64, feather: f64) -> f64 {
    let leading_start = center - half_width - feather;
    let leading_end = center - half_width;
    let trailing_start = center + half_width;
    let trailing_end = center + half_width + feather;
    plateau_mask(
        value,
        leading_start,
        leading_end,
        trailing_start,
        trailing_end,
    )
}

fn glass_band_mix(radial_local: f64, angular_sweep: f64) -> f64 {
    clamp(
        0.36 + soft_band(
            radial_local,
            RING_GLASS_INNER_REFLECTION_CENTER,
            RING_GLASS_INNER_REFLECTION_WIDTH,
            0.08,
        ) * 0.28
            + soft_band(
                radial_local,
                RING_GLASS_OUTER_REFLECTION_CENTER,
                RING_GLASS_OUTER_REFLECTION_WIDTH,
                0.07,
            ) * 0.2
            + angular_sweep * 0.1,
        0.0,
        1.0,
    )
}

fn normalize_range(value: f64, minimum: f64, maximum: f64) -> f64 {
    if maximum <= minimum {
        return 0.0;
    }
    clamp((value - minimum) / (maximum - minimum), 0.0, 1.0)
}

fn smoothstep(edge_start: f64, edge_end: f64, value: f64) -> f64 {
    if edge_start == edge_end {
        return if value < edge_start { 0.0 } else { 1.0 };
    }
    let t = clamp((value - edge_start) / (edge_end - edge_start), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blend_rgb(from: [u8; 3], to: [u8; 3], amount: f64) -> [u8; 3] {
    let amount = clamp(amount, 0.0, 1.0);
    let blend_channel = |start: u8, end: u8| {
        (f64::from(start) + (f64::from(end) - f64::from(start)) * amount).round() as u8
    };
    [
        blend_channel(from[0], to[0]),
        blend_channel(from[1], to[1]),
        blend_channel(from[2], to[2]),
    ]
}

fn rgba_from_rgb(rgb: [u8; 3]) -> Rgba<u8> {
    Rgba([rgb[0], rgb[1], rgb[2], 255])
}

fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    value.max(minimum).min(maximum)
}
