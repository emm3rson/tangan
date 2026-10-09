use std::path::Path;

use crate::errors::ProcessingError;
use crate::models::{FileResult, ImageFormat, PngOptimizationLevel, ResizeOptions};
use crate::services::export;

use super::{decode, encode, optimize_png, resize};

/// Compresses one source image by re-encoding it in its own format at the
/// requested quality, applying the optional resize, and writing
/// `stem-compressed.ext` to `output_dir`. Always returns a `FileResult` —
/// failures are captured, never panicked.
pub fn compress_file(
  source: &Path,
  output_dir: &Path,
  quality: u8,
  png_level: Option<PngOptimizationLevel>,
  resize_opts: &ResizeOptions,
) -> FileResult {
  let source_string = source.to_string_lossy().into_owned();
  match compress_file_inner(source, output_dir, quality, png_level, resize_opts) {
    Ok((output_path, original_size, output_size)) => FileResult {
      source_path: source_string,
      output_path: Some(output_path),
      success: true,
      original_size,
      output_size: Some(output_size),
      error: None,
      warnings: None,
    },
    Err(error) => FileResult {
      source_path: source_string,
      output_path: None,
      success: false,
      original_size: std::fs::metadata(source)
        .map(|meta| meta.len())
        .unwrap_or(0),
      output_size: None,
      error: Some(error.into_dto()),
      warnings: None,
    },
  }
}

fn compress_file_inner(
  source: &Path,
  output_dir: &Path,
  quality: u8,
  png_level: Option<PngOptimizationLevel>,
  resize_opts: &ResizeOptions,
) -> Result<(String, u64, u64), ProcessingError> {
  let original_size = std::fs::metadata(source)
    .map_err(|error| {
      if error.kind() == std::io::ErrorKind::PermissionDenied {
        ProcessingError::permission_denied(format!(
          "Permission denied reading '{}': {error}",
          source.display()
        ))
      } else {
        ProcessingError::file_not_found(format!(
          "Could not read source file '{}': {error}",
          source.display()
        ))
      }
    })?
    .len();

  let format = decode::detect_format(source)?;
  let png_level = png_level.unwrap_or_default();

  let bytes =
    if matches!(format, ImageFormat::Png) && matches!(resize_opts, ResizeOptions::Original) {
      // Direct path: optimize the raw file, preserving metadata and APNG frames
      // that a decode/re-encode round trip would drop.
      let raw = std::fs::read(source).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
          ProcessingError::permission_denied(format!(
            "Permission denied reading '{}': {error}",
            source.display()
          ))
        } else {
          ProcessingError::file_not_found(format!(
            "Could not read source file '{}': {error}",
            source.display()
          ))
        }
      })?;
      optimize_png::optimize(&raw, png_level)?
    } else {
      let img = decode::decode(source)?;
      let img = resize::apply(&img, resize_opts)?;
      let bytes = encode::encode(&img, format, Some(quality))?;
      if matches!(format, ImageFormat::Png) {
        optimize_png::optimize(&bytes, png_level)?
      } else {
        bytes
      }
    };

  let output_path =
    export::resolve_output_path(output_dir, source, format.extension(), Some("-compressed"))?;
  std::fs::write(&output_path, &bytes).map_err(|error| {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
      ProcessingError::permission_denied(format!(
        "Permission denied writing '{}': {error}",
        output_path.display()
      ))
    } else {
      ProcessingError::write_failed(format!(
        "Could not write output file '{}': {error}",
        output_path.display()
      ))
    }
  })?;

  Ok((
    output_path.to_string_lossy().into_owned(),
    original_size,
    bytes.len() as u64,
  ))
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering};

  use image::GenericImageView;

  use super::compress_file;
  use crate::models::ResizeOptions;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-compress-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_fixture_png(path: &std::path::Path, width: u32, height: u32) {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
      image::Rgba([(x * 7) as u8, (y * 11) as u8, 120, 255])
    }));
    img.save(path).unwrap();
  }

  #[test]
  fn compresses_png_to_suffixed_name() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    write_fixture_png(&source, 32, 24);

    let result = compress_file(&source, &dir, 80, None, &ResizeOptions::Original);
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let output_path = result.output_path.unwrap();
    assert!(output_path.ends_with("photo-compressed.png"));
    assert!(std::path::Path::new(&output_path).exists());
    assert_eq!(
      result.original_size,
      std::fs::metadata(&source).unwrap().len()
    );
    assert!(result.output_size.unwrap() > 0);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn compresses_jpeg_to_jpg_with_quality_reduction() {
    let dir = temp_dir();
    let source = dir.join("photo.jpeg");
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(64, 64, |x, y| {
      image::Rgb([(x * 3) as u8, (y * 5) as u8, 90])
    }));
    img.save(&source).unwrap();

    let low = compress_file(&source, &dir, 20, None, &ResizeOptions::Original);
    assert!(low.success, "unexpected failure: {:?}", low.error);
    let low_path = low.output_path.unwrap();
    assert!(low_path.ends_with("photo-compressed.jpg"));
    assert!(low.output_size.unwrap() < low.original_size);

    let high = compress_file(&source, &dir, 95, None, &ResizeOptions::Original);
    assert!(high.success, "unexpected failure: {:?}", high.error);
    assert_ne!(high.output_path.as_deref(), Some(low_path.as_str()));
    assert!(low.output_size.unwrap() < high.output_size.unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn png_direct_path_never_grows_the_file() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    write_fixture_png(&source, 32, 24);

    let result = compress_file(&source, &dir, 80, None, &ResizeOptions::Original);
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let output_size = result.output_size.unwrap();
    assert!(
      output_size <= result.original_size,
      "output {output_size} exceeded original {}",
      result.original_size
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn png_direct_path_preserves_apng_frames() {
    let dir = temp_dir();
    let source = dir.join("anim.png");
    std::fs::write(&source, super::super::test_util::apng_fixture()).unwrap();

    let result = compress_file(&source, &dir, 80, None, &ResizeOptions::Original);
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let output_path = result.output_path.unwrap();
    assert!(output_path.ends_with("anim-compressed.png"));
    let bytes = std::fs::read(&output_path).unwrap();
    assert!(
      bytes.windows(4).any(|window| window == b"acTL"),
      "output lost its APNG animation chunks"
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn png_with_resize_is_optimized_and_valid() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    write_fixture_png(&source, 64, 32);

    let result = compress_file(
      &source,
      &dir,
      80,
      None,
      &ResizeOptions::Dimensions {
        width: 32,
        height: 16,
        lock_aspect_ratio: false,
      },
    );
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let output_path = result.output_path.unwrap();
    let img = image::open(&output_path).unwrap();
    assert_eq!(img.dimensions(), (32, 16));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn corrupt_png_fails_without_panicking() {
    let dir = temp_dir();
    let source = dir.join("broken.png");
    std::fs::write(&source, b"garbage bytes that are not a png").unwrap();

    let result = compress_file(&source, &dir, 80, None, &ResizeOptions::Original);
    assert!(!result.success);
    assert!(result.error.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
