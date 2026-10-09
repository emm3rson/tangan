use std::path::Path;

use crate::errors::ProcessingError;
use crate::models::{FileResult, ImageFormat, PngOptimizationLevel, ResizeOptions};
use crate::services::export;

use super::decode::decode;
use super::encode::encode;
use super::{optimize_png, resize, svg};

/// Converts one source image into `format`, applying the optional resize, and
/// writes it to `output_dir`. Always returns a `FileResult` — failures are
/// captured, never panicked.
pub fn convert_file(
  source: &Path,
  output_dir: &Path,
  format: ImageFormat,
  quality: Option<u8>,
  png_level: Option<PngOptimizationLevel>,
  resize_opts: &ResizeOptions,
) -> FileResult {
  let source_string = source.to_string_lossy().into_owned();
  match convert_file_inner(source, output_dir, format, quality, png_level, resize_opts) {
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

fn convert_file_inner(
  source: &Path,
  output_dir: &Path,
  format: ImageFormat,
  quality: Option<u8>,
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

  let is_svg = source
    .extension()
    .and_then(|ext| ext.to_str())
    .map(|ext| ext.eq_ignore_ascii_case("svg"))
    .unwrap_or(false);

  let img = if is_svg {
    let tree = svg::read_and_parse(source)?;
    let intrinsic = svg::intrinsic_size(&tree);
    let target = resize::target_size(intrinsic, resize_opts)?.unwrap_or(intrinsic);
    svg::render(&tree, target.0, target.1)?
  } else {
    let img = decode(source)?;
    resize::apply(&img, resize_opts)?
  };

  let bytes = encode(&img, format, quality)?;
  let bytes = if matches!(format, ImageFormat::Png) {
    optimize_png::optimize(&bytes, png_level.unwrap_or_default())?
  } else {
    bytes
  };

  let output_path = export::resolve_output_path(output_dir, source, format.extension(), None)?;
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

  use super::convert_file;
  use crate::models::{ImageFormat, ResizeOptions};

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-convert-test-{}-{}",
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

  fn write_fixture_svg(path: &std::path::Path, width: u32, height: u32) {
    let svg = format!(
      r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}">
        <rect width="{width}" height="{height}" fill="#336699" />
      </svg>"##
    );
    std::fs::write(path, svg.as_bytes()).unwrap();
  }

  #[test]
  fn converts_png_to_jpeg_with_expected_name_and_sizes() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    write_fixture_png(&source, 32, 24);

    let result = convert_file(
      &source,
      &dir,
      ImageFormat::Jpeg,
      Some(80),
      None,
      &ResizeOptions::Original,
    );
    assert!(result.success, "unexpected failure: {:?}", result.error);
    assert_eq!(
      result.original_size,
      std::fs::metadata(&source).unwrap().len()
    );
    let output_path = result.output_path.unwrap();
    assert!(output_path.ends_with("photo.jpg"));
    assert!(std::path::Path::new(&output_path).exists());
    assert!(result.output_size.unwrap() > 0);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn converts_to_webp_and_back() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    write_fixture_png(&source, 32, 24);

    let webp = convert_file(
      &source,
      &dir,
      ImageFormat::Webp,
      Some(75),
      None,
      &ResizeOptions::Original,
    );
    assert!(webp.success, "unexpected failure: {:?}", webp.error);
    let webp_path = webp.output_path.unwrap();

    let roundtrip = convert_file(
      std::path::Path::new(&webp_path),
      &dir,
      ImageFormat::Png,
      None,
      None,
      &ResizeOptions::Original,
    );
    assert!(
      roundtrip.success,
      "unexpected failure: {:?}",
      roundtrip.error
    );
    let png_path = roundtrip.output_path.unwrap();
    let decoded = image::ImageReader::open(&png_path)
      .unwrap()
      .decode()
      .unwrap();
    assert_eq!(decoded.width(), 32);
    assert_eq!(decoded.height(), 24);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn percentage_resize_is_applied_to_output() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    write_fixture_png(&source, 40, 20);

    let result = convert_file(
      &source,
      &dir,
      ImageFormat::Webp,
      Some(80),
      None,
      &ResizeOptions::Percentage { percentage: 50 },
    );
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let decoded = image_webp::WebPDecoder::new(std::io::BufReader::new(
      std::fs::File::open(result.output_path.unwrap()).unwrap(),
    ))
    .unwrap();
    assert_eq!(decoded.dimensions(), (20, 10));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn converts_svg_to_png_webp_and_jpeg() {
    let dir = temp_dir();
    let source = dir.join("graphic.svg");
    write_fixture_svg(&source, 100, 50);

    // SVG -> PNG
    let png_res = convert_file(
      &source,
      &dir,
      ImageFormat::Png,
      None,
      None,
      &ResizeOptions::Original,
    );
    assert!(png_res.success, "SVG->PNG failed: {:?}", png_res.error);
    let png_path = png_res.output_path.unwrap();
    let png_decoded = image::ImageReader::open(&png_path)
      .unwrap()
      .decode()
      .unwrap();
    assert_eq!(png_decoded.width(), 100);
    assert_eq!(png_decoded.height(), 50);

    // SVG -> JPEG (with resize)
    let jpg_res = convert_file(
      &source,
      &dir,
      ImageFormat::Jpeg,
      Some(85),
      None,
      &ResizeOptions::Dimensions {
        width: 200,
        height: 100,
        lock_aspect_ratio: true,
      },
    );
    assert!(jpg_res.success, "SVG->JPG failed: {:?}", jpg_res.error);
    let jpg_path = jpg_res.output_path.unwrap();
    let jpg_decoded = image::ImageReader::open(&jpg_path)
      .unwrap()
      .decode()
      .unwrap();
    assert_eq!(jpg_decoded.width(), 200);
    assert_eq!(jpg_decoded.height(), 100);

    // SVG -> WebP
    let webp_res = convert_file(
      &source,
      &dir,
      ImageFormat::Webp,
      Some(80),
      None,
      &ResizeOptions::Original,
    );
    assert!(webp_res.success, "SVG->WebP failed: {:?}", webp_res.error);

    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn converts_corrupt_svg_with_graceful_failure() {
    let dir = temp_dir();
    let source = dir.join("broken.svg");
    std::fs::write(&source, b"<svg><not-closed>").unwrap();

    let res = convert_file(
      &source,
      &dir,
      ImageFormat::Png,
      None,
      None,
      &ResizeOptions::Original,
    );
    assert!(!res.success);
    assert!(res.error.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
