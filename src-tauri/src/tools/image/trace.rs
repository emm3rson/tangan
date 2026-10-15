use std::io::BufReader;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::errors::ProcessingError;
use crate::models::{
  BatchResult, FileResult, TraceColorMode, TraceDetail, TraceImagesRequest, TraceOptions,
  TracePreviewResult,
};
use crate::services::{batch, export};
use image::{DynamicImage, GenericImageView, ImageDecoder};
use vtracer::{ColorImage, ColorMode, Config, Preset};

const MAX_TRACE_PIXELS: u64 = 4_000_000;
const RESIZE_GUIDANCE: &str =
  "Resize the image to 4 million pixels or fewer, then try tracing again.";
static TRACE_LOCK: Mutex<()> = Mutex::new(());

/// Traces a supported raster image entirely in memory. Preview and export use
/// this same function so their options and generated vector paths stay aligned.
pub fn preview(
  source_path: &Path,
  options: TraceOptions,
) -> Result<TracePreviewResult, ProcessingError> {
  let (svg, width, height) = trace_to_svg(source_path, options)?;
  Ok(TracePreviewResult { svg, width, height })
}

pub fn export_file(
  source_path: &Path,
  output_dir: &Path,
  options: TraceOptions,
) -> Result<PathBuf, ProcessingError> {
  export::ensure_output_dir(output_dir)?;
  let (svg, _, _) = trace_to_svg(source_path, options)?;
  let output_path = export::resolve_output_path(output_dir, source_path, "svg", None)?;
  std::fs::write(&output_path, svg.as_bytes()).map_err(|error| {
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
  Ok(output_path)
}

pub fn run_batch<F>(
  request: &TraceImagesRequest,
  on_progress: F,
) -> Result<BatchResult, ProcessingError>
where
  F: Fn(u32, u32, &str),
{
  let output_dir = PathBuf::from(&request.output_directory);
  export::ensure_output_dir(&output_dir)?;
  let files: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();
  batch::run_sequential_batch(
    &files,
    |source| {
      let source_path = source.to_string_lossy().into_owned();
      let original_size = std::fs::metadata(source)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
      match catch_unwind(AssertUnwindSafe(|| {
        export_file(source, &output_dir, request.options)
      })) {
        Ok(Ok(output_path)) => {
          let output_size = std::fs::metadata(&output_path)
            .ok()
            .map(|metadata| metadata.len());
          FileResult {
            source_path,
            output_path: Some(output_path.to_string_lossy().into_owned()),
            success: true,
            original_size,
            output_size,
            error: None,
            warnings: None,
          }
        }
        Ok(Err(error)) => FileResult {
          source_path,
          output_path: None,
          success: false,
          original_size,
          output_size: None,
          error: Some(error.into()),
          warnings: None,
        },
        Err(_) => FileResult {
          source_path,
          output_path: None,
          success: false,
          original_size,
          output_size: None,
          error: Some(
            ProcessingError::processing_failed(
              "Tracing failed unexpectedly. Try resizing the image and try again.",
            )
            .into(),
          ),
          warnings: None,
        },
      }
    },
    on_progress,
  )
}

fn trace_to_svg(
  source_path: &Path,
  options: TraceOptions,
) -> Result<(String, u32, u32), ProcessingError> {
  // A settings change can start a fresh preview while an older blocking task
  // is still tracing. Keep peak decode and tracer memory to one image process.
  let _trace_guard = match TRACE_LOCK.lock() {
    Ok(guard) => guard,
    Err(poisoned) => {
      // The lock protects no shared image state, so recovering its guard is
      // safe and lets later requests proceed after a caught worker panic.
      TRACE_LOCK.clear_poison();
      poisoned.into_inner()
    }
  };
  let format = super::decode::detect_format(source_path)?;
  let (raw_width, raw_height) = image_dimensions(source_path, format)?;
  check_trace_dimensions(raw_width, raw_height)?;
  let mut image = decode_trace_image(source_path, format)?;
  image.apply_orientation(jpeg_orientation(source_path, format)?);
  let (width, height) = image.dimensions();
  check_trace_dimensions(width, height)?;
  if width == 0 || height == 0 {
    return Err(ProcessingError::invalid_image(
      "Image has invalid dimensions",
    ));
  }

  let mut rgba = image.to_rgba8();
  let has_transparency = rgba.pixels().any(|pixel| pixel[3] == 0);
  if rgba.pixels().all(|pixel| pixel[3] == 0) {
    return Err(ProcessingError::invalid_image(
      "The image has no visible pixels",
    ));
  }
  for pixel in rgba.pixels_mut() {
    let alpha = pixel[3];
    if alpha == 0 {
      continue;
    }
    // SVG has no raster alpha input to the tracer. Composite partial alpha on
    // white, while retaining fully transparent pixels for color-mode keying.
    if alpha < 255 {
      for channel in &mut pixel.0[..3] {
        *channel = ((*channel as u16 * alpha as u16 + 255 * (255 - alpha as u16)) / 255) as u8;
      }
      pixel[3] = 255;
    }
  }

  let mut pixels = rgba.into_raw();
  let (trace_width, trace_height) =
    if has_transparency && options.color_mode == TraceColorMode::Color {
      // vtracer only enables its color-key path when it sees enough transparency
      // on sampled edge rows. A one-pixel transparent border guarantees its key
      // path runs for internal-only holes too. The final nested viewport crops
      // that tracing margin back to the original image dimensions.
      pixels = add_transparent_border(&pixels, width, height);
      (width + 2, height + 2)
    } else {
      (width, height)
    };
  if options.color_mode == TraceColorMode::Binary {
    for pixel in pixels.chunks_exact_mut(4) {
      if pixel[3] == 0 {
        // Transparent pixels remain on the white side of the binary threshold.
        pixel[..3].fill(255);
        pixel[3] = 255;
      } else {
        let luminance =
          (299u32 * pixel[0] as u32 + 587u32 * pixel[1] as u32 + 114u32 * pixel[2] as u32 + 500)
            / 1000;
        pixel[..3].fill(luminance as u8);
      }
    }
  }

  let image = ColorImage {
    pixels,
    width: trace_width as usize,
    height: trace_height as usize,
  };
  let config = tracer_config(options);
  let svg = catch_unwind(AssertUnwindSafe(|| vtracer::convert(image, config)))
    .map_err(|_| ProcessingError::processing_failed("The tracing engine stopped unexpectedly"))?
    .map_err(|error| {
      ProcessingError::processing_failed(format!("Could not trace image: {error}"))
    })?;

  let svg = svg.to_string();
  let svg = if has_transparency && options.color_mode == TraceColorMode::Color {
    crop_transparent_border(&svg, width, height, trace_width, trace_height)
  } else {
    svg
  };
  Ok((svg, width, height))
}

fn jpeg_orientation(
  source_path: &Path,
  format: crate::models::ImageFormat,
) -> Result<image::metadata::Orientation, ProcessingError> {
  if !matches!(format, crate::models::ImageFormat::Jpeg) {
    return Ok(image::metadata::Orientation::NoTransforms);
  }
  let reader = image::ImageReader::open(source_path)
    .map_err(|error| ProcessingError::decode_failed(format!("Could not open image: {error}")))?
    .with_guessed_format()
    .map_err(|error| {
      ProcessingError::unsupported_format(format!("Could not identify image: {error}"))
    })?;
  let mut decoder = reader.into_decoder().map_err(|error| {
    ProcessingError::decode_failed(format!("Could not read image metadata: {error}"))
  })?;
  decoder.orientation().map_err(|error| {
    ProcessingError::decode_failed(format!("Could not read JPEG orientation: {error}"))
  })
}

fn decode_trace_image(
  source_path: &Path,
  format: crate::models::ImageFormat,
) -> Result<DynamicImage, ProcessingError> {
  if !matches!(format, crate::models::ImageFormat::Jpeg) {
    return super::decode::decode(source_path);
  }
  let reader = image::ImageReader::open(source_path)
    .map_err(|error| ProcessingError::decode_failed(format!("Could not open image: {error}")))?
    .with_guessed_format()
    .map_err(|error| {
      ProcessingError::unsupported_format(format!("Could not identify image: {error}"))
    })?;
  let decoder = reader
    .into_decoder()
    .map_err(|error| ProcessingError::decode_failed(format!("Could not decode JPEG: {error}")))?;
  DynamicImage::from_decoder(decoder)
    .map_err(|error| ProcessingError::decode_failed(format!("Could not decode JPEG: {error}")))
}

fn add_transparent_border(source: &[u8], width: u32, height: u32) -> Vec<u8> {
  let next_width = width as usize + 2;
  let next_height = height as usize + 2;
  let mut output = vec![0; next_width * next_height * 4];
  for y in 0..height as usize {
    let src_start = y * width as usize * 4;
    let dst_start = ((y + 1) * next_width + 1) * 4;
    output[dst_start..dst_start + width as usize * 4]
      .copy_from_slice(&source[src_start..src_start + width as usize * 4]);
  }
  output
}

fn crop_transparent_border(
  svg: &str,
  width: u32,
  height: u32,
  trace_width: u32,
  trace_height: u32,
) -> String {
  format!(
    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><svg x=\"-1\" y=\"-1\" width=\"{trace_width}\" height=\"{trace_height}\" viewBox=\"0 0 {trace_width} {trace_height}\">{}</svg></svg>",
    svg.trim_start_matches("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")
  )
}

fn tracer_config(options: TraceOptions) -> Config {
  let mut config = Config::from_preset(match options.color_mode {
    TraceColorMode::Color => Preset::Poster,
    TraceColorMode::Binary => Preset::Bw,
  });
  config.color_mode = match options.color_mode {
    TraceColorMode::Color => ColorMode::Color,
    TraceColorMode::Binary => ColorMode::Binary,
  };
  config.mode = if options.smooth {
    visioncortex::PathSimplifyMode::Spline
  } else {
    visioncortex::PathSimplifyMode::Polygon
  };
  let (speckle, precision, layer_difference) = match options.detail {
    TraceDetail::Low => (10, 6, 32),
    TraceDetail::Balanced => (4, 8, 16),
    TraceDetail::High => (1, 8, 8),
  };
  config.filter_speckle = speckle;
  config.color_precision = precision;
  config.layer_difference = layer_difference;
  config
}

fn image_dimensions(
  source_path: &Path,
  format: crate::models::ImageFormat,
) -> Result<(u32, u32), ProcessingError> {
  match format {
    crate::models::ImageFormat::Webp => {
      let file = std::fs::File::open(source_path).map_err(|error| {
        ProcessingError::decode_failed(format!(
          "Could not open '{}': {error}",
          source_path.display()
        ))
      })?;
      let decoder = image_webp::WebPDecoder::new(BufReader::new(file)).map_err(|error| {
        ProcessingError::decode_failed(format!("Could not read image dimensions: {error}"))
      })?;
      Ok(decoder.dimensions())
    }
    _ => {
      let reader = image::ImageReader::open(source_path)
        .map_err(|error| {
          ProcessingError::decode_failed(format!(
            "Could not open '{}': {error}",
            source_path.display()
          ))
        })?
        .with_guessed_format()
        .map_err(|error| {
          ProcessingError::unsupported_format(format!("Could not identify image format: {error}"))
        })?;
      reader.into_dimensions().map_err(|error| {
        ProcessingError::decode_failed(format!("Could not read image dimensions: {error}"))
      })
    }
  }
}

fn check_trace_dimensions(width: u32, height: u32) -> Result<(), ProcessingError> {
  let pixels = u64::from(width) * u64::from(height);
  if width > super::MAX_DIMENSION || height > super::MAX_DIMENSION {
    return Err(ProcessingError::limit_exceeded(format!(
      "Image dimensions {width} × {height} exceed the tracing limit of {} pixels per side. {RESIZE_GUIDANCE}",
      super::MAX_DIMENSION
    )));
  }
  if pixels > MAX_TRACE_PIXELS {
    return Err(ProcessingError::limit_exceeded(format!(
      "Image has {pixels} pixels; tracing supports up to {MAX_TRACE_PIXELS}. {RESIZE_GUIDANCE}"
    )));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering};

  use image::{Rgba, RgbaImage};

  use super::*;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-trace-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_image(path: &Path, width: u32, height: u32, pixel: impl Fn(u32, u32) -> Rgba<u8>) {
    RgbaImage::from_fn(width, height, pixel).save(path).unwrap();
  }

  fn write_oriented_jpeg(path: &Path) {
    let image = image::RgbImage::from_fn(4, 2, |x, y| {
      if x < 2 && y == 0 {
        image::Rgb([220, 30, 30])
      } else {
        image::Rgb([30, 30, 220])
      }
    });
    DynamicImage::ImageRgb8(image).save(path).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let exif = [
      b'E', b'x', b'i', b'f', 0, 0, // APP1 Exif signature
      b'I', b'I', 42, 0, 8, 0, 0, 0, // Little-endian TIFF header
      1, 0, // One IFD entry
      0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // Orientation = rotate 90 clockwise
      0, 0, 0, 0, // No next IFD
    ];
    let segment_length = (exif.len() as u16 + 2).to_be_bytes();
    let mut jpeg = Vec::with_capacity(bytes.len() + exif.len() + 4);
    jpeg.extend_from_slice(&bytes[..2]);
    jpeg.extend_from_slice(&[0xff, 0xe1]);
    jpeg.extend_from_slice(&segment_length);
    jpeg.extend_from_slice(&exif);
    jpeg.extend_from_slice(&bytes[2..]);
    std::fs::write(path, jpeg).unwrap();
  }

  fn write_png_header_only(path: &Path, width: u32, height: u32) {
    RgbaImage::from_pixel(1, 1, Rgba([10, 20, 30, 255]))
      .save(path)
      .unwrap();
    let mut bytes = std::fs::read(path).unwrap();
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = png_crc32(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    std::fs::write(path, bytes).unwrap();
  }

  fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
      crc ^= u32::from(byte);
      for _ in 0..8 {
        crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
      }
    }
    !crc
  }

  fn options(color_mode: TraceColorMode) -> TraceOptions {
    TraceOptions {
      color_mode,
      detail: TraceDetail::Balanced,
      smooth: true,
    }
  }

  #[test]
  fn outputs_parseable_path_svg_for_color_and_binary_modes() {
    let dir = temp_dir();
    let source = dir.join("two-colors.png");
    write_image(&source, 48, 40, |x, _| {
      if x < 24 {
        Rgba([210, 40, 40, 255])
      } else {
        Rgba([20, 80, 210, 255])
      }
    });

    for mode in [TraceColorMode::Color, TraceColorMode::Binary] {
      let preview = preview(&source, options(mode)).unwrap();
      assert!(preview.svg.contains("<path"));
      assert!(!preview.svg.contains("<image"));
      resvg::usvg::Tree::from_data(preview.svg.as_bytes(), &resvg::usvg::Options::default())
        .unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn binary_mode_uses_luminance_instead_of_red_channel() {
    let dir = temp_dir();
    let blue = dir.join("blue.png");
    let green = dir.join("green.png");
    write_image(&blue, 32, 32, |_, _| Rgba([0, 0, 255, 255]));
    write_image(&green, 32, 32, |_, _| Rgba([0, 255, 0, 255]));
    let dark = preview(&blue, options(TraceColorMode::Binary)).unwrap();
    let bright = preview(&green, options(TraceColorMode::Binary)).unwrap();
    assert!(
      dark.svg.contains("<path"),
      "dark blue luminance should classify as black"
    );
    assert!(
      !bright.svg.contains("<path"),
      "bright green luminance should classify as white"
    );
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn enforces_four_million_pixel_limit_before_decode() {
    let dir = temp_dir();
    let source = dir.join("over-limit.png");
    write_png_header_only(&source, 2001, 2000);
    let err = preview(&source, options(TraceColorMode::Color)).unwrap_err();
    assert!(
      matches!(err, ProcessingError::LimitExceeded { .. }),
      "{err:?}"
    );
    assert!(err.to_string().contains("Resize the image"));
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn corrupt_raster_is_reported_as_error() {
    let dir = temp_dir();
    let source = dir.join("broken.png");
    std::fs::write(&source, b"not a png").unwrap();
    assert!(preview(&source, options(TraceColorMode::Color)).is_err());
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn jpeg_exif_orientation_is_applied_before_tracing() {
    let dir = temp_dir();
    let source = dir.join("rotated.jpg");
    write_oriented_jpeg(&source);
    let result = preview(&source, options(TraceColorMode::Color)).unwrap();
    assert_eq!((result.width, result.height), (2, 4));
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn flattened_request_fields_deserialize_from_camel_case_ipc_shape() {
    let preview: crate::models::PreviewTraceRequest = serde_json::from_str(
      r#"{"sourcePath":"C:/in.png","colorMode":"binary","detail":"high","smooth":false}"#,
    )
    .unwrap();
    assert_eq!(preview.options.color_mode, TraceColorMode::Binary);
    assert_eq!(preview.options.detail, TraceDetail::High);
    assert!(!preview.options.smooth);

    let batch: TraceImagesRequest = serde_json::from_str(
      r#"{"files":["C:/in.png"],"outputDirectory":"C:/out","colorMode":"color","detail":"low","smooth":true,"jobId":"job-1"}"#,
    )
    .unwrap();
    assert_eq!(batch.options.color_mode, TraceColorMode::Color);
    assert_eq!(batch.options.detail, TraceDetail::Low);
    assert!(batch.options.smooth);
  }

  #[test]
  fn preview_and_export_match_and_collisions_preserve_source() {
    let dir = temp_dir();
    let source = dir.join("mark.png");
    write_image(&source, 32, 32, |x, y| {
      if (8..24).contains(&x) && (8..24).contains(&y) {
        Rgba([0, 0, 0, 255])
      } else {
        Rgba([255, 255, 255, 255])
      }
    });
    let source_bytes = std::fs::read(&source).unwrap();
    let expected = preview(&source, options(TraceColorMode::Color)).unwrap();
    let output = export_file(&source, &dir, options(TraceColorMode::Color)).unwrap();
    assert_eq!(std::fs::read_to_string(output).unwrap(), expected.svg);
    let second = export_file(&source, &dir, options(TraceColorMode::Color)).unwrap();
    assert!(second
      .file_name()
      .unwrap()
      .to_string_lossy()
      .contains("-2.svg"));
    assert_eq!(std::fs::read(source).unwrap(), source_bytes);
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn batch_keeps_success_when_another_file_is_corrupt() {
    let dir = temp_dir();
    let output_dir = dir.join("out");
    std::fs::create_dir(&output_dir).unwrap();
    let good = dir.join("good.png");
    write_image(&good, 24, 24, |x, y| {
      if x < 12 && y < 12 {
        Rgba([10, 50, 200, 255])
      } else {
        Rgba([240, 210, 30, 255])
      }
    });
    let corrupt = dir.join("bad.png");
    std::fs::write(&corrupt, b"corrupt raster").unwrap();
    let request = TraceImagesRequest {
      files: vec![
        good.to_string_lossy().into_owned(),
        corrupt.to_string_lossy().into_owned(),
      ],
      output_directory: output_dir.to_string_lossy().into_owned(),
      options: options(TraceColorMode::Color),
      job_id: "test-job".into(),
    };
    let result = run_batch(&request, |_, _, _| {}).unwrap();
    assert_eq!((result.total, result.succeeded, result.failed), (2, 1, 1));
    assert!(result.items[0].success);
    assert!(!result.items[1].success);
    assert!(result.items[0]
      .output_path
      .as_ref()
      .is_some_and(|path| Path::new(path).is_file()));
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn fully_transparent_border_remains_transparent() {
    let dir = temp_dir();
    let source = dir.join("transparent.png");
    write_image(&source, 40, 40, |x, y| {
      if (10..30).contains(&x) && (10..30).contains(&y) {
        Rgba([220, 20, 20, 255])
      } else {
        Rgba([0, 0, 0, 0])
      }
    });
    let result = preview(&source, options(TraceColorMode::Color)).unwrap();
    let tree = crate::tools::image::svg::parse(result.svg.as_bytes()).unwrap();
    let rendered = crate::tools::image::svg::render(&tree, result.width, result.height).unwrap();
    let rgba = rendered.to_rgba8();
    assert!(
      rgba.get_pixel(1, 1)[3] < 16,
      "transparent border must stay a hole"
    );
    assert!(
      rgba.get_pixel(20, 20)[3] > 240,
      "opaque foreground must remain visible"
    );
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn internal_transparent_hole_stays_a_hole() {
    let dir = temp_dir();
    let source = dir.join("hole.png");
    write_image(&source, 40, 40, |x, y| {
      if (5..8).contains(&x) && (5..8).contains(&y) {
        Rgba([0, 0, 0, 0])
      } else {
        Rgba([190, 40, 30, 255])
      }
    });
    let result = preview(&source, options(TraceColorMode::Color)).unwrap();
    assert_eq!((result.width, result.height), (40, 40));
    let tree = crate::tools::image::svg::parse(result.svg.as_bytes()).unwrap();
    let rendered = crate::tools::image::svg::render(&tree, result.width, result.height).unwrap();
    let rgba = rendered.to_rgba8();
    assert!(
      rgba.get_pixel(6, 6)[3] < 16,
      "interior transparent hole must stay clear"
    );
    assert!(
      rgba.get_pixel(12, 12)[3] > 240,
      "surrounding image must remain visible"
    );
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn semitransparent_color_is_composited_over_white() {
    let dir = temp_dir();
    let source = dir.join("semi.png");
    write_image(&source, 32, 32, |x, y| {
      if (8..24).contains(&x) && (8..24).contains(&y) {
        Rgba([255, 0, 0, 128])
      } else {
        Rgba([0, 0, 0, 0])
      }
    });
    let result = preview(&source, options(TraceColorMode::Color)).unwrap();
    let tree = crate::tools::image::svg::parse(result.svg.as_bytes()).unwrap();
    let rendered = crate::tools::image::svg::render(&tree, result.width, result.height).unwrap();
    let rgba = rendered.to_rgba8();
    let center = rgba.get_pixel(16, 16);
    assert!(center[0] > 245);
    assert!((120..=140).contains(&center[1]));
    assert!((120..=140).contains(&center[2]));
    assert_eq!(center[3], 255);
    std::fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn fully_transparent_image_is_rejected() {
    let dir = temp_dir();
    let source = dir.join("empty.png");
    write_image(&source, 8, 8, |_, _| Rgba([0, 0, 0, 0]));
    assert!(matches!(
      preview(&source, options(TraceColorMode::Color)),
      Err(ProcessingError::InvalidImage { .. })
    ));
    std::fs::remove_dir_all(dir).unwrap();
  }
}
