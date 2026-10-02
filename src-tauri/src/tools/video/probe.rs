use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::errors::ProcessingError;
use crate::models::InputFile;

pub const SUPPORTED_VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi"];

const INVALID_VIDEO_MESSAGE: &str = "This video could not be inspected";

pub struct VideoBinaries {
  pub ffmpeg: PathBuf,
  pub ffprobe: PathBuf,
}

/// Resolves the bundled FFmpeg executables.
///
/// Order: the `TOOLBOX_FFMPEG_DIR` environment variable (set at app setup for
/// the installed resource directory), then the dev staging directory under the
/// crate's `resources/ffmpeg`. Returns `VideoEngineUnavailable` when neither
/// location contains both executables.
pub fn resolve_binaries() -> Result<VideoBinaries, ProcessingError> {
  let mut candidates: Vec<PathBuf> = Vec::new();
  if let Ok(dir) = std::env::var("TOOLBOX_FFMPEG_DIR") {
    candidates.push(PathBuf::from(dir));
  }
  candidates.push(
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("resources")
      .join("ffmpeg"),
  );

  for dir in candidates {
    let ffmpeg = dir.join("ffmpeg.exe");
    let ffprobe = dir.join("ffprobe.exe");
    if ffmpeg.is_file() && ffprobe.is_file() {
      return Ok(VideoBinaries { ffmpeg, ffprobe });
    }
  }

  Err(ProcessingError::video_engine_unavailable(
    "Video engine is not available",
  ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbedVideo {
  /// Coded (stored) dimensions of the first real video stream.
  pub width: u32,
  pub height: u32,
  /// Rotation in degrees from display matrix/side data (0 when absent).
  pub rotation: i32,
  pub duration_secs: Option<f64>,
  pub has_audio: bool,
  pub audio_stream_count: usize,
  pub subtitle_stream_count: usize,
  pub attachment_stream_count: usize,
  pub data_stream_count: usize,
}

impl ProbedVideo {
  /// Dimensions as they are displayed: width/height swap for ±90/270 rotation.
  pub fn visible_dimensions(&self) -> (u32, u32) {
    let normalized = ((self.rotation % 360) + 360) % 360;
    if normalized == 90 || normalized == 270 {
      (self.height, self.width)
    } else {
      (self.width, self.height)
    }
  }
}

/// Runs ffprobe against a file and parses its JSON report.
pub fn run_probe(ffprobe: &Path, path: &Path) -> Result<ProbedVideo, ProcessingError> {
  let output = Command::new(ffprobe)
    .args([
      "-v",
      "error",
      "-print_format",
      "json",
      "-show_format",
      "-show_streams",
    ])
    .arg(path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .map_err(|_| ProcessingError::video_engine_unavailable("Could not start ffprobe"))?;

  if !output.status.success() {
    return Err(ProcessingError::invalid_video(INVALID_VIDEO_MESSAGE));
  }

  parse_probe_json(&String::from_utf8_lossy(&output.stdout))
}

/// Parses ffprobe's JSON report into a `ProbedVideo`.
///
/// Rotation comes from `side_data_list[].rotation` (falling back to the legacy
/// `tags.rotate`); duration comes from `format.duration` (falling back to the
/// video stream duration). Cover art (`disposition.attached_pic`) video streams
/// are excluded; a file without a real video stream is invalid.
pub fn parse_probe_json(json: &str) -> Result<ProbedVideo, ProcessingError> {
  let value: Value = serde_json::from_str(json)
    .map_err(|_| ProcessingError::invalid_video(INVALID_VIDEO_MESSAGE))?;
  let streams = value
    .get("streams")
    .and_then(Value::as_array)
    .ok_or_else(|| ProcessingError::invalid_video(INVALID_VIDEO_MESSAGE))?;

  let mut video_stream: Option<&Value> = None;
  let mut audio_stream_count = 0usize;
  let mut subtitle_stream_count = 0usize;
  let mut attachment_stream_count = 0usize;
  let mut data_stream_count = 0usize;

  for stream in streams {
    match stream.get("codec_type").and_then(Value::as_str) {
      Some("video") => {
        let attached_pic = stream
          .get("disposition")
          .and_then(|disposition| disposition.get("attached_pic"))
          .and_then(Value::as_i64)
          .unwrap_or(0)
          == 1;
        if !attached_pic && video_stream.is_none() {
          video_stream = Some(stream);
        }
      }
      Some("audio") => audio_stream_count += 1,
      Some("subtitle") => subtitle_stream_count += 1,
      Some("attachment") => attachment_stream_count += 1,
      Some("data") => data_stream_count += 1,
      _ => {}
    }
  }

  let stream =
    video_stream.ok_or_else(|| ProcessingError::invalid_video("No video stream found"))?;

  let width = coded_dimension(stream, "coded_width", "width");
  let height = coded_dimension(stream, "coded_height", "height");
  if width == 0 || height == 0 {
    return Err(ProcessingError::invalid_video(INVALID_VIDEO_MESSAGE));
  }
  let rotation = stream_rotation(stream);
  let duration_secs = value
    .get("format")
    .and_then(|format| format.get("duration"))
    .and_then(parse_duration_value)
    .or_else(|| stream.get("duration").and_then(parse_duration_value));

  Ok(ProbedVideo {
    width,
    height,
    rotation,
    duration_secs,
    has_audio: audio_stream_count > 0,
    audio_stream_count,
    subtitle_stream_count,
    attachment_stream_count,
    data_stream_count,
  })
}

fn coded_dimension(stream: &Value, coded_key: &str, plain_key: &str) -> u32 {
  stream
    .get(coded_key)
    .and_then(Value::as_u64)
    .filter(|dimension| *dimension > 0)
    .or_else(|| stream.get(plain_key).and_then(Value::as_u64))
    .unwrap_or(0) as u32
}

fn stream_rotation(stream: &Value) -> i32 {
  if let Some(side_data) = stream.get("side_data_list").and_then(Value::as_array) {
    for entry in side_data {
      if let Some(rotation) = entry.get("rotation") {
        if let Some(parsed) = parse_rotation_value(rotation) {
          return parsed;
        }
      }
    }
  }
  if let Some(tags) = stream.get("tags") {
    if let Some(rotation) = tags.get("rotate") {
      if let Some(parsed) = parse_rotation_value(rotation) {
        return parsed;
      }
    }
  }
  0
}

fn parse_rotation_value(value: &Value) -> Option<i32> {
  let raw = match value {
    Value::Number(number) => number.to_string(),
    Value::String(text) => text.clone(),
    _ => return None,
  };
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return None;
  }
  // Lenient: values like "-90.0" or "-0.0" round to the nearest integer and
  // -0.0 normalizes to 0.
  let degrees: f64 = trimmed.parse().ok()?;
  Some(degrees.round() as i32)
}

fn parse_duration_value(value: &Value) -> Option<f64> {
  let raw = match value {
    Value::Number(number) => number.to_string(),
    Value::String(text) => text.clone(),
    _ => return None,
  };
  let seconds: f64 = raw.trim().parse().ok()?;
  if !seconds.is_finite() || seconds < 0.0 {
    return None;
  }
  Some(seconds)
}

/// Inspects each path and returns one `InputFile` per video.
///
/// A directory path is expanded shallowly: its direct children that look like
/// supported videos are inspected (no recursion). A file path yields itself.
pub fn inspect_video_paths(paths: Vec<String>) -> Vec<InputFile> {
  let binaries = resolve_binaries().ok();
  let mut out = Vec::new();
  for path in paths {
    match std::fs::metadata(&path) {
      Ok(meta) if meta.is_dir() => out.extend(expand_video_directory(&path, binaries.as_ref())),
      Ok(meta) if meta.is_file() => {
        out.push(inspect_video_file(&path, meta.len(), binaries.as_ref()))
      }
      Ok(_) => out.push(InputFile::invalid(&path, 0, "Not a regular file")),
      Err(_) => out.push(InputFile::invalid(&path, 0, "File not found")),
    }
  }
  out
}

fn expand_video_directory(path: &str, binaries: Option<&VideoBinaries>) -> Vec<InputFile> {
  let Ok(entries) = std::fs::read_dir(path) else {
    return Vec::new();
  };
  let mut files: Vec<_> = entries
    .filter_map(Result::ok)
    .filter(|entry| entry.path().is_file())
    .collect();
  files.sort_by_key(|entry| entry.file_name());
  files
    .into_iter()
    .filter_map(|entry| {
      let file_path = entry.path();
      let extension = file_path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
      if !SUPPORTED_VIDEO_EXTENSIONS.contains(&extension.as_str()) {
        return None;
      }
      let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
      Some(inspect_video_file(
        &file_path.to_string_lossy(),
        size,
        binaries,
      ))
    })
    .collect()
}

fn inspect_video_file(path: &str, size: u64, binaries: Option<&VideoBinaries>) -> InputFile {
  let mut file = InputFile::new(path, size);

  if !SUPPORTED_VIDEO_EXTENSIONS.contains(&file.extension.as_str()) {
    file.status = "invalid".into();
    file.error = Some("Unsupported format".into());
    return file;
  }

  if !Path::new(path).exists() {
    file.status = "invalid".into();
    file.error = Some("File not found".into());
    return file;
  }

  if size == 0 {
    file.status = "invalid".into();
    file.error = Some("File is empty".into());
    return file;
  }

  let Some(binaries) = binaries else {
    file.status = "invalid".into();
    file.error = Some("Video engine is not available".into());
    return file;
  };

  match run_probe(&binaries.ffprobe, Path::new(path)) {
    Ok(probe) => {
      let (width, height) = probe.visible_dimensions();
      file.width = width;
      file.height = height;
      file.duration = probe.duration_secs;
      file.status = "ready".into();
    }
    Err(error) => {
      file.status = "invalid".into();
      file.error = Some(error.to_string());
    }
  }

  file
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_landscape_with_side_data_rotation() {
    let json = r#"{
      "streams": [{
        "codec_type": "video",
        "coded_width": 1920,
        "coded_height": 1080,
        "width": 1920,
        "height": 1080,
        "side_data_list": [{ "rotation": -90 }]
      }],
      "format": { "duration": "12.5" }
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!(probe.width, 1920);
    assert_eq!(probe.height, 1080);
    assert_eq!(probe.rotation, -90);
    assert_eq!(probe.visible_dimensions(), (1080, 1920));
    assert_eq!(probe.duration_secs, Some(12.5));
  }

  #[test]
  fn parses_legacy_rotate_tag() {
    let json = r#"{
      "streams": [{
        "codec_type": "video",
        "coded_width": 640,
        "coded_height": 480,
        "width": 640,
        "height": 480,
        "tags": { "rotate": "90" }
      }],
      "format": { "duration": "3" }
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!(probe.rotation, 90);
    assert_eq!(probe.visible_dimensions(), (480, 640));
  }

  #[test]
  fn parses_lenient_rotation_values() {
    let json = r#"{
      "streams": [{
        "codec_type": "video",
        "width": 640,
        "height": 480,
        "side_data_list": [{ "rotation": "-90.0" }]
      }],
      "format": {}
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!(probe.rotation, -90);

    let json = r#"{
      "streams": [{
        "codec_type": "video",
        "width": 640,
        "height": 480,
        "side_data_list": [{ "rotation": "-0.0" }]
      }],
      "format": {}
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!(probe.rotation, 0);
    assert_eq!(probe.visible_dimensions(), (640, 480));
  }

  #[test]
  fn counts_extra_streams() {
    let json = r#"{
      "streams": [
        { "codec_type": "video", "coded_width": 320, "coded_height": 240 },
        { "codec_type": "audio", "codec_name": "aac" },
        { "codec_type": "audio", "codec_name": "ac3" },
        { "codec_type": "subtitle", "codec_name": "mov_text" },
        { "codec_type": "attachment", "codec_name": "ttf" },
        { "codec_type": "data", "codec_name": "bin_data" }
      ],
      "format": { "duration": "5.25" }
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert!(probe.has_audio);
    assert_eq!(probe.audio_stream_count, 2);
    assert_eq!(probe.subtitle_stream_count, 1);
    assert_eq!(probe.attachment_stream_count, 1);
    assert_eq!(probe.data_stream_count, 1);
    assert_eq!(probe.duration_secs, Some(5.25));
  }

  #[test]
  fn no_audio_stream_is_reported() {
    let json = r#"{
      "streams": [{ "codec_type": "video", "coded_width": 160, "coded_height": 120 }],
      "format": { "duration": "1" }
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert!(!probe.has_audio);
    assert_eq!(probe.audio_stream_count, 0);
  }

  #[test]
  fn attached_pic_video_is_excluded() {
    let json = r#"{
      "streams": [
        {
          "codec_type": "video",
          "coded_width": 800,
          "coded_height": 800,
          "disposition": { "attached_pic": 1 }
        },
        { "codec_type": "video", "coded_width": 1920, "coded_height": 1080 }
      ],
      "format": { "duration": "4" }
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!(probe.width, 1920);
    assert_eq!(probe.height, 1080);
  }

  #[test]
  fn only_attached_pic_video_is_invalid() {
    let json = r#"{
      "streams": [
        {
          "codec_type": "video",
          "coded_width": 800,
          "coded_height": 800,
          "disposition": { "attached_pic": 1 }
        }
      ],
      "format": { "duration": "4" }
    }"#;
    let error = parse_probe_json(json).unwrap_err();
    assert!(matches!(error, ProcessingError::InvalidVideo { .. }));
  }

  #[test]
  fn missing_or_zero_video_dimensions_are_invalid() {
    for json in [
      r#"{
        "streams": [{ "codec_type": "video", "height": 240 }],
        "format": {}
      }"#,
      r#"{
        "streams": [{ "codec_type": "video", "width": 320, "height": 0 }],
        "format": {}
      }"#,
    ] {
      let error = parse_probe_json(json).unwrap_err();
      assert!(matches!(error, ProcessingError::InvalidVideo { .. }));
    }
  }

  #[test]
  fn zero_coded_dimensions_fall_back_to_display_dimensions() {
    let json = r#"{
      "streams": [{
        "codec_type": "video",
        "coded_width": 0,
        "coded_height": 0,
        "width": 320,
        "height": 240
      }],
      "format": {}
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!((probe.width, probe.height), (320, 240));
  }

  #[test]
  fn missing_streams_key_is_invalid() {
    let error = parse_probe_json(r#"{ "format": {} }"#).unwrap_err();
    assert!(matches!(error, ProcessingError::InvalidVideo { .. }));
  }

  #[test]
  fn invalid_json_is_invalid() {
    assert!(parse_probe_json("").is_err());
    assert!(parse_probe_json("not json {").is_err());
  }

  #[test]
  fn duration_falls_back_to_video_stream() {
    let json = r#"{
      "streams": [
        { "codec_type": "video", "coded_width": 320, "coded_height": 240, "duration": "2.75" },
        { "codec_type": "audio", "duration": "2.5" }
      ],
      "format": {}
    }"#;
    let probe = parse_probe_json(json).unwrap();
    assert_eq!(probe.duration_secs, Some(2.75));
  }

  #[test]
  fn visible_dimensions_swap_only_for_quarter_turns() {
    let base = |rotation: i32| ProbedVideo {
      width: 1920,
      height: 1080,
      rotation,
      duration_secs: None,
      has_audio: false,
      audio_stream_count: 0,
      subtitle_stream_count: 0,
      attachment_stream_count: 0,
      data_stream_count: 0,
    };
    assert_eq!(base(0).visible_dimensions(), (1920, 1080));
    assert_eq!(base(90).visible_dimensions(), (1080, 1920));
    assert_eq!(base(-90).visible_dimensions(), (1080, 1920));
    assert_eq!(base(270).visible_dimensions(), (1080, 1920));
    assert_eq!(base(-270).visible_dimensions(), (1080, 1920));
    assert_eq!(base(180).visible_dimensions(), (1920, 1080));
  }
}
