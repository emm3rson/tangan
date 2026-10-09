use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use image::GenericImageView;
use kmeans_colors::{get_kmeans, CentroidData, Kmeans, Sort};
use palette::{IntoColor, Lab, Srgb};

use crate::errors::ProcessingError;
use crate::models::{
  ExportColorPaletteResult, ExtractColorPaletteResult, ImageFormat, PaletteColorDto,
  PaletteExportFormat, RgbColorDto,
};
use crate::services::export;

use super::{decode, encode, resize, svg};

/// Alpha below this value is treated as "nearly transparent" and excluded from
/// clustering, so a transparent PNG/SVG does not drag white/black ghosts into
/// the palette.
const ALPHA_CUTOFF: u8 = 128;

/// Largest side (px) of the aspect-preserving analysis raster used for
/// clustering and handle coordinates.
const MAX_ANALYSIS_SIDE: u32 = 256;

/// Largest long edge (px) of the canonical preview returned to the editor.
const MAX_PREVIEW_LONG_EDGE: u32 = 1024;

/// Cluster count is `target × CLUSTER_MULTIPLIER`, capped so k-means stays fast
/// on the bounded analysis raster.
const CLUSTER_MULTIPLIER: usize = 3;
const MAX_CLUSTERS: usize = 36;

/// Number of deterministic k-means attempts; the lowest score wins.
const KMEANS_RUNS: u64 = 3;
const KMEANS_SEED: u64 = 42;
const KMEANS_MAX_ITER: usize = 20;
const KMEANS_CONVERGE: f32 = 5.0;

/// Delta E 76 (Euclidean Lab distance) below which two chosen centroids are
/// treated as the same color and merged.
const DELTA_E76_MERGE: f32 = 8.0;

const MIN_TARGET: usize = 3;
const MAX_TARGET: usize = 12;

/// Palette sheet layout (px), rendered as an internal SVG through the usvg
/// pipeline before PNG encoding.
const SHEET_COLUMNS: u32 = 3;
const SHEET_CELL_W: u32 = 220;
const SHEET_COLOR_H: u32 = 150;
const SHEET_TEXT_H: u32 = 62;
const SHEET_PAD: u32 = 20;
const SHEET_GAP: u32 = 16;

/// JPEG quality for the rasterized palette sheet. Fixed high value because the
/// sheet carries small HEX/RGB text that softens quickly under compression.
const SHEET_JPEG_QUALITY: u8 = 90;

enum Source {
  Raster(image::DynamicImage),
  Svg(Box<resvg::usvg::Tree>),
}

impl Source {
  fn original_size(&self) -> (u32, u32) {
    match self {
      Source::Raster(img) => img.dimensions(),
      Source::Svg(tree) => svg::intrinsic_size(tree),
    }
  }

  fn render(&self, width: u32, height: u32) -> Result<image::DynamicImage, ProcessingError> {
    match self {
      Source::Raster(img) => Ok(resize::resize(img, (width, height))),
      Source::Svg(tree) => svg::render(tree, width, height),
    }
  }
}

/// Scales a `(width, height)` pair to fit inside `max_side` per side, preserving
/// aspect ratio and never upscaling.
fn fit_max_side(width: u32, height: u32, max_side: u32) -> (u32, u32) {
  let scale = (max_side as f32 / (width.max(height) as f32)).min(1.0);
  let next_w = ((width as f32 * scale).round() as u32).max(1);
  let next_h = ((height as f32 * scale).round() as u32).max(1);
  (next_w, next_h)
}

/// Delta E 76: Euclidean distance in CIELAB.
fn lab_distance(a: Lab, b: Lab) -> f32 {
  let dl = a.l - b.l;
  let da = a.a - b.a;
  let db = a.b - b.b;
  (dl * dl + da * da + db * db).sqrt()
}

fn lab_to_rgb_u8(lab: Lab) -> Srgb<u8> {
  let srgb: Srgb = lab.into_color();
  srgb.into_format()
}

/// Greedily selects `target` centroids balancing diversity and population.
///
/// The first pick is the most populous centroid. Each later pick maximizes
/// `sqrt(population) × minimum Lab distance to the already-chosen set`, so a
/// small but visually distinct accent can still win over a near-duplicate of a
/// dominant color.
fn select_palette(data: &[CentroidData<Lab>], target: usize) -> Vec<usize> {
  let mut order: Vec<usize> = (0..data.len()).collect();
  order.sort_by(|&a, &b| {
    data[b]
      .percentage
      .total_cmp(&data[a].percentage)
      .then_with(|| a.cmp(&b))
  });

  let mut chosen: Vec<usize> = Vec::new();
  let mut remaining = order;
  while chosen.len() < target && !remaining.is_empty() {
    let mut best: Option<(usize, f32)> = None;
    for &candidate in &remaining {
      let weight = data[candidate].percentage.sqrt();
      let min_dist = chosen
        .iter()
        .map(|&c| lab_distance(data[c].centroid, data[candidate].centroid))
        .fold(f32::INFINITY, f32::min);
      let score = if chosen.is_empty() {
        weight
      } else {
        weight * min_dist
      };
      if best.is_none_or(|(_, s)| score > s) {
        best = Some((candidate, score));
      }
    }
    let (pick, _) = best.expect("remaining is non-empty");
    chosen.push(pick);
    remaining.retain(|&r| r != pick);
  }
  chosen
}

fn merge_duplicates(data: &[CentroidData<Lab>], chosen: &[usize]) -> Vec<usize> {
  let mut merged: Vec<usize> = Vec::new();
  for &i in chosen {
    let is_duplicate = merged
      .iter()
      .any(|&m| lab_distance(data[m].centroid, data[i].centroid) <= DELTA_E76_MERGE);
    if !is_duplicate {
      merged.push(i);
    }
  }
  merged
}

/// Finds the assigned visible pixel closest (in Lab) to a centroid.
fn closest_assigned_pixel(
  indices: &[u8],
  visible_lab: &[Lab],
  visible_xy: &[(u32, u32)],
  centroid_index: usize,
  centroid: Lab,
) -> Result<(u32, u32), ProcessingError> {
  let mut best_xy: Option<(u32, u32)> = None;
  let mut best_d = f32::INFINITY;
  for (pixel, &idx) in indices.iter().enumerate() {
    if idx as usize != centroid_index {
      continue;
    }
    let d = lab_distance(centroid, visible_lab[pixel]);
    if d < best_d {
      best_d = d;
      best_xy = Some(visible_xy[pixel]);
    }
  }
  best_xy.ok_or_else(|| ProcessingError::invalid_image("No pixels assigned to centroid"))
}

/// Extracts a bounded, source-mapped color palette from a single image.
///
/// `target_count` must be in `MIN_TARGET..=MAX_TARGET`. The result carries the
/// original dimensions, a canonical PNG preview (base64, bounded long edge),
/// and one `PaletteColorDto` per color holding canonical RGB plus normalized
/// `[0, 1]` sample coordinates on the preview. Fewer distinct colors than
/// requested yields a non-fatal notice; no visible pixels is a typed failure.
pub fn extract(
  source_path: &Path,
  target_count: u32,
) -> Result<ExtractColorPaletteResult, ProcessingError> {
  let target = target_count as usize;
  if !(MIN_TARGET..=MAX_TARGET).contains(&target) {
    return Err(ProcessingError::invalid_image(format!(
      "Palette size must be between {MIN_TARGET} and {MAX_TARGET}"
    )));
  }

  let is_svg = source_path
    .extension()
    .and_then(|ext| ext.to_str())
    .map(|ext| ext.eq_ignore_ascii_case("svg"))
    .unwrap_or(false);
  let source = if is_svg {
    Source::Svg(Box::new(svg::read_and_parse(source_path)?))
  } else {
    Source::Raster(decode::decode(source_path)?)
  };

  let (orig_w, orig_h) = source.original_size();
  let (analysis_w, analysis_h) = fit_max_side(orig_w, orig_h, MAX_ANALYSIS_SIDE);
  let (preview_w, preview_h) = fit_max_side(orig_w, orig_h, MAX_PREVIEW_LONG_EDGE);

  let analysis = source.render(analysis_w, analysis_h)?;
  let preview = source.render(preview_w, preview_h)?;
  let preview_png = encode::encode(&preview, ImageFormat::Png, None)?;
  let preview_base64 = BASE64.encode(preview_png);

  let rgba = analysis.to_rgba8();
  let mut visible_lab: Vec<Lab> = Vec::new();
  let mut visible_xy: Vec<(u32, u32)> = Vec::new();
  for (x, y, pixel) in rgba.enumerate_pixels() {
    if pixel[3] < ALPHA_CUTOFF {
      continue;
    }
    let srgb = Srgb::<u8>::new(pixel[0], pixel[1], pixel[2]);
    visible_lab.push(srgb.into_linear().into_color());
    visible_xy.push((x, y));
  }

  if visible_lab.is_empty() {
    return Err(ProcessingError::invalid_image(
      "The image has no visible pixels",
    ));
  }

  let k = (target * CLUSTER_MULTIPLIER)
    .min(MAX_CLUSTERS)
    .min(visible_lab.len())
    .max(1);

  let mut best: Option<Kmeans<Lab>> = None;
  for run in 0..KMEANS_RUNS {
    let result = get_kmeans(
      k,
      KMEANS_MAX_ITER,
      KMEANS_CONVERGE,
      false,
      &visible_lab,
      KMEANS_SEED + run,
    );
    if best.as_ref().is_none_or(|b| result.score < b.score) {
      best = Some(result);
    }
  }
  let result = best.expect("at least one k-means run");

  let data = Lab::sort_indexed_colors(&result.centroids, &result.indices);

  let chosen = select_palette(&data, target);
  let mut ordered = merge_duplicates(&data, &chosen);
  ordered.sort_by(|&a, &b| {
    data[b]
      .percentage
      .total_cmp(&data[a].percentage)
      .then_with(|| a.cmp(&b))
  });

  let denom_x = (analysis_w - 1).max(1) as f32;
  let denom_y = (analysis_h - 1).max(1) as f32;

  let mut colors: Vec<PaletteColorDto> = Vec::with_capacity(ordered.len());
  for &i in &ordered {
    let centroid_index = data[i].index as usize;
    let rgb = lab_to_rgb_u8(data[i].centroid);
    let (x, y) = closest_assigned_pixel(
      &result.indices,
      &visible_lab,
      &visible_xy,
      centroid_index,
      data[i].centroid,
    )?;
    colors.push(PaletteColorDto {
      r: rgb.red,
      g: rgb.green,
      b: rgb.blue,
      x: x as f32 / denom_x,
      y: y as f32 / denom_y,
    });
  }

  let notice = if colors.len() < target {
    Some(format!("Found {} distinguishable colors.", colors.len()))
  } else {
    None
  };

  Ok(ExtractColorPaletteResult {
    width: orig_w,
    height: orig_h,
    preview_width: preview_w,
    preview_height: preview_h,
    preview_base64,
    colors,
    notice,
  })
}

fn hex_upper(color: &RgbColorDto) -> String {
  format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn build_sheet_svg(colors: &[RgbColorDto], width: u32, height: u32) -> String {
  let columns = (colors.len() as u32).min(SHEET_COLUMNS);
  let cell_h = SHEET_COLOR_H + SHEET_TEXT_H;
  let mut out = String::new();
  out.push_str(&format!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
  ));
  out.push_str(&format!(
    r##"<rect width="{width}" height="{height}" fill="#ffffff"/>"##
  ));
  for (index, color) in colors.iter().enumerate() {
    let col = index as u32 % columns;
    let row = index as u32 / columns;
    let x = SHEET_PAD + col * (SHEET_CELL_W + SHEET_GAP);
    let y = SHEET_PAD + row * (cell_h + SHEET_GAP);
    let fill = format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b);
    out.push_str(&format!(
      r#"<rect x="{x}" y="{y}" width="{SHEET_CELL_W}" height="{SHEET_COLOR_H}" rx="8" fill="{fill}"/>"#
    ));
    out.push_str(&format!(
      r##"<text x="{x}" y="{ty}" font-family="Arial, sans-serif" font-size="24" font-weight="700" fill="#21201c">{hex}</text>"##,
      ty = y + SHEET_COLOR_H + 26,
      hex = hex_upper(color),
    ));
    out.push_str(&format!(
      r##"<text x="{x}" y="{ty}" font-family="Arial, sans-serif" font-size="15" fill="#6c6a63">RGB({r}, {g}, {b})</text>"##,
      ty = y + SHEET_COLOR_H + 50,
      r = color.r,
      g = color.g,
      b = color.b,
    ));
  }
  out.push_str("</svg>");
  out
}

/// Exports a deterministic, collision-safe palette sheet as PNG, JPG, or SVG.
///
/// `colors` must hold 1..=12 entries. The sheet renders `colors` into a
/// maximum-three-column grid (color block + uppercase HEX + `RGB(r, g, b)`).
/// PNG/JPG rasterize the internal SVG through the usvg/system-font pipeline;
/// SVG writes the vector sheet directly. Saved as
/// `<source-stem>-palette.<ext>`.
pub fn export(
  source_path: &Path,
  colors: &[RgbColorDto],
  output_dir: &Path,
  format: PaletteExportFormat,
) -> Result<ExportColorPaletteResult, ProcessingError> {
  if colors.is_empty() || colors.len() > MAX_TARGET {
    return Err(ProcessingError::invalid_image(format!(
      "Palette must contain between 1 and {MAX_TARGET} colors"
    )));
  }
  export::ensure_output_dir(output_dir)?;

  let columns = (colors.len() as u32).min(SHEET_COLUMNS);
  let rows = (colors.len() as u32).div_ceil(columns);
  let width = SHEET_PAD * 2 + columns * SHEET_CELL_W + (columns - 1) * SHEET_GAP;
  let cell_h = SHEET_COLOR_H + SHEET_TEXT_H;
  let height = SHEET_PAD * 2 + rows * cell_h + (rows - 1) * SHEET_GAP;

  let sheet = build_sheet_svg(colors, width, height);
  let bytes = match format {
    PaletteExportFormat::Svg => sheet.into_bytes(),
    PaletteExportFormat::Png | PaletteExportFormat::Jpg => {
      let tree = svg::parse(sheet.as_bytes())?;
      let img = svg::render(&tree, width, height)?;
      match format {
        PaletteExportFormat::Jpg => {
          encode::encode(&img, ImageFormat::Jpeg, Some(SHEET_JPEG_QUALITY))?
        }
        _ => encode::encode(&img, ImageFormat::Png, None)?,
      }
    }
  };

  let output_path = export::resolve_output_path(
    output_dir,
    source_path,
    format.extension(),
    Some("-palette"),
  )?;
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

  Ok(ExportColorPaletteResult {
    output_path: output_path.to_string_lossy().into_owned(),
  })
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering};

  use image::{Rgba, RgbaImage};

  use super::*;
  use crate::errors::ProcessingError;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-palette-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_png(path: &Path, width: u32, height: u32, fill: impl Fn(u32, u32) -> Rgba<u8>) {
    let img = image::DynamicImage::ImageRgba8(RgbaImage::from_fn(width, height, fill));
    img.save(path).unwrap();
  }

  /// Four solid quadrants (red, green, blue, yellow) built at the analysis size
  /// so no resize blending introduces spurious boundary colors.
  fn quadrant_png(path: &Path) {
    write_png(path, 256, 256, |x, y| {
      if x < 128 && y < 128 {
        Rgba([255, 0, 0, 255])
      } else if x >= 128 && y < 128 {
        Rgba([0, 255, 0, 255])
      } else if x < 128 {
        Rgba([0, 0, 255, 255])
      } else {
        Rgba([255, 255, 0, 255])
      }
    });
  }

  fn flat_png(path: &Path) {
    write_png(path, 256, 256, |_, _| Rgba([40, 120, 200, 255]));
  }

  fn two_color_png(path: &Path) {
    write_png(path, 256, 256, |x, _| {
      if x < 128 {
        Rgba([220, 40, 40, 255])
      } else {
        Rgba([40, 40, 220, 255])
      }
    });
  }

  fn accent_png(path: &Path) {
    write_png(path, 256, 256, |x, y| {
      if (96..160).contains(&x) && (96..160).contains(&y) {
        Rgba([255, 255, 0, 255])
      } else if y < 128 {
        Rgba([255, 0, 0, 255])
      } else if x < 128 {
        Rgba([0, 255, 0, 255])
      } else {
        Rgba([0, 0, 255, 255])
      }
    });
  }

  #[test]
  fn rejects_target_out_of_range() {
    let dir = temp_dir();
    let source = dir.join("flat.png");
    flat_png(&source);

    assert!(matches!(
      extract(&source, 2),
      Err(ProcessingError::InvalidImage { .. })
    ));
    assert!(matches!(
      extract(&source, 13),
      Err(ProcessingError::InvalidImage { .. })
    ));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn flat_image_returns_one_color_with_notice() {
    let dir = temp_dir();
    let source = dir.join("flat.png");
    flat_png(&source);

    let result = extract(&source, 6).unwrap();
    assert_eq!(result.colors.len(), 1);
    assert!(result.notice.is_some());
    let color = result.colors[0];
    assert!(color.r > 30 && color.r < 50);
    assert!(color.g > 110 && color.g < 130);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn two_color_image_is_merged_with_notice() {
    let dir = temp_dir();
    let source = dir.join("two.png");
    two_color_png(&source);

    let result = extract(&source, 6).unwrap();
    assert!(result.colors.len() < 6, "fewer colors than requested");
    assert!(result.notice.is_some());
    assert!(result.colors.len() >= 2, "both distinct colors kept");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn output_is_deterministic() {
    let dir = temp_dir();
    let source = dir.join("quad.png");
    quadrant_png(&source);

    let first = extract(&source, 4).unwrap();
    let second = extract(&source, 4).unwrap();
    assert_eq!(first.colors.len(), second.colors.len());
    for (a, b) in first.colors.iter().zip(second.colors.iter()) {
      assert_eq!((a.r, a.g, a.b, a.x, a.y), (b.r, b.g, b.b, b.x, b.y));
    }
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn four_distinct_quadrants_are_retained() {
    let dir = temp_dir();
    let source = dir.join("quad.png");
    quadrant_png(&source);

    let result = extract(&source, 4).unwrap();
    assert_eq!(result.colors.len(), 4, "all four distinct colors kept");

    let is_yellow = |c: &PaletteColorDto| c.r > 200 && c.g > 200 && c.b < 60;
    let is_red = |c: &PaletteColorDto| c.r > 200 && c.g < 60 && c.b < 60;
    let is_green = |c: &PaletteColorDto| c.r < 60 && c.g > 200 && c.b < 60;
    let is_blue = |c: &PaletteColorDto| c.r < 60 && c.g < 60 && c.b > 200;
    assert!(result.colors.iter().any(is_yellow));
    assert!(result.colors.iter().any(is_red));
    assert!(result.colors.iter().any(is_green));
    assert!(result.colors.iter().any(is_blue));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn small_diverse_accent_is_retained() {
    let dir = temp_dir();
    let source = dir.join("accent.png");
    accent_png(&source);

    let result = extract(&source, 4).unwrap();
    assert_eq!(result.colors.len(), 4);
    assert!(
      result
        .colors
        .iter()
        .any(|c| c.r > 200 && c.g > 200 && c.b < 60),
      "yellow accent retained among red/green/blue regions"
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn coordinates_are_finite_and_within_unit_range() {
    let dir = temp_dir();
    let source = dir.join("quad.png");
    quadrant_png(&source);

    let result = extract(&source, 4).unwrap();
    for color in &result.colors {
      assert!(color.x.is_finite() && color.y.is_finite());
      assert!((0.0..=1.0).contains(&color.x));
      assert!((0.0..=1.0).contains(&color.y));
    }
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn fully_transparent_image_fails_as_invalid() {
    let dir = temp_dir();
    let source = dir.join("empty.png");
    write_png(&source, 64, 64, |_, _| Rgba([0, 0, 0, 0]));

    let err = extract(&source, 6).unwrap_err();
    assert!(matches!(err, ProcessingError::InvalidImage { .. }));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn transparent_region_is_ignored() {
    let dir = temp_dir();
    let source = dir.join("partial.png");
    write_png(&source, 256, 256, |x, _| {
      if x < 128 {
        Rgba([255, 128, 0, 255])
      } else {
        Rgba([0, 0, 0, 0])
      }
    });

    let result = extract(&source, 6).unwrap();
    assert_eq!(result.colors.len(), 1, "only the opaque region counts");
    let color = result.colors[0];
    assert!(color.r > 200 && color.g > 100 && color.g < 160 && color.b < 60);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn svg_source_extracts_colors() {
    let dir = temp_dir();
    let source = dir.join("palette.svg");
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
      <rect width="100" height="100" fill="#ff0000" />
      <rect x="100" width="100" height="100" fill="#0000ff" />
    </svg>"##;
    std::fs::write(&source, svg).unwrap();

    let result = extract(&source, 4).unwrap();
    assert_eq!((result.width, result.height), (200, 100));
    assert!(!result.colors.is_empty());
    assert!(result.colors.len() <= 4);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn preview_long_edge_is_bounded() {
    let dir = temp_dir();
    let source = dir.join("wide.png");
    write_png(&source, 2000, 500, |x, y| {
      Rgba([(x % 251) as u8, (y % 251) as u8, 120, 255])
    });

    let result = extract(&source, 3).unwrap();
    assert!(result.preview_width <= 1024);
    assert!(result.preview_height <= 1024);
    assert_eq!(result.preview_width, 1024);
    assert_eq!(result.preview_height, 256);
    assert!(!result.preview_base64.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn export_writes_sheet_with_expected_dimensions() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    flat_png(&source);

    let colors = vec![
      RgbColorDto { r: 255, g: 0, b: 0 },
      RgbColorDto { r: 0, g: 255, b: 0 },
      RgbColorDto { r: 0, g: 0, b: 255 },
      RgbColorDto {
        r: 255,
        g: 255,
        b: 0,
      },
      RgbColorDto { r: 0, g: 0, b: 0 },
    ];
    let result = export(&source, &colors, &dir, PaletteExportFormat::Png).unwrap();
    let path = Path::new(&result.output_path);
    assert!(path.exists());
    assert_eq!(
      path.file_name().unwrap().to_string_lossy(),
      "photo-palette.png"
    );

    let decoded = image::ImageReader::open(path).unwrap().decode().unwrap();
    let expected_w = SHEET_PAD * 2 + 3 * SHEET_CELL_W + 2 * SHEET_GAP;
    let expected_h = SHEET_PAD * 2 + 2 * (SHEET_COLOR_H + SHEET_TEXT_H) + SHEET_GAP;
    assert_eq!(decoded.width(), expected_w);
    assert_eq!(decoded.height(), expected_h);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn export_writes_decodable_jpeg_sheet() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    flat_png(&source);

    let colors = vec![
      RgbColorDto { r: 255, g: 0, b: 0 },
      RgbColorDto { r: 0, g: 255, b: 0 },
      RgbColorDto { r: 0, g: 0, b: 255 },
    ];
    let result = export(&source, &colors, &dir, PaletteExportFormat::Jpg).unwrap();
    let path = Path::new(&result.output_path);
    assert_eq!(
      path.file_name().unwrap().to_string_lossy(),
      "photo-palette.jpg"
    );

    let decoded = image::ImageReader::open(path).unwrap().decode().unwrap();
    assert!(matches!(decoded, image::DynamicImage::ImageRgb8(_)));
    let expected_w = SHEET_PAD * 2 + 3 * SHEET_CELL_W + 2 * SHEET_GAP;
    let expected_h = SHEET_PAD * 2 + SHEET_COLOR_H + SHEET_TEXT_H;
    assert_eq!(decoded.width(), expected_w);
    assert_eq!(decoded.height(), expected_h);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn export_writes_vector_svg_sheet() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    flat_png(&source);

    let colors = vec![
      RgbColorDto { r: 255, g: 0, b: 0 },
      RgbColorDto { r: 0, g: 255, b: 0 },
      RgbColorDto { r: 0, g: 0, b: 255 },
    ];
    let result = export(&source, &colors, &dir, PaletteExportFormat::Svg).unwrap();
    let path = Path::new(&result.output_path);
    assert_eq!(
      path.file_name().unwrap().to_string_lossy(),
      "photo-palette.svg"
    );

    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(text.contains("#FF0000"));
    assert!(text.contains("RGB(0, 255, 0)"));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn export_renames_on_collision() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    flat_png(&source);

    let colors = vec![RgbColorDto {
      r: 10,
      g: 20,
      b: 30,
    }];
    let first = export(&source, &colors, &dir, PaletteExportFormat::Png).unwrap();
    let second = export(&source, &colors, &dir, PaletteExportFormat::Png).unwrap();
    assert_eq!(
      Path::new(&first.output_path)
        .file_name()
        .unwrap()
        .to_string_lossy(),
      "photo-palette.png"
    );
    assert_eq!(
      Path::new(&second.output_path)
        .file_name()
        .unwrap()
        .to_string_lossy(),
      "photo-palette-2.png"
    );
    assert_ne!(first.output_path, source.to_string_lossy());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn export_rejects_empty_and_over_limit() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    flat_png(&source);

    assert!(matches!(
      export(&source, &[], &dir, PaletteExportFormat::Png),
      Err(ProcessingError::InvalidImage { .. })
    ));

    let too_many: Vec<RgbColorDto> = (0..13).map(|i| RgbColorDto { r: i, g: 0, b: 0 }).collect();
    assert!(matches!(
      export(&source, &too_many, &dir, PaletteExportFormat::Png),
      Err(ProcessingError::InvalidImage { .. })
    ));
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
