pub mod caustic;
mod error;
mod ffi;
mod id;
mod payload;
mod render;
mod scan;
mod types;
mod verify;

pub use error::{OrbError, Result};
pub use id::{OrbId, derive_orb_id};
pub use scan::{scan_orb_image, scan_orb_image_from_luma8};
pub use types::{
    DEFAULT_IMAGE_SIZE, DEFAULT_VERIFY_THRESHOLD, GenerateOrbRequest, OrbImage, ScanResult,
    VerificationResult,
};
pub use verify::verify_orb_image;

use render::render_orb_card;

pub fn generate_orb_image(request: &GenerateOrbRequest) -> Result<OrbImage> {
    let image = render_orb_card(&request.orb_id, request.image_size)?;
    Ok(OrbImage::new(request.orb_id.clone(), image))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage, imageops};

    use crate::{
        DEFAULT_IMAGE_SIZE, DEFAULT_VERIFY_THRESHOLD, GenerateOrbRequest, derive_orb_id,
        generate_orb_image, scan_orb_image, scan_orb_image_from_luma8, verify_orb_image,
    };

    #[test]
    fn derive_orb_id_is_stable() {
        let orb_id = derive_orb_id("https://example.com");
        assert_eq!(orb_id.as_str(), "orb1_100680ad546ce6a577f42f52df33b4cf");
    }

    #[test]
    fn generate_scan_and_verify_round_trip() {
        let request = GenerateOrbRequest::new(derive_orb_id("https://example.com"));
        let orb_image = generate_orb_image(&request).expect("generate orb image");
        let png_bytes = orb_image.to_png_bytes().expect("encode png");

        let scan_result = scan_orb_image(&png_bytes).expect("scan orb image");
        assert_eq!(scan_result.orb_id, request.orb_id);

        let verification_result = verify_orb_image(&png_bytes).expect("verify orb image");
        assert_eq!(verification_result.orb_id, request.orb_id);
        assert!(verification_result.is_match);
        assert!(verification_result.distance <= DEFAULT_VERIFY_THRESHOLD);
    }

    #[test]
    fn generated_png_round_trip_from_disk() {
        let request = GenerateOrbRequest::new(derive_orb_id("looper"));
        let orb_image = generate_orb_image(&request).expect("generate orb image");
        let temp_path = std::env::temp_dir().join(format!(
            "orb-code-round-trip-{}-{}.png",
            std::process::id(),
            request.orb_id.as_str()
        ));

        orb_image.save_png(&temp_path).expect("write orb image");
        let file_bytes = fs::read(&temp_path).expect("read orb image from disk");
        let scan_result = scan_orb_image(&file_bytes).expect("scan orb image from disk");
        fs::remove_file(temp_path).expect("remove temp file");

        assert_eq!(scan_result.orb_id, request.orb_id);
    }

    #[test]
    fn missing_payload_is_reported() {
        let blank = ImageBuffer::from_pixel(
            DEFAULT_IMAGE_SIZE,
            DEFAULT_IMAGE_SIZE,
            Rgba([255_u8, 255_u8, 255_u8, 255_u8]),
        );
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(blank)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .expect("encode blank png");

        let error = scan_orb_image(&png_bytes).expect_err("blank image should fail");
        assert!(matches!(error, crate::OrbError::OrbNotFound));
    }

    #[test]
    fn scan_survives_padding_and_rotation() {
        let request = GenerateOrbRequest::new(derive_orb_id("orb-padding"));
        let orb_image = generate_orb_image(&request).expect("generate orb image");
        let rotated = imageops::rotate90(&orb_image.image);
        let mut canvas = RgbaImage::from_pixel(1400, 1400, Rgba([248, 248, 248, 255]));
        imageops::overlay(&mut canvas, &rotated, 170, 110);

        let mut image_bytes = Vec::new();
        DynamicImage::ImageRgba8(canvas)
            .write_to(
                &mut std::io::Cursor::new(&mut image_bytes),
                image::ImageFormat::Png,
            )
            .expect("encode shifted png");

        let scan_result = scan_orb_image(&image_bytes).expect("scan rotated orb image");
        assert_eq!(scan_result.orb_id, request.orb_id);
    }

    #[test]
    fn scan_survives_blur_and_jpeg_recompression() {
        let request = GenerateOrbRequest::new(derive_orb_id("orb-jpeg"));
        let orb_image = generate_orb_image(&request).expect("generate orb image");
        let blurred = imageops::blur(&orb_image.image, 1.4);

        let mut image_bytes = Vec::new();
        DynamicImage::ImageRgba8(blurred)
            .write_to(
                &mut std::io::Cursor::new(&mut image_bytes),
                image::ImageFormat::Jpeg,
            )
            .expect("encode jpeg");

        let scan_result = scan_orb_image(&image_bytes).expect("scan blurred jpeg");
        assert_eq!(scan_result.orb_id, request.orb_id);
    }

    #[test]
    fn scan_from_luma8_round_trips_generated_orb() {
        let request = GenerateOrbRequest::new(derive_orb_id("orb-luma8"));
        let orb_image = generate_orb_image(&request).expect("generate orb image");
        let luma = image::DynamicImage::ImageRgba8(orb_image.image.clone()).to_luma8();
        let width = luma.width();
        let height = luma.height();

        let scan_result = scan_orb_image_from_luma8(luma.as_raw(), width, height)
            .expect("scan orb image from luma8 buffer");
        assert_eq!(scan_result.orb_id, request.orb_id);
    }

    #[test]
    fn scan_from_luma8_rejects_mismatched_dimensions() {
        let too_small = vec![0_u8; 10];
        let error = scan_orb_image_from_luma8(&too_small, 32, 32).expect_err("length mismatch");
        assert!(matches!(error, crate::OrbError::MalformedPayload));
    }

    #[test]
    fn scan_survives_downscaled_png() {
        let request = GenerateOrbRequest::new(derive_orb_id("orb-downscaled"));
        let orb_image = generate_orb_image(&request).expect("generate orb image");
        let downscaled = imageops::resize(&orb_image.image, 256, 256, imageops::FilterType::Lanczos3);

        let mut image_bytes = Vec::new();
        DynamicImage::ImageRgba8(downscaled)
            .write_to(
                &mut std::io::Cursor::new(&mut image_bytes),
                image::ImageFormat::Png,
            )
            .expect("encode downscaled png");

        let scan_result = scan_orb_image(&image_bytes).expect("scan downscaled png");
        assert_eq!(scan_result.orb_id, request.orb_id);
    }
}
