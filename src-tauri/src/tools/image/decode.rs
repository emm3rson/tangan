use std::io::BufReader;
use std::path::Path;

use image::DynamicImage;

use crate::errors::ProcessingError;
use crate::models::ImageFormat;

use super::{MAX_DIMENSION, MAX_PIXELS};

fn check_size(width: u32, height: u32) -> Result<(), ProcessingError> {
  if width > MAX_DIMENSION
    || height > MAX_DIMENSION
    || u64::from(width) * u64::from(height) > MAX_PIXELS
  {
    return Err(ProcessingError::invalid_image(format!(
      "Image too large: {width} × {height}"
    )));
  }
  Ok(())
}

/// Decodes an image file into a `DynamicImage`.
///
/// WebP uses `image_webp::WebPDecoder` because `image`'s built-in WebP
/// decoder is lossless-only. PNG/JPEG decode through `image::ImageReader`.
pub fn decode(source: &Path) -> Result<DynamicImage, ProcessingError> {
  let mut reader = image::ImageReader::open(source).map_err(|error| {
    ProcessingError::decode_failed(format!("Could not open '{}': {error}", source.display()))
  })?;
  reader = reader.with_guessed_format().map_err(|error| {
    ProcessingError::unsupported_format(format!(
      "Unrecognized image format in '{}': {error}",
      source.display()
    ))
  })?;

  match reader.format() {
    Some(image::ImageFormat::WebP) => {
      let file = std::fs::File::open(source).map_err(|error| {
        ProcessingError::decode_failed(format!(
          "Could not open WebP '{}': {error}",
          source.display()
        ))
      })?;
      let mut decoder = image_webp::WebPDecoder::new(BufReader::new(file)).map_err(|error| {
        ProcessingError::decode_failed(format!(
          "Could not decode WebP '{}': {error}",
          source.display()
        ))
      })?;
      let (width, height) = decoder.dimensions();
      let size = decoder.output_buffer_size().ok_or_else(|| {
        ProcessingError::invalid_image(format!("Image too large: {}", source.display()))
      })?;
      let mut buffer = vec![0u8; size];
      decoder.read_image(&mut buffer).map_err(|error| {
        ProcessingError::decode_failed(format!(
          "Could not decode WebP '{}': {error}",
          source.display()
        ))
      })?;
      if decoder.has_alpha() {
        image::RgbaImage::from_raw(width, height, buffer)
          .map(DynamicImage::ImageRgba8)
          .ok_or_else(|| {
            ProcessingError::invalid_image(format!("Decoded pixels invalid: {}", source.display()))
          })
      } else {
        image::RgbImage::from_raw(width, height, buffer)
          .map(DynamicImage::ImageRgb8)
          .ok_or_else(|| {
            ProcessingError::invalid_image(format!("Decoded pixels invalid: {}", source.display()))
          })
      }
    }
    Some(image::ImageFormat::Png) | Some(image::ImageFormat::Jpeg) => {
      let (width, height) = reader.into_dimensions().map_err(|error| {
        ProcessingError::decode_failed(format!(
          "Could not read dimensions of '{}': {error}",
          source.display()
        ))
      })?;
      check_size(width, height)?;
      let mut reader = image::ImageReader::open(source).map_err(|error| {
        ProcessingError::decode_failed(format!("Could not open '{}': {error}", source.display()))
      })?;
      reader = reader.with_guessed_format().map_err(|error| {
        ProcessingError::unsupported_format(format!(
          "Unrecognized image format in '{}': {error}",
          source.display()
        ))
      })?;
      reader.decode().map_err(|error| {
        ProcessingError::decode_failed(format!("Could not decode '{}': {error}", source.display()))
      })
    }
    _ => Err(ProcessingError::unsupported_format(format!(
      "Unsupported format for '{}'",
      source.display()
    ))),
  }
}

/// Detects the image format of a file from its header.
pub fn detect_format(source: &Path) -> Result<ImageFormat, ProcessingError> {
  let mut reader = image::ImageReader::open(source).map_err(|error| {
    ProcessingError::decode_failed(format!("Could not open '{}': {error}", source.display()))
  })?;
  reader = reader.with_guessed_format().map_err(|error| {
    ProcessingError::unsupported_format(format!(
      "Unrecognized image format in '{}': {error}",
      source.display()
    ))
  })?;

  match reader.format() {
    Some(image::ImageFormat::Png) => Ok(ImageFormat::Png),
    Some(image::ImageFormat::Jpeg) => Ok(ImageFormat::Jpeg),
    Some(image::ImageFormat::WebP) => Ok(ImageFormat::Webp),
    _ => Err(ProcessingError::unsupported_format(format!(
      "Unsupported format for '{}'",
      source.display()
    ))),
  }
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering};

  use image::GenericImageView;

  use super::decode;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-decode-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_fixture_png(path: &std::path::Path, width: u32, height: u32) {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
      image::Rgba([(x % 251) as u8, (y % 251) as u8, 120, 255])
    }));
    img.save(path).unwrap();
  }

  #[test]
  fn corrupt_png_fails_without_panicking() {
    let dir = temp_dir();
    let source = dir.join("broken.png");
    std::fs::write(&source, b"garbage bytes that are not a png").unwrap();
    assert!(decode(&source).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn truncated_png_fails_without_panicking() {
    let dir = temp_dir();
    let source = dir.join("truncated.png");
    std::fs::write(&source, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).unwrap();
    assert!(decode(&source).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn zero_byte_file_fails_without_panicking() {
    let dir = temp_dir();
    let source = dir.join("empty.png");
    std::fs::write(&source, []).unwrap();
    assert!(decode(&source).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn over_limit_dimension_is_rejected() {
    let dir = temp_dir();
    let source = dir.join("huge.png");
    write_fixture_png(&source, super::MAX_DIMENSION + 1, 10);
    let err = decode(&source).unwrap_err();
    assert!(matches!(
      err,
      crate::errors::ProcessingError::InvalidImage { .. }
    ));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn valid_png_decodes() {
    let dir = temp_dir();
    let source = dir.join("ok.png");
    write_fixture_png(&source, 32, 24);
    let img = decode(&source).unwrap();
    assert_eq!(img.dimensions(), (32, 24));
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
