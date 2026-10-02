use serde::{Deserialize, Serialize};

use crate::errors::ProcessingErrorDto;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
  Png,
  Jpeg,
  Webp,
}

impl ImageFormat {
  pub fn extension(&self) -> &'static str {
    match self {
      ImageFormat::Png => "png",
      ImageFormat::Jpeg => "jpg",
      ImageFormat::Webp => "webp",
    }
  }
}

/// Sheet format for the palette export. Separate from `ImageFormat`: the
/// palette sheet supports vector SVG output and has no WebP variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaletteExportFormat {
  Png,
  Jpg,
  Svg,
}

impl PaletteExportFormat {
  pub fn extension(&self) -> &'static str {
    match self {
      PaletteExportFormat::Png => "png",
      PaletteExportFormat::Jpg => "jpg",
      PaletteExportFormat::Svg => "svg",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PngOptimizationLevel {
  Fast,
  #[default]
  Balanced,
  Max,
}

impl PngOptimizationLevel {
  pub fn preset(&self) -> u8 {
    match self {
      PngOptimizationLevel::Fast => 1,
      PngOptimizationLevel::Balanced => 2,
      PngOptimizationLevel::Max => 4,
    }
  }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertImagesRequest {
  pub files: Vec<String>,
  pub output_directory: String,
  pub output_format: ImageFormat,
  pub quality: Option<u8>,
  pub png_level: Option<PngOptimizationLevel>,
  pub resize: ResizeOptions,
  pub job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressImagesRequest {
  pub files: Vec<String>,
  pub output_directory: String,
  pub quality: u8,
  pub png_level: Option<PngOptimizationLevel>,
  pub resize: ResizeOptions,
  pub job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateLogoPackRequest {
  pub source_path: String,
  pub output_directory: String,
  pub asset_ids: Vec<String>,
  pub job_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoAssetDefinition {
  pub id: String,
  pub filename: String,
  pub width: u32,
  pub height: u32,
  pub format: String,
  pub default_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateLogoPackResult {
  pub pack_directory: String,
  pub batch: BatchResult,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum ResizeOptions {
  Original,
  #[serde(rename_all = "camelCase")]
  Dimensions {
    width: u32,
    height: u32,
    #[serde(default)]
    #[allow(dead_code)]
    lock_aspect_ratio: bool,
  },
  #[serde(rename_all = "camelCase")]
  Percentage {
    percentage: u32,
  },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputFile {
  pub path: String,
  pub name: String,
  pub extension: String,
  pub size: u64,
  pub width: u32,
  pub height: u32,
  pub status: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub duration: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
}

impl InputFile {
  pub fn new(path: &str, size: u64) -> Self {
    let path_info = std::path::Path::new(path);
    let name = path_info
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_else(|| path.to_string());
    let extension = path_info
      .extension()
      .map(|ext| ext.to_string_lossy().to_lowercase())
      .unwrap_or_default();

    Self {
      path: path.to_string(),
      name,
      extension,
      size,
      width: 0,
      height: 0,
      status: "invalid".into(),
      duration: None,
      error: None,
    }
  }

  pub fn invalid(path: &str, size: u64, error: impl Into<String>) -> Self {
    let mut file = Self::new(path, size);
    file.error = Some(error.into());
    file
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoOutputFormat {
  Mp4,
  Webm,
}

impl VideoOutputFormat {
  pub fn extension(&self) -> &'static str {
    match self {
      VideoOutputFormat::Mp4 => "mp4",
      VideoOutputFormat::Webm => "webm",
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoQualityPreset {
  High,
  Balanced,
  Small,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoResolutionPreset {
  Original,
  #[serde(rename = "1080p")]
  P1080,
  #[serde(rename = "720p")]
  P720,
  #[serde(rename = "480p")]
  P480,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessVideosRequest {
  pub files: Vec<String>,
  pub output_directory: String,
  pub output_format: VideoOutputFormat,
  pub resolution: VideoResolutionPreset,
  pub quality: VideoQualityPreset,
  pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoBatchStatus {
  Completed,
  Canceled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoBatchResult {
  pub status: VideoBatchStatus,
  pub total: u32,
  pub succeeded: u32,
  pub failed: u32,
  pub items: Vec<FileResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoProcessingProgress {
  pub job_id: String,
  pub completed_files: u32,
  pub total_files: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub current_file: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub current_file_percent: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertPdfsRequest {
  pub files: Vec<String>,
  pub output_directory: String,
  pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PdfOptimizationPreset {
  #[serde(rename = "lossless")]
  Lossless,
  #[serde(rename = "balanced")]
  Balanced,
  #[serde(rename = "smaller")]
  Smaller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PdfOptimizeBatchStatus {
  Completed,
  Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PdfOptimizeOutcome {
  Optimized,
  AlreadyOptimized,
  Failed,
  Canceled,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizePdfsRequest {
  pub files: Vec<String>,
  pub output_directory: String,
  pub preset: PdfOptimizationPreset,
  pub job_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizePdfFileResult {
  pub source_path: String,
  pub outcome: PdfOptimizeOutcome,
  pub original_size: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub output_path: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub output_size: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<ProcessingErrorDto>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub warnings: Option<Vec<FileWarning>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizePdfBatchResult {
  pub status: PdfOptimizeBatchStatus,
  pub total: u32,
  pub optimized: u32,
  pub already_optimized: u32,
  pub failed: u32,
  pub items: Vec<OptimizePdfFileResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfOptimizationProgress {
  pub job_id: String,
  pub completed_files: u32,
  pub total_files: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub current_file: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub current_file_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWarning {
  pub code: String,
  pub message: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pages: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
  pub source_path: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub output_path: Option<String>,
  pub success: bool,
  pub original_size: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub output_size: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<ProcessingErrorDto>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub warnings: Option<Vec<FileWarning>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
  pub total: u32,
  pub succeeded: u32,
  pub failed: u32,
  pub items: Vec<FileResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingProgress {
  pub job_id: String,
  pub completed: u32,
  pub total: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub current_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RgbColorDto {
  pub r: u8,
  pub g: u8,
  pub b: u8,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteColorDto {
  pub r: u8,
  pub g: u8,
  pub b: u8,
  pub x: f32,
  pub y: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractColorPaletteRequest {
  pub source_path: String,
  pub target_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractColorPaletteResult {
  pub width: u32,
  pub height: u32,
  pub preview_width: u32,
  pub preview_height: u32,
  pub preview_base64: String,
  pub colors: Vec<PaletteColorDto>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub notice: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportColorPaletteRequest {
  pub source_path: String,
  pub colors: Vec<RgbColorDto>,
  pub output_directory: String,
  pub format: PaletteExportFormat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportColorPaletteResult {
  pub output_path: String,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn input_file_constructor_normalizes_extension_and_defaults_metadata() {
    let file = InputFile::new("C:/Files/Photo.JPEG", 123);

    assert_eq!(file.path, "C:/Files/Photo.JPEG");
    assert_eq!(file.name, "Photo.JPEG");
    assert_eq!(file.extension, "jpeg");
    assert_eq!(file.size, 123);
    assert_eq!((file.width, file.height), (0, 0));
    assert_eq!(file.status, "invalid");
    assert_eq!(file.duration, None);
    assert_eq!(file.error, None);

    let invalid = InputFile::invalid("missing.pdf", 0, "File not found");
    assert_eq!(invalid.status, "invalid");
    assert_eq!(invalid.error.as_deref(), Some("File not found"));
  }

  #[test]
  fn deserializes_convert_request_with_dimensions_camel_case() {
    let json = r#"{
      "files": ["C:/test.svg"],
      "outputDirectory": "C:/out",
      "outputFormat": "png",
      "resize": {
        "mode": "dimensions",
        "width": 800,
        "height": 600,
        "lockAspectRatio": true
      },
      "jobId": "test-job"
    }"#;

    let req: ConvertImagesRequest = serde_json::from_str(json).expect("valid json");
    assert_eq!(req.files, vec!["C:/test.svg"]);
    assert_eq!(
      req.resize,
      ResizeOptions::Dimensions {
        width: 800,
        height: 600,
        lock_aspect_ratio: true,
      }
    );
  }

  #[test]
  fn deserializes_dimensions_without_lock_aspect_ratio_defaults_false() {
    let json = r#"{
      "files": ["C:/test.svg"],
      "outputDirectory": "C:/out",
      "outputFormat": "png",
      "resize": {
        "mode": "dimensions",
        "width": 1024,
        "height": 768
      },
      "jobId": "test-job"
    }"#;

    let req: ConvertImagesRequest = serde_json::from_str(json).expect("valid json");
    assert_eq!(
      req.resize,
      ResizeOptions::Dimensions {
        width: 1024,
        height: 768,
        lock_aspect_ratio: false,
      }
    );
  }

  #[test]
  fn video_enums_serialize_to_pinned_shapes() {
    assert_eq!(
      serde_json::to_string(&VideoOutputFormat::Mp4).unwrap(),
      r#""mp4""#
    );
    assert_eq!(
      serde_json::to_string(&VideoOutputFormat::Webm).unwrap(),
      r#""webm""#
    );
    assert_eq!(
      serde_json::to_string(&VideoQualityPreset::High).unwrap(),
      r#""high""#
    );
    assert_eq!(
      serde_json::to_string(&VideoQualityPreset::Balanced).unwrap(),
      r#""balanced""#
    );
    assert_eq!(
      serde_json::to_string(&VideoQualityPreset::Small).unwrap(),
      r#""small""#
    );
    assert_eq!(
      serde_json::to_string(&VideoResolutionPreset::Original).unwrap(),
      r#""original""#
    );
    assert_eq!(
      serde_json::to_string(&VideoResolutionPreset::P1080).unwrap(),
      r#""1080p""#
    );
    assert_eq!(
      serde_json::to_string(&VideoResolutionPreset::P720).unwrap(),
      r#""720p""#
    );
    assert_eq!(
      serde_json::to_string(&VideoResolutionPreset::P480).unwrap(),
      r#""480p""#
    );
    assert_eq!(
      serde_json::to_string(&VideoBatchStatus::Completed).unwrap(),
      r#""completed""#
    );
    assert_eq!(
      serde_json::to_string(&VideoBatchStatus::Canceled).unwrap(),
      r#""canceled""#
    );

    let request: ProcessVideosRequest = serde_json::from_str(
      r#"{
        "files": ["C:/test.mp4"],
        "outputDirectory": "C:/out",
        "outputFormat": "mp4",
        "resolution": "1080p",
        "quality": "balanced",
        "jobId": "job-1"
      }"#,
    )
    .expect("valid json");
    assert_eq!(request.output_format, VideoOutputFormat::Mp4);
    assert_eq!(request.resolution, VideoResolutionPreset::P1080);
    assert_eq!(request.quality, VideoQualityPreset::Balanced);
    assert_eq!(request.job_id, "job-1");
  }

  #[test]
  fn deserializes_extract_color_palette_request_from_camel_case() {
    let req: ExtractColorPaletteRequest = serde_json::from_str(
      r#"{
        "sourcePath": "C:/photo.png",
        "targetCount": 6
      }"#,
    )
    .expect("valid json");
    assert_eq!(req.source_path, "C:/photo.png");
    assert_eq!(req.target_count, 6);
  }

  #[test]
  fn serializes_palette_color_dto_to_camel_case() {
    let json = serde_json::to_string(&PaletteColorDto {
      r: 255,
      g: 128,
      b: 0,
      x: 0.5,
      y: 0.25,
    })
    .unwrap();
    assert_eq!(json, r#"{"r":255,"g":128,"b":0,"x":0.5,"y":0.25}"#);
  }

  #[test]
  fn pdf_optimize_enums_serialize_to_pinned_shapes() {
    assert_eq!(
      serde_json::to_string(&PdfOptimizationPreset::Lossless).unwrap(),
      r#""lossless""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizationPreset::Balanced).unwrap(),
      r#""balanced""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizationPreset::Smaller).unwrap(),
      r#""smaller""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizeOutcome::Optimized).unwrap(),
      r#""optimized""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizeOutcome::AlreadyOptimized).unwrap(),
      r#""alreadyOptimized""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizeOutcome::Failed).unwrap(),
      r#""failed""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizeOutcome::Canceled).unwrap(),
      r#""canceled""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizeBatchStatus::Completed).unwrap(),
      r#""completed""#
    );
    assert_eq!(
      serde_json::to_string(&PdfOptimizeBatchStatus::Canceled).unwrap(),
      r#""canceled""#
    );

    let request: OptimizePdfsRequest = serde_json::from_str(
      r#"{
        "files": ["C:/test.pdf"],
        "outputDirectory": "C:/out",
        "preset": "balanced",
        "jobId": "job-1"
      }"#,
    )
    .expect("valid json");
    assert_eq!(request.preset, PdfOptimizationPreset::Balanced);
    assert_eq!(request.job_id, "job-1");

    let lossless: OptimizePdfsRequest = serde_json::from_str(
      r#"{"files":[],"outputDirectory":"C:/out","preset":"lossless","jobId":"j"}"#,
    )
    .unwrap();
    assert_eq!(lossless.preset, PdfOptimizationPreset::Lossless);
    let smaller: OptimizePdfsRequest = serde_json::from_str(
      r#"{"files":[],"outputDirectory":"C:/out","preset":"smaller","jobId":"j"}"#,
    )
    .unwrap();
    assert_eq!(smaller.preset, PdfOptimizationPreset::Smaller);
  }
}
