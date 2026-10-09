use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use image::GenericImageView;

use crate::errors::ProcessingError;
use crate::models::{BatchResult, FileResult, ImageFormat, LogoAssetDefinition};
use crate::services::export;

use super::{decode, encode, resize, svg};

/// Minimum square source resolution for raster sources in Logo Pack (matches the UI hint
/// "at least 512 × 512 px" and the largest preset size, icon-512).
const MIN_SOURCE_SIZE: u32 = 512;

/// Sizes packed into the multi-res `favicon.ico`.
const ICO_SIZES: &[u32] = &[16, 32, 48];

/// Base name of the output pack folder; numbered on collision
/// (`web-pack`, `web-pack-2`, ...) so every run gets a fresh folder.
const PACK_FOLDER_NAME: &str = "web-pack";

enum PackSource {
  Raster(image::DynamicImage),
  Svg(Box<resvg::usvg::Tree>),
}

impl PackSource {
  fn render_square(&self, size: u32) -> Result<image::DynamicImage, ProcessingError> {
    match self {
      PackSource::Raster(img) => Ok(resize::resize(img, (size, size))),
      PackSource::Svg(tree) => svg::render_fitted_square(tree, size),
    }
  }
}

/// The Standard Web Pack: the single source of truth for asset dimensions,
/// filenames, and formats. The frontend renders its checkboxes from
/// `get_logo_presets`, which returns this list.
pub static STANDARD_WEB_PACK: LazyLock<Vec<LogoAssetDefinition>> = LazyLock::new(|| {
  vec![
    LogoAssetDefinition {
      id: "favicon-ico".into(),
      filename: "favicon.ico".into(),
      width: 0,
      height: 0,
      format: "ico".into(),
      default_enabled: true,
    },
    LogoAssetDefinition {
      id: "favicon-16".into(),
      filename: "favicon-16x16.png".into(),
      width: 16,
      height: 16,
      format: "png".into(),
      default_enabled: true,
    },
    LogoAssetDefinition {
      id: "favicon-32".into(),
      filename: "favicon-32x32.png".into(),
      width: 32,
      height: 32,
      format: "png".into(),
      default_enabled: true,
    },
    LogoAssetDefinition {
      id: "apple-touch".into(),
      filename: "apple-touch-icon.png".into(),
      width: 180,
      height: 180,
      format: "png".into(),
      default_enabled: true,
    },
    LogoAssetDefinition {
      id: "icon-192".into(),
      filename: "icon-192.png".into(),
      width: 192,
      height: 192,
      format: "png".into(),
      default_enabled: true,
    },
    LogoAssetDefinition {
      id: "icon-512".into(),
      filename: "icon-512.png".into(),
      width: 512,
      height: 512,
      format: "png".into(),
      default_enabled: true,
    },
  ]
});

pub fn preset_by_id(id: &str) -> Option<&'static LogoAssetDefinition> {
  STANDARD_WEB_PACK.iter().find(|preset| preset.id == id)
}

/// Generates the selected Standard Web Pack assets from a square or SVG source into a
/// fresh pack folder under `output_dir` (the user-chosen export root),
/// invoking `on_progress` after each asset with `(completed, total, filename)`.
/// Returns the actual pack folder path and a `BatchResult` with one
/// `FileResult` per asset; per-asset failures never abort the pack. Source
/// validation failures (non-square raster, raster too small, unsupported) return `Err`.
///
/// Each run resolves a free folder (`web-pack`, `web-pack-2`, ...) so prior
/// packs are never overwritten. The command layer owns emitting Tauri progress
/// events; this processor stays runtime-agnostic so it is unit-testable
/// without a Tauri app.
pub fn generate(
  source_path: &Path,
  output_dir: &Path,
  asset_ids: &[String],
  on_progress: impl Fn(u32, u32, &str),
) -> Result<(PathBuf, BatchResult), ProcessingError> {
  export::ensure_output_dir(output_dir)?;
  let pack_dir = export::resolve_output_dir(&output_dir.join(PACK_FOLDER_NAME))?;
  export::create_output_dir(&pack_dir)?;

  let is_svg = source_path
    .extension()
    .and_then(|ext| ext.to_str())
    .map(|ext| ext.eq_ignore_ascii_case("svg"))
    .unwrap_or(false);

  let source = if is_svg {
    let tree = svg::read_and_parse(source_path)?;
    PackSource::Svg(Box::new(tree))
  } else {
    let img = decode::decode(source_path)?;
    let (width, height) = img.dimensions();
    if width != height || width < MIN_SOURCE_SIZE {
      return Err(ProcessingError::invalid_dimensions(format!(
        "Source must be square and at least {MIN_SOURCE_SIZE} × {MIN_SOURCE_SIZE} (got {width} × {height})"
      )));
    }
    PackSource::Raster(img)
  };

  let total = asset_ids.len() as u32;
  let mut items = Vec::with_capacity(asset_ids.len());
  for (index, asset_id) in asset_ids.iter().enumerate() {
    let result = generate_asset(&source, &pack_dir, asset_id);
    on_progress(index as u32 + 1, total, &result.source_path);
    items.push(result);
  }

  let succeeded = items.iter().filter(|item| item.success).count() as u32;
  Ok((
    pack_dir,
    BatchResult {
      total,
      succeeded,
      failed: total - succeeded,
      items,
    },
  ))
}

fn generate_asset(source: &PackSource, output_dir: &Path, asset_id: &str) -> FileResult {
  let Some(preset) = preset_by_id(asset_id) else {
    return FileResult {
      source_path: asset_id.to_string(),
      output_path: None,
      success: false,
      original_size: 0,
      output_size: None,
      error: Some(
        ProcessingError::processing_failed(format!("Unknown logo asset: {asset_id}")).into_dto(),
      ),
      warnings: None,
    };
  };
  match generate_asset_inner(source, output_dir, preset) {
    Ok((output_path, output_size)) => FileResult {
      source_path: preset.filename.clone(),
      output_path: Some(output_path),
      success: true,
      original_size: 0,
      output_size: Some(output_size),
      error: None,
      warnings: None,
    },
    Err(error) => FileResult {
      source_path: preset.filename.clone(),
      output_path: None,
      success: false,
      original_size: 0,
      output_size: None,
      error: Some(error.into_dto()),
      warnings: None,
    },
  }
}

fn generate_asset_inner(
  source: &PackSource,
  output_dir: &Path,
  preset: &LogoAssetDefinition,
) -> Result<(String, u64), ProcessingError> {
  let bytes = if preset.format == "ico" {
    encode_ico(source)?
  } else {
    let img = source.render_square(preset.width)?;
    encode::encode(&img, ImageFormat::Png, None)?
  };

  let output_path = output_dir.join(&preset.filename);
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
    bytes.len() as u64,
  ))
}

/// Encodes a multi-size ICO: each size is rendered from the source, PNG-encoded,
/// and packed into one ICO via `IcoFrame`/`IcoEncoder` (image 0.25 API).
fn encode_ico(source: &PackSource) -> Result<Vec<u8>, ProcessingError> {
  let mut frames = Vec::with_capacity(ICO_SIZES.len());
  for &size in ICO_SIZES {
    let img = source.render_square(size)?;
    let png = encode::encode(&img, ImageFormat::Png, None)?;
    let frame =
      image::codecs::ico::IcoFrame::with_encoded(png, size, size, image::ExtendedColorType::Rgba8)
        .map_err(|error| {
          ProcessingError::encode_failed(format!("Could not build ICO frame: {error}"))
        })?;
    frames.push(frame);
  }

  let mut buffer = Vec::new();
  image::codecs::ico::IcoEncoder::new(&mut buffer)
    .encode_images(&frames)
    .map_err(|error| ProcessingError::encode_failed(format!("Could not encode ICO: {error}")))?;
  Ok(buffer)
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering};

  use super::{generate, preset_by_id, STANDARD_WEB_PACK};
  use crate::errors::ProcessingError;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-logo-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn noop_progress(_completed: u32, _total: u32, _filename: &str) {}

  fn write_fixture_png(path: &std::path::Path, size: u32) {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(size, size, |x, y| {
      image::Rgba([(x % 251) as u8, (y % 251) as u8, 120, 255])
    }));
    img.save(path).unwrap();
  }

  #[test]
  fn preset_list_matches_standard_web_pack() {
    assert_eq!(STANDARD_WEB_PACK.len(), 6);
    assert!(preset_by_id("favicon-ico").is_some());
    assert!(preset_by_id("icon-512").is_some());
    assert!(preset_by_id("nope").is_none());
    for preset in STANDARD_WEB_PACK.iter() {
      assert!(!preset.filename.is_empty());
      assert!(preset.format == "png" || preset.format == "ico");
    }
  }

  #[test]
  fn generates_full_pack_from_square_source() {
    let dir = temp_dir();
    let source = dir.join("logo.png");
    write_fixture_png(&source, 512);

    let ids: Vec<String> = STANDARD_WEB_PACK.iter().map(|p| p.id.to_string()).collect();
    let (pack_dir, result) = generate(&source, &dir, &ids, noop_progress).unwrap();
    assert_eq!(
      pack_dir.file_name().unwrap().to_string_lossy(),
      "web-pack",
      "first run uses the plain folder name"
    );
    assert_eq!(result.total, 6);
    assert_eq!(result.succeeded, 6);
    assert_eq!(result.failed, 0);

    for preset in STANDARD_WEB_PACK.iter() {
      let path = pack_dir.join(&preset.filename);
      assert!(path.exists(), "missing {}", preset.filename);
    }
    let ico = std::fs::read(pack_dir.join("favicon.ico")).unwrap();
    assert_eq!(&ico[0..4], &[0x00, 0x00, 0x01, 0x00], "ICO header");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn second_generation_uses_numbered_folder_and_keeps_first() {
    let dir = temp_dir();
    let source = dir.join("logo.png");
    write_fixture_png(&source, 512);
    let ids: Vec<String> = STANDARD_WEB_PACK.iter().map(|p| p.id.to_string()).collect();

    let (first_dir, _) = generate(&source, &dir, &ids, noop_progress).unwrap();
    let (second_dir, second_result) = generate(&source, &dir, &ids, noop_progress).unwrap();

    assert_eq!(first_dir.file_name().unwrap().to_string_lossy(), "web-pack");
    assert_eq!(
      second_dir.file_name().unwrap().to_string_lossy(),
      "web-pack-2"
    );
    assert_eq!(second_result.succeeded, 6);
    assert!(first_dir.join("favicon.ico").exists());
    assert!(second_dir.join("favicon.ico").exists());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn subset_generates_only_selected_assets() {
    let dir = temp_dir();
    let source = dir.join("logo.png");
    write_fixture_png(&source, 512);

    let ids = vec!["icon-192".to_string()];
    let (pack_dir, result) = generate(&source, &dir, &ids, noop_progress).unwrap();
    assert_eq!(result.succeeded, 1);
    assert!(pack_dir.join("icon-192.png").exists());
    assert!(!pack_dir.join("favicon.ico").exists());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn non_square_source_is_rejected() {
    let dir = temp_dir();
    let source = dir.join("logo.png");
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(512, 400, |x, y| {
      image::Rgba([(x % 251) as u8, (y % 251) as u8, 120, 255])
    }));
    img.save(&source).unwrap();

    let ids = vec!["icon-192".to_string()];
    let err = generate(&source, &dir, &ids, noop_progress).unwrap_err();
    assert!(matches!(err, ProcessingError::InvalidDimensions { .. }));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn too_small_square_source_is_rejected() {
    let dir = temp_dir();
    let source = dir.join("logo.png");
    write_fixture_png(&source, 256);

    let ids = vec!["icon-192".to_string()];
    let err = generate(&source, &dir, &ids, noop_progress).unwrap_err();
    assert!(matches!(err, ProcessingError::InvalidDimensions { .. }));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn generates_full_pack_from_small_svg_source() {
    let dir = temp_dir();
    let source = dir.join("vector_logo.svg");
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
      <rect width="24" height="24" fill="#0055ff" />
    </svg>"##;
    std::fs::write(&source, svg).unwrap();

    let ids: Vec<String> = STANDARD_WEB_PACK.iter().map(|p| p.id.to_string()).collect();
    let (pack_dir, result) = generate(&source, &dir, &ids, noop_progress).unwrap();
    assert_eq!(result.total, 6);
    assert_eq!(result.succeeded, 6);
    assert_eq!(result.failed, 0);

    let icon512 = pack_dir.join("icon-512.png");
    assert!(icon512.exists());
    let decoded512 = image::ImageReader::open(&icon512)
      .unwrap()
      .decode()
      .unwrap();
    assert_eq!(decoded512.width(), 512);
    assert_eq!(decoded512.height(), 512);

    let ico = std::fs::read(pack_dir.join("favicon.ico")).unwrap();
    assert_eq!(&ico[0..4], &[0x00, 0x00, 0x01, 0x00], "ICO header");

    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn generates_full_pack_from_non_square_svg_source() {
    let dir = temp_dir();
    let source = dir.join("wide_logo.svg");
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
      <rect width="200" height="100" fill="#00aa00" />
    </svg>"##;
    std::fs::write(&source, svg).unwrap();

    let ids: Vec<String> = STANDARD_WEB_PACK.iter().map(|p| p.id.to_string()).collect();
    let (pack_dir, result) = generate(&source, &dir, &ids, noop_progress).unwrap();
    assert_eq!(result.succeeded, 6);

    let icon192 = pack_dir.join("icon-192.png");
    assert!(icon192.exists());
    let decoded192 = image::ImageReader::open(&icon192)
      .unwrap()
      .decode()
      .unwrap();
    assert_eq!(decoded192.width(), 192);
    assert_eq!(decoded192.height(), 192);

    std::fs::remove_dir_all(&dir).unwrap();
  }
}
