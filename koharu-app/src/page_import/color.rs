//! Bring page images with an embedded colour profile into sRGB.
//!
//! Koharu works on raw pixel values and writes renders / exports without a
//! colour profile, which viewers then read as sRGB. A page tagged with
//! another profile (Apple RGB, Adobe RGB, Dot Gain 20%, …) would come out
//! visibly darker or duller than the original, so such pages are converted
//! to sRGB: on import, and when an older project's page is decoded.

use std::io::Cursor;

use anyhow::{Context as _, Result};
use image::{
    DynamicImage, GrayAlphaImage, GrayImage, ImageDecoder, ImageFormat, ImageReader, RgbImage,
    RgbaImage,
};
use qcms::{DataType, Intent, Profile, Transform};

/// Decode an image, converted to sRGB when it carries a colour profile
/// that changes its colours. `true` when it was converted.
pub fn decode_srgb(bytes: &[u8]) -> Result<(DynamicImage, bool)> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("read image")?
        .into_decoder()
        .context("decode image")?;
    let icc = decoder.icc_profile().ok().flatten();
    let img = DynamicImage::from_decoder(decoder).context("decode image")?;
    Ok(
        match icc.as_deref().and_then(|icc| convert_to_srgb(&img, icc)) {
            Some(converted) => (converted, true),
            None => (img, false),
        },
    )
}

/// Decode a page image for import, converting it to sRGB when needed.
/// Returns the bytes to store (the input when nothing was converted,
/// otherwise a PNG) and the decoded image.
pub fn decode_page(bytes: Vec<u8>) -> Result<(Vec<u8>, DynamicImage)> {
    let (img, converted) = decode_srgb(&bytes)?;
    if !converted {
        return Ok((bytes, img));
    }
    let mut png = Cursor::new(Vec::new());
    img.write_to(&mut png, ImageFormat::Png)
        .context("encode sRGB page")?;
    Ok((png.into_inner(), img))
}

/// The image converted from `icc` to sRGB, or `None` when the profile is
/// unsupported (CMYK, broken), does not match the pixels, or leaves them
/// (practically) unchanged, e.g. an sRGB profile.
fn convert_to_srgb(img: &DynamicImage, icc: &[u8]) -> Option<DynamicImage> {
    let input = Profile::new_from_slice(icc, false)?;
    let srgb = Profile::new_sRGB();
    let intent = Intent::Perceptual;
    let (w, h) = (img.width(), img.height());
    let channels = img.color().channel_count();
    let has_alpha = img.color().has_alpha();

    let converted = match (icc.get(16..20)?, has_alpha) {
        (b"RGB ", false) if channels >= 3 => {
            let mut px = img.to_rgb8().into_raw();
            Transform::new(&input, &srgb, DataType::RGB8, intent)?.apply(&mut px);
            DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, px)?)
        }
        (b"RGB ", true) if channels >= 3 => {
            let mut px = img.to_rgba8().into_raw();
            Transform::new(&input, &srgb, DataType::RGBA8, intent)?.apply(&mut px);
            DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, px)?)
        }
        (b"GRAY", false) if channels < 3 => {
            let src = img.to_luma8();
            let mut out = vec![0u8; src.as_raw().len() * 3];
            Transform::new_to(&input, &srgb, DataType::Gray8, DataType::RGB8, intent)?
                .convert(src.as_raw(), &mut out);
            // A grey profile maps to neutral sRGB: keep the page grey.
            let grey: Vec<u8> = out.as_chunks::<3>().0.iter().map(|p| p[1]).collect();
            DynamicImage::ImageLuma8(GrayImage::from_raw(w, h, grey)?)
        }
        (b"GRAY", true) if channels < 3 => {
            let src = img.to_luma_alpha8();
            let mut out = vec![0u8; src.as_raw().len() * 2];
            Transform::new_to(&input, &srgb, DataType::GrayA8, DataType::RGBA8, intent)?
                .convert(src.as_raw(), &mut out);
            let grey: Vec<u8> = out
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[1], p[3]])
                .collect();
            DynamicImage::ImageLumaA8(GrayAlphaImage::from_raw(w, h, grey)?)
        }
        _ => return None,
    };

    (max_difference(img, &converted) > 1).then_some(converted)
}

fn max_difference(a: &DynamicImage, b: &DynamicImage) -> u8 {
    a.to_rgba8()
        .as_raw()
        .iter()
        .zip(b.to_rgba8().as_raw())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(img: &DynamicImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A minimal v2 matrix/TRC ICC profile with sRGB primaries and a plain
    /// gamma curve (gamma 1.8 behaves like Apple RGB).
    fn gamma_rgb_profile(gamma: f32) -> Vec<u8> {
        fn be32(v: u32) -> [u8; 4] {
            v.to_be_bytes()
        }
        fn s15(v: f64) -> [u8; 4] {
            ((v * 65536.0).round() as i32).to_be_bytes()
        }
        fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
            let mut t = b"XYZ \0\0\0\0".to_vec();
            t.extend(s15(x));
            t.extend(s15(y));
            t.extend(s15(z));
            t
        }
        let curv = {
            let mut t = b"curv\0\0\0\0".to_vec();
            t.extend(be32(1));
            t.extend(((gamma * 256.0).round() as u16).to_be_bytes());
            t.extend([0, 0]);
            t
        };
        // sRGB (D50-adapted) primaries and white point.
        let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            (b"rXYZ", xyz(0.4361, 0.2225, 0.0139)),
            (b"gXYZ", xyz(0.3851, 0.7169, 0.0971)),
            (b"bXYZ", xyz(0.1431, 0.0606, 0.7141)),
            (b"rTRC", curv.clone()),
            (b"gTRC", curv.clone()),
            (b"bTRC", curv),
        ];
        let table_len = 4 + 12 * tags.len();
        let mut offset = 128 + table_len;
        let mut table = be32(tags.len() as u32).to_vec();
        let mut data = Vec::new();
        for (sig, body) in &tags {
            table.extend(*sig);
            table.extend(be32(offset as u32));
            table.extend(be32(body.len() as u32));
            data.extend(body);
            offset += body.len();
        }
        let size = 128 + table.len() + data.len();
        let mut header = vec![0u8; 128];
        header[0..4].copy_from_slice(&be32(size as u32));
        header[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
        header[12..16].copy_from_slice(b"mntr");
        header[16..20].copy_from_slice(b"RGB ");
        header[20..24].copy_from_slice(b"XYZ ");
        header[36..40].copy_from_slice(b"acsp");
        header[68..80].copy_from_slice(&[0, 0, 0xF6, 0xD6, 0, 1, 0, 0, 0, 0, 0xD3, 0x2D]);
        [header, table, data].concat()
    }

    #[test]
    fn untagged_pages_pass_through_untouched() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, image::Rgb([60, 90, 120])));
        let bytes = png_bytes(&img);
        let (out, decoded) = decode_page(bytes.clone()).unwrap();
        assert_eq!(out, bytes);
        assert_eq!(decoded.to_rgb8(), img.to_rgb8());
    }

    #[test]
    fn gamma_1_8_profile_is_lightened_into_srgb() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, image::Rgb([60, 90, 120])));
        let icc = gamma_rgb_profile(1.8);
        let converted = convert_to_srgb(&img, &icc).expect("profile changes the colours");
        let px = converted.to_rgb8().get_pixel(0, 0).0;
        // Mid tones in a gamma 1.8 space are lighter than the same numbers
        // read as sRGB, which is why such pages looked darker.
        assert!(px[0] > 60 && px[1] > 90 && px[2] > 120, "{px:?}");
    }

    #[test]
    fn non_matching_or_unknown_profiles_are_ignored() {
        let grey = DynamicImage::ImageLuma8(GrayImage::from_pixel(2, 2, image::Luma([80])));
        assert!(convert_to_srgb(&grey, &gamma_rgb_profile(1.8)).is_none());
        let rgb = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3])));
        assert!(convert_to_srgb(&rgb, b"not a profile").is_none());
    }
}
