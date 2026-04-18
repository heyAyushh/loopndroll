use std::{fs, io::Cursor, path::Path};

use image::RgbaImage;

use crate::{OrbError, OrbId, Result};

pub const DEFAULT_IMAGE_SIZE: u32 = 1024;
pub const DEFAULT_VERIFY_THRESHOLD: u32 = 14;
const MINIMUM_IMAGE_SIZE: u32 = 512;

#[derive(Debug, Clone)]
pub struct GenerateOrbRequest {
    pub orb_id: OrbId,
    pub image_size: u32,
}

impl GenerateOrbRequest {
    pub fn new(orb_id: OrbId) -> Self {
        Self {
            orb_id,
            image_size: DEFAULT_IMAGE_SIZE,
        }
    }

    pub fn with_image_size(mut self, image_size: u32) -> Result<Self> {
        if image_size < MINIMUM_IMAGE_SIZE {
            return Err(OrbError::ImageSizeTooSmall);
        }
        self.image_size = image_size;
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct OrbImage {
    pub orb_id: OrbId,
    pub image: RgbaImage,
}

impl OrbImage {
    pub(crate) fn new(orb_id: OrbId, image: RgbaImage) -> Self {
        Self { orb_id, image }
    }

    pub fn to_png_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(self.image.clone())
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|error| OrbError::PngEncode {
                details: error.to_string(),
            })?;
        Ok(bytes)
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<()> {
        let png_bytes = self.to_png_bytes()?;
        fs::write(path, png_bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanResult {
    pub orb_id: OrbId,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationResult {
    pub orb_id: OrbId,
    pub version: u8,
    pub is_match: bool,
    pub distance: u32,
    pub threshold: u32,
}
