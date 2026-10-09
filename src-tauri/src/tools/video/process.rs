use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::errors::{ProcessingError, ProcessingErrorDto};
use crate::models::{
  FileResult, FileWarning, ProcessVideosRequest, VideoBatchResult, VideoBatchStatus,
  VideoProcessingProgress,
};
use crate::services::export;

use super::args;
use super::probe::{self, ProbedVideo, VideoBinaries};

pub const EVENT_PROGRESS: &str = "video-progress";

const STDERR_TAIL_LINES: usize = 10;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const PARTIAL_EXTENSION_HINT: &str = "part-";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobState {
  Running,
  CancelRequested,
}

static JOBS: OnceLock<Mutex<HashMap<String, JobState>>> = OnceLock::new();
static PARTIAL_COUNTER: AtomicU32 = AtomicU32::new(0);

fn jobs() -> &'static Mutex<HashMap<String, JobState>> {
  JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_job(job_id: &str) {
  jobs()
    .lock()
    .unwrap()
    .insert(job_id.to_string(), JobState::Running);
}

pub fn unregister_job(job_id: &str) {
  jobs().lock().unwrap().remove(job_id);
}

/// Flips a running job into the cancel-requested state.
/// Returns `false` when the job is unknown or already canceling.
pub fn cancel_job(job_id: &str) -> bool {
  let mut map = jobs().lock().unwrap();
  if let Some(state) = map.get_mut(job_id) {
    if *state == JobState::Running {
      *state = JobState::CancelRequested;
      return true;
    }
  }
  false
}

fn is_cancel_requested(job_id: &str) -> bool {
  jobs().lock().unwrap().get(job_id) == Some(&JobState::CancelRequested)
}

struct JobGuard(String);

impl Drop for JobGuard {
  fn drop(&mut self) {
    unregister_job(&self.0);
  }
}

/// One `-progress pipe:1` sample from FFmpeg's stdout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSample {
  pub out_time_secs: Option<f64>,
  pub finished: bool,
}

/// Parses a single key=value progress line.
///
/// `out_time_us` and `out_time_ms` are both reported by FFmpeg in
/// microseconds; `progress=end` marks the stream finished. Unknown keys are
/// ignored.
pub fn parse_progress_line(line: &str) -> Option<ProgressSample> {
  let (key, value) = line.trim().split_once('=')?;
  match key.trim() {
    "out_time_us" | "out_time_ms" => {
      let micros: i64 = value.trim().parse().ok()?;
      Some(ProgressSample {
        out_time_secs: Some(micros as f64 / 1_000_000.0),
        finished: false,
      })
    }
    "progress" => Some(ProgressSample {
      out_time_secs: None,
      finished: value.trim() == "end",
    }),
    _ => None,
  }
}

type EmitFn = Box<dyn Fn(&VideoProcessingProgress) + Send + Sync>;

struct BatchEmitter {
  job_id: String,
  total_files: u32,
  completed: AtomicU32,
  current_file: Mutex<Option<(String, f64)>>,
  emit: EmitFn,
}

impl BatchEmitter {
  fn snapshot(&self) {
    let current = self.current_file.lock().unwrap().clone();
    let (current_file, current_file_percent) = match current {
      Some((name, percent)) => (Some(name), Some(percent)),
      None => (None, None),
    };
    let progress = VideoProcessingProgress {
      job_id: self.job_id.clone(),
      completed_files: self.completed.load(Ordering::Relaxed),
      total_files: self.total_files,
      current_file,
      current_file_percent,
    };
    (self.emit)(&progress);
  }

  fn start_file(&self, name: &str) {
    *self.current_file.lock().unwrap() = Some((name.to_string(), 0.0));
    self.snapshot();
  }

  fn update_percent(&self, percent: f64) {
    {
      let mut current = self.current_file.lock().unwrap();
      match current.as_mut() {
        Some((_, slot)) => *slot = percent,
        None => return,
      }
    }
    self.snapshot();
  }

  fn finish_file(&self) {
    *self.current_file.lock().unwrap() = None;
    self.snapshot();
  }
}

enum EncodeOutcome {
  Success { warnings: Option<Vec<FileWarning>> },
  Failed(ProcessingError),
  Canceled,
}

/// Processes video files sequentially through the bundled FFmpeg.
///
/// Each file is encoded to a uniquely named partial output in `output_dir`,
/// inspected, and only then moved to its collision-safe final path. Cancellation
/// stops the active child, prevents later files from starting, removes the
/// partial output, and is reported as a typed `Canceled` batch status; files
/// completed before cancellation are kept. Unstarted files are not included in
/// the result items.
pub fn run_batch<F>(
  files: &[PathBuf],
  output_dir: &Path,
  request: &ProcessVideosRequest,
  binaries: &VideoBinaries,
  emit: F,
) -> Result<VideoBatchResult, ProcessingError>
where
  F: Fn(&VideoProcessingProgress) + Send + Sync + 'static,
{
  register_job(&request.job_id);
  let _guard = JobGuard(request.job_id.clone());

  let emitter = Arc::new(BatchEmitter {
    job_id: request.job_id.clone(),
    total_files: files.len() as u32,
    completed: AtomicU32::new(0),
    current_file: Mutex::new(None),
    emit: Box::new(emit),
  });

  let mut items: Vec<FileResult> = Vec::new();
  let mut canceled = false;

  for (index, file) in files.iter().enumerate() {
    if is_cancel_requested(&request.job_id) {
      canceled = true;
      break;
    }

    let source_path = file.to_string_lossy().into_owned();
    let original_size = std::fs::metadata(file).map(|meta| meta.len()).unwrap_or(0);

    let source_probe = match probe::run_probe(&binaries.ffprobe, file) {
      Ok(probed) => probed,
      Err(error) => {
        items.push(FileResult {
          source_path,
          output_path: None,
          success: false,
          original_size,
          output_size: None,
          error: Some(error.into_dto()),
          warnings: None,
        });
        emitter.completed.store(index as u32 + 1, Ordering::Relaxed);
        emitter.finish_file();
        continue;
      }
    };

    let extension = request.output_format.extension();
    let final_path = match export::resolve_output_path(output_dir, file, extension, None) {
      Ok(path) => path,
      Err(error) => {
        items.push(FileResult {
          source_path,
          output_path: None,
          success: false,
          original_size,
          output_size: None,
          error: Some(error.into_dto()),
          warnings: None,
        });
        emitter.completed.store(index as u32 + 1, Ordering::Relaxed);
        emitter.finish_file();
        continue;
      }
    };
    let partial_path = resolve_partial_path(output_dir, &final_path, extension);

    let file_name = file
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_else(|| source_path.clone());
    emitter.completed.store(index as u32, Ordering::Relaxed);
    emitter.start_file(&file_name);

    let outcome = encode_file(
      binaries,
      request,
      &source_probe,
      file,
      &partial_path,
      &final_path,
      &emitter,
    );

    let mut was_canceled = false;
    match outcome {
      EncodeOutcome::Success { warnings } => {
        let output_size = std::fs::metadata(&final_path)
          .map(|meta| meta.len())
          .unwrap_or(0);
        items.push(FileResult {
          source_path,
          output_path: Some(final_path.to_string_lossy().into_owned()),
          success: true,
          original_size,
          output_size: Some(output_size),
          error: None,
          warnings,
        });
      }
      EncodeOutcome::Failed(error) => {
        items.push(FileResult {
          source_path,
          output_path: None,
          success: false,
          original_size,
          output_size: None,
          error: Some(error.into_dto()),
          warnings: None,
        });
      }
      EncodeOutcome::Canceled => {
        was_canceled = true;
        items.push(FileResult {
          source_path,
          output_path: None,
          success: false,
          original_size,
          output_size: None,
          error: Some(ProcessingErrorDto::canceled()),
          warnings: None,
        });
      }
    }

    emitter.completed.store(index as u32 + 1, Ordering::Relaxed);
    emitter.finish_file();

    if was_canceled {
      canceled = true;
      break;
    }
  }

  let status = if canceled {
    VideoBatchStatus::Canceled
  } else {
    VideoBatchStatus::Completed
  };
  let total = items.len() as u32;
  let succeeded = items.iter().filter(|item| item.success).count() as u32;

  Ok(VideoBatchResult {
    status,
    total,
    succeeded,
    failed: total - succeeded,
    items,
  })
}

/// Encodes one file to a partial output and commits it transactionally.
fn encode_file(
  binaries: &VideoBinaries,
  request: &ProcessVideosRequest,
  source_probe: &ProbedVideo,
  input: &Path,
  partial: &Path,
  final_path: &Path,
  emitter: &Arc<BatchEmitter>,
) -> EncodeOutcome {
  let ffmpeg_args = args::build_ffmpeg_args(
    (request.output_format, request.quality, request.resolution),
    source_probe,
    input,
    partial,
  );

  let mut child = match Command::new(&binaries.ffmpeg)
    .args(&ffmpeg_args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
  {
    Ok(child) => child,
    Err(_) => {
      return EncodeOutcome::Failed(ProcessingError::encode_failed("Could not start FFmpeg"));
    }
  };

  let stdout = child.stdout.take();
  let stderr = child.stderr.take();

  let stdout_emitter = Arc::clone(emitter);
  let duration_secs = source_probe.duration_secs;
  let stdout_handle = std::thread::spawn(move || {
    let Some(stdout) = stdout else {
      return;
    };
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
      let Ok(line) = line else { break };
      let Some(sample) = parse_progress_line(&line) else {
        continue;
      };
      if let (Some(out_time_secs), Some(total_secs)) = (sample.out_time_secs, duration_secs) {
        if total_secs > 0.0 {
          let percent = ((out_time_secs / total_secs) * 100.0).clamp(0.0, 99.9);
          stdout_emitter.update_percent(percent);
        }
      }
    }
  });

  let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
  let stderr_writer = Arc::clone(&stderr_tail);
  let stderr_handle = std::thread::spawn(move || {
    let Some(stderr) = stderr else {
      return;
    };
    let reader = BufReader::new(stderr);
    for line in reader.lines() {
      let Ok(line) = line else { break };
      let mut tail = stderr_writer.lock().unwrap();
      if tail.len() >= STDERR_TAIL_LINES {
        tail.pop_front();
      }
      tail.push_back(line);
    }
  });

  let mut canceled = false;
  let mut exit_status: Option<ExitStatus> = None;
  let mut status_error = None;
  loop {
    if is_cancel_requested(&request.job_id) {
      let _ = child.kill();
      let _ = child.wait();
      canceled = true;
      break;
    }
    match child.try_wait() {
      Ok(Some(status)) => {
        exit_status = Some(status);
        break;
      }
      Ok(None) => std::thread::sleep(POLL_INTERVAL),
      Err(error) => {
        status_error = Some(error);
        let _ = child.kill();
        let _ = child.wait();
        break;
      }
    }
  }

  let _ = stdout_handle.join();
  let _ = stderr_handle.join();

  if canceled {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Canceled;
  }

  if let Some(error) = status_error {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::encode_failed(format!(
      "Could not read FFmpeg status: {error}"
    )));
  }

  let Some(status) = exit_status else {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::encode_failed("FFmpeg exited unexpectedly"));
  };

  if !status.success() {
    let _ = std::fs::remove_file(partial);
    let code = status
      .code()
      .map(|code| code.to_string())
      .unwrap_or_else(|| "unknown".into());
    let mut message = format!("FFmpeg exited with code {code}");
    let tail = stderr_tail_message(&stderr_tail);
    if !tail.is_empty() {
      message.push_str(&format!(": {tail}"));
    }
    return EncodeOutcome::Failed(ProcessingError::encode_failed(message));
  }

  let output_size = std::fs::metadata(partial)
    .map(|meta| meta.len())
    .unwrap_or(0);
  let output_valid = output_size > 0 && probe::run_probe(&binaries.ffprobe, partial).is_ok();

  if !output_valid {
    let _ = std::fs::remove_file(partial);
    let mut message = "FFmpeg produced invalid output".to_string();
    let tail = stderr_tail_message(&stderr_tail);
    if !tail.is_empty() {
      message.push_str(&format!(": {tail}"));
    }
    return EncodeOutcome::Failed(ProcessingError::encode_failed(message));
  }

  if let Err(error) = std::fs::rename(partial, final_path) {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::write_failed(format!(
      "Could not finalize output '{}': {error}",
      final_path.display()
    )));
  }

  EncodeOutcome::Success {
    warnings: omitted_stream_warnings(source_probe),
  }
}

/// Resolves a unique `{final_stem}.part-{n}.{ext}` path in the output
/// directory, retaining the target extension so inspection and renaming work.
fn resolve_partial_path(output_dir: &Path, final_path: &Path, extension: &str) -> PathBuf {
  let stem = final_path
    .file_stem()
    .map(|stem| stem.to_string_lossy().into_owned())
    .unwrap_or_else(|| "output".into());
  loop {
    let n = PARTIAL_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    let candidate = output_dir.join(format!("{stem}.{PARTIAL_EXTENSION_HINT}{n}.{extension}"));
    if !candidate.exists() {
      return candidate;
    }
  }
}

fn stderr_tail_message(tail: &Mutex<VecDeque<String>>) -> String {
  let tail = tail.lock().unwrap();
  tail.iter().cloned().collect::<Vec<_>>().join(" | ")
}

fn omitted_stream_warnings(probe: &ProbedVideo) -> Option<Vec<FileWarning>> {
  let mut warnings = Vec::new();

  if probe.audio_stream_count > 1 {
    let extra = probe.audio_stream_count - 1;
    warnings.push(FileWarning {
      code: "VIDEO_OMITTED_AUDIO".into(),
      message: if extra == 1 {
        "Extra audio stream omitted: only the first track is kept".into()
      } else {
        "Extra audio streams omitted: only the first track is kept".into()
      },
      pages: None,
    });
  }

  if probe.subtitle_stream_count > 0 {
    warnings.push(FileWarning {
      code: "VIDEO_OMITTED_SUBTITLES".into(),
      message: if probe.subtitle_stream_count == 1 {
        "Subtitle stream is not preserved".into()
      } else {
        "Subtitle streams are not preserved".into()
      },
      pages: None,
    });
  }

  if probe.attachment_stream_count > 0 {
    warnings.push(FileWarning {
      code: "VIDEO_OMITTED_ATTACHMENTS".into(),
      message: if probe.attachment_stream_count == 1 {
        "Attached file is not preserved".into()
      } else {
        "Attached files are not preserved".into()
      },
      pages: None,
    });
  }

  if probe.data_stream_count > 0 {
    warnings.push(FileWarning {
      code: "VIDEO_OMITTED_DATA".into(),
      message: if probe.data_stream_count == 1 {
        "Data stream is not preserved".into()
      } else {
        "Data streams are not preserved".into()
      },
      pages: None,
    });
  }

  if warnings.is_empty() {
    None
  } else {
    Some(warnings)
  }
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

  use crate::models::{VideoOutputFormat, VideoQualityPreset, VideoResolutionPreset};

  use super::*;

  static COUNTER: AtomicU32 = AtomicU32::new(0);
  static TEST_JOB_COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-video-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, AtomicOrdering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn unique_job_id() -> String {
    format!(
      "video-test-job-{}-{}",
      std::process::id(),
      TEST_JOB_COUNTER.fetch_add(1, AtomicOrdering::Relaxed)
    )
  }

  fn request(
    job_id: &str,
    output_dir: &Path,
    format: VideoOutputFormat,
    quality: VideoQualityPreset,
    resolution: VideoResolutionPreset,
  ) -> ProcessVideosRequest {
    ProcessVideosRequest {
      files: Vec::new(),
      output_directory: output_dir.to_string_lossy().into_owned(),
      output_format: format,
      resolution,
      quality,
      job_id: job_id.to_string(),
    }
  }

  fn noop_emit(_progress: &VideoProcessingProgress) {}

  fn list_dir(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
      .unwrap()
      .filter_map(Result::ok)
      .map(|entry| entry.file_name().to_string_lossy().into_owned())
      .collect();
    names.sort();
    names
  }

  fn staged_binaries() -> Option<VideoBinaries> {
    match probe::resolve_binaries() {
      Ok(binaries) => Some(binaries),
      Err(_) => {
        eprintln!(
          "skipping gated test: FFmpeg binaries are not staged (run src-tauri/scripts/prepare-ffmpeg.ps1)"
        );
        None
      }
    }
  }

  fn create_fixture(binaries: &VideoBinaries, dir: &Path, name: &str, spec: &[&str]) -> PathBuf {
    let path = dir.join(name);
    let output = Command::new(&binaries.ffmpeg)
      .args(["-hide_banner", "-y", "-v", "error"])
      .args(spec)
      .arg(&path)
      .output()
      .expect("ffmpeg fixture spawn");
    assert!(
      output.status.success(),
      "fixture {name} failed: {}",
      String::from_utf8_lossy(&output.stderr)
    );
    path
  }

  fn write_fake_cmd(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
  }

  /// Batch body for a fake ffmpeg.cmd. `%LAST%` resolves to the final
  /// argument, which run_batch always sets to the partial output path.
  fn fake_ffmpeg_body(final_command: &str) -> String {
    format!(
      "@echo off\r\nset \"LAST=%~1\"\r\n:loop\r\nif \"%~1\"==\"\" goto done\r\nset \"LAST=%~1\"\r\nshift\r\ngoto loop\r\n:done\r\n{final_command}\r\n"
    )
  }

  const FAKE_PROBE_JSON: &str = "{\"streams\":[{\"codec_type\":\"video\",\"coded_width\":320,\"coded_height\":240,\"width\":320,\"height\":240}],\"format\":{\"duration\":\"1.0\"}}";
  const FAKE_PROBE_JSON_WITHOUT_DURATION: &str = "{\"streams\":[{\"codec_type\":\"video\",\"coded_width\":320,\"coded_height\":240,\"width\":320,\"height\":240}],\"format\":{}}";

  // --- registry state machine ---

  #[test]
  fn cancel_unknown_job_returns_false() {
    assert!(!cancel_job("no-such-job-ever"));
  }

  #[test]
  fn cancel_running_job_flips_state() {
    let job_id = unique_job_id();
    register_job(&job_id);
    assert!(!is_cancel_requested(&job_id));
    assert!(cancel_job(&job_id));
    assert!(is_cancel_requested(&job_id));
    assert!(!cancel_job(&job_id), "already canceling");
    unregister_job(&job_id);
    assert!(!is_cancel_requested(&job_id));
  }

  // --- progress parsing ---

  #[test]
  fn parses_out_time_us_as_microseconds() {
    let sample = parse_progress_line("out_time_us=500000").unwrap();
    assert_eq!(sample.out_time_secs, Some(0.5));
    assert!(!sample.finished);
  }

  #[test]
  fn parses_out_time_ms_as_microseconds() {
    let sample = parse_progress_line("out_time_ms=1500000").unwrap();
    assert_eq!(sample.out_time_secs, Some(1.5));
    assert!(!sample.finished);
  }

  #[test]
  fn parses_progress_end_and_continue() {
    let end = parse_progress_line("progress=end").unwrap();
    assert!(end.finished);
    assert_eq!(end.out_time_secs, None);

    let continuing = parse_progress_line("progress=continue").unwrap();
    assert!(!continuing.finished);
  }

  #[test]
  fn ignores_unknown_progress_lines() {
    assert_eq!(parse_progress_line("bitrate=1234"), None);
    assert_eq!(parse_progress_line("frame=42"), None);
    assert_eq!(parse_progress_line("no-equals-sign"), None);
    assert_eq!(parse_progress_line("out_time_us=not-a-number"), None);
  }

  #[test]
  fn trims_whitespace_around_progress_lines() {
    let sample = parse_progress_line("  out_time_us=1000  \n").unwrap();
    assert_eq!(sample.out_time_secs, Some(0.001));
  }

  // --- partial path resolution ---

  #[test]
  fn partial_paths_are_unique_and_keep_target_extension() {
    let dir = temp_dir();
    let final_path = dir.join("clip.mp4");
    let first = resolve_partial_path(&dir, &final_path, "mp4");
    assert!(first.to_string_lossy().contains(".part-"));
    assert_eq!(first.extension().and_then(|ext| ext.to_str()), Some("mp4"));
    std::fs::write(&first, b"occupy").unwrap();
    let second = resolve_partial_path(&dir, &final_path, "mp4");
    assert_ne!(first, second);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  // --- real FFmpeg tests (gated on staged binaries) ---

  #[test]
  fn mp4_encode_succeeds_and_reports_progress() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let source = create_fixture(
      &binaries,
      &dir,
      "source.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=15:duration=0.5",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=0.5",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "64k",
      ],
    );
    let files = vec![source];
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );

    let events: Arc<Mutex<Vec<VideoProcessingProgress>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let result = run_batch(&files, &out, &request, &binaries, move |progress| {
      sink.lock().unwrap().push(progress.clone());
    })
    .unwrap();

    assert_eq!(result.status, VideoBatchStatus::Completed);
    assert_eq!(result.total, 1);
    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed, 0);

    let item = &result.items[0];
    assert!(item.success, "unexpected failure: {:?}", item.error);
    let output_path = item.output_path.as_ref().unwrap();
    assert!(Path::new(output_path).ends_with("source.mp4"));
    assert!(item.original_size > 0);
    assert!(item.output_size.unwrap() > 0);
    assert!(
      item.warnings.is_none(),
      "single-audio source has no warnings"
    );

    let probed = probe::run_probe(&binaries.ffprobe, Path::new(output_path)).unwrap();
    assert!(probed.width > 0 && probed.height > 0);
    assert!(probed.duration_secs.unwrap_or(0.0) > 0.0);
    assert_eq!(probed.audio_stream_count, 1);

    let events = events.lock().unwrap();
    assert!(
      events
        .iter()
        .any(|event| event.current_file.as_deref() == Some("source.mp4")),
      "expected a start-of-file event: {events:?}"
    );
    assert!(
      events
        .iter()
        .any(|event| event.completed_files == 1 && event.current_file.is_none()),
      "expected a completion event: {events:?}"
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn webm_encode_succeeds() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let source = create_fixture(
      &binaries,
      &dir,
      "source.webm",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x120:rate=10:duration=0.3",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=0.3",
        "-c:v",
        "libvpx-vp9",
        "-deadline",
        "realtime",
        "-cpu-used",
        "8",
        "-crf",
        "40",
        "-b:v",
        "0",
        "-row-mt",
        "1",
        "-c:a",
        "libopus",
        "-b:a",
        "64k",
      ],
    );
    let files = vec![source];
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Webm,
      VideoQualityPreset::High,
      VideoResolutionPreset::Original,
    );

    let result = run_batch(&files, &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.status, VideoBatchStatus::Completed);
    assert_eq!(result.succeeded, 1);
    let item = &result.items[0];
    assert!(item.success, "unexpected failure: {:?}", item.error);

    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(item.output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!((probed.width, probed.height), (160, 120));
    assert_eq!(probed.audio_stream_count, 1);
    assert!(probed.duration_secs.unwrap_or(0.0) > 0.0);

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn landscape_no_upscale_keeps_source_dimensions() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let source = create_fixture(
      &binaries,
      &dir,
      "small.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=640x360:rate=15:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::P1080,
    );

    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(result.items[0].output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!((probed.width, probed.height), (640, 360));

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn portrait_preset_sizing() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();

    let big = create_fixture(
      &binaries,
      &dir,
      "big-portrait.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1080x1920:rate=10:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );
    let big_request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::P720,
    );
    let result = run_batch(&[big], &out, &big_request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(result.items[0].output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!((probed.width, probed.height), (720, 1280));

    let small = create_fixture(
      &binaries,
      &dir,
      "small-portrait.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=240x320:rate=10:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );
    let small_request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::P1080,
    );
    let result = run_batch(&[small], &out, &small_request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(result.items[0].output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!((probed.width, probed.height), (240, 320));

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn rotation_metadata_preserves_visible_orientation() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();

    // Landscape pixels carrying a 90 degree display rotation: the visible
    // orientation is portrait, so FFmpeg's autorotation must store the output
    // as 480x640 portrait pixels. The rotation is attached in a second,
    // stream-copy remux step because `-display_rotation` on a lavfi input is
    // not persisted by the mov muxer.
    let base = create_fixture(
      &binaries,
      &dir,
      "rotated-base.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=640x480:rate=10:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );
    let source = dir.join("rotated.mp4");
    let remux = Command::new(&binaries.ffmpeg)
      .args([
        "-hide_banner",
        "-y",
        "-v",
        "error",
        "-display_rotation",
        "90",
        "-i",
      ])
      .arg(&base)
      .args(["-c", "copy"])
      .arg(&source)
      .output()
      .expect("ffmpeg remux spawn");
    assert!(
      remux.status.success(),
      "rotation remux failed: {}",
      String::from_utf8_lossy(&remux.stderr)
    );

    let fixture_probe = probe::run_probe(&binaries.ffprobe, &source).unwrap();
    assert_eq!(
      fixture_probe.rotation.abs(),
      90,
      "fixture must carry rotation metadata for this test to be meaningful"
    );

    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::P480,
    );

    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(result.items[0].output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!(
      (probed.width, probed.height),
      (480, 640),
      "output must be stored as portrait pixels"
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn silent_source_produces_no_audio_and_no_warning() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let source = create_fixture(
      &binaries,
      &dir,
      "silent.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x120:rate=10:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );

    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let item = &result.items[0];
    assert!(
      item.warnings.is_none(),
      "no audio warning for silent source"
    );
    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(item.output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!(probed.audio_stream_count, 0);
    assert!(!probed.has_audio);

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn extra_audio_stream_is_omitted_with_warning() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let source = create_fixture(
      &binaries,
      &dir,
      "dual-audio.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=10:duration=0.3",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=0.3",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=880:duration=0.3",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-map",
        "2:a",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "64k",
      ],
    );
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );

    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let item = &result.items[0];
    let warnings = item
      .warnings
      .as_ref()
      .expect("extra audio warning expected");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "VIDEO_OMITTED_AUDIO");
    assert_eq!(
      warnings[0].message,
      "Extra audio stream omitted: only the first track is kept"
    );

    let probed = probe::run_probe(
      &binaries.ffprobe,
      Path::new(item.output_path.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!(probed.audio_stream_count, 1);

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn colliding_output_name_is_renamed_incrementally() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let source = create_fixture(
      &binaries,
      &dir,
      "collide.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x120:rate=10:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );
    std::fs::write(out.join("collide.mp4"), b"pre-existing").unwrap();
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );

    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let output_path = result.items[0].output_path.as_ref().unwrap();
    assert!(
      Path::new(output_path).ends_with("collide-2.mp4"),
      "expected collide-2.mp4, got {output_path}"
    );
    assert_eq!(
      std::fs::read(out.join("collide.mp4")).unwrap(),
      b"pre-existing"
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn corrupt_input_fails_without_leaving_files() {
    let Some(binaries) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let corrupt = dir.join("garbage.mp4");
    std::fs::write(&corrupt, b"this is definitely not a video file").unwrap();
    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );

    let result = run_batch(&[corrupt], &out, &request, &binaries, noop_emit).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.failed, 1);
    assert_eq!(
      result.items[0].error.as_ref().unwrap().code,
      "INVALID_VIDEO"
    );
    assert!(
      list_dir(&out).is_empty(),
      "no partial or output files should remain: {:?}",
      list_dir(&out)
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  // --- fake binary tests (no staged FFmpeg required) ---

  #[test]
  fn forced_child_failure_cleans_partial() {
    let dir = temp_dir();
    let out = temp_dir();
    let fakes = temp_dir();
    let source = dir.join("input.mp4");
    std::fs::write(&source, b"fake video bytes").unwrap();

    let binaries = VideoBinaries {
      ffmpeg: write_fake_cmd(&fakes, "ffmpeg.cmd", "@echo off\r\nexit /b 1\r\n"),
      ffprobe: write_fake_cmd(
        &fakes,
        "ffprobe.cmd",
        &format!("@echo off\r\necho {FAKE_PROBE_JSON}\r\n"),
      ),
    };

    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );
    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();

    assert_eq!(result.total, 1);
    assert_eq!(result.failed, 1);
    let item = &result.items[0];
    assert!(!item.success);
    assert_eq!(item.error.as_ref().unwrap().code, "ENCODE_FAILED");
    assert!(
      item
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("exited with code 1"),
      "failure should include the exit code: {:?}",
      item.error
    );
    assert!(
      list_dir(&out).is_empty(),
      "partial must be removed: {:?}",
      list_dir(&out)
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
    std::fs::remove_dir_all(&fakes).unwrap();
  }

  #[test]
  fn valid_output_without_reported_duration_is_kept() {
    let dir = temp_dir();
    let out = temp_dir();
    let fakes = temp_dir();
    let source = dir.join("input.mp4");
    std::fs::write(&source, b"fake video bytes").unwrap();

    let binaries = VideoBinaries {
      ffmpeg: write_fake_cmd(
        &fakes,
        "ffmpeg.cmd",
        &fake_ffmpeg_body("echo encoded video> \"%LAST%\""),
      ),
      ffprobe: write_fake_cmd(
        &fakes,
        "ffprobe.cmd",
        &format!("@echo off\r\necho {FAKE_PROBE_JSON_WITHOUT_DURATION}\r\n"),
      ),
    };

    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );
    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();

    assert_eq!(result.succeeded, 1, "{:?}", result.items[0].error);
    let output_path = result.items[0].output_path.as_ref().unwrap();
    assert!(Path::new(output_path).is_file());
    assert!(
      !list_dir(&out)
        .iter()
        .any(|name| name.contains(PARTIAL_EXTENSION_HINT)),
      "partial must be committed: {:?}",
      list_dir(&out)
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
    std::fs::remove_dir_all(&fakes).unwrap();
  }

  #[test]
  fn cancel_removes_partial_and_reports_canceled() {
    let dir = temp_dir();
    let out = temp_dir();
    let fakes = temp_dir();
    let source = dir.join("input.mp4");
    std::fs::write(&source, b"fake video bytes").unwrap();

    let binaries = VideoBinaries {
      ffmpeg: write_fake_cmd(
        &fakes,
        "ffmpeg.cmd",
        &fake_ffmpeg_body("type nul > \"%LAST%\"\r\nping -n 30 127.0.0.1 >nul"),
      ),
      ffprobe: write_fake_cmd(
        &fakes,
        "ffprobe.cmd",
        "@echo off\r\necho {\"streams\":[{\"codec_type\":\"video\",\"coded_width\":320,\"coded_height\":240,\"width\":320,\"height\":240}],\"format\":{\"duration\":\"60\"}}\r\n",
      ),
    };

    let job_id = unique_job_id();
    let request = request(
      &job_id,
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );

    let out_for_thread = out.clone();
    let handle = std::thread::spawn(move || {
      run_batch(&[source], &out_for_thread, &request, &binaries, noop_emit)
    });

    // Wait until the partial file exists; run_batch registers the job before
    // any file starts, so the job is registered by now.
    let mut partial_name: Option<String> = None;
    for _ in 0..50 {
      if let Some(found) = list_dir(&out)
        .into_iter()
        .find(|name| name.contains(PARTIAL_EXTENSION_HINT))
      {
        partial_name = Some(found);
        break;
      }
      std::thread::sleep(Duration::from_millis(100));
    }
    partial_name.expect("fake ffmpeg should create a partial file");
    assert!(cancel_job(&job_id), "job must be registered while running");

    let result = handle.join().unwrap().unwrap();

    assert!(
      !list_dir(&out)
        .iter()
        .any(|name| name.contains(PARTIAL_EXTENSION_HINT)),
      "partial must be removed in either outcome: {:?}",
      list_dir(&out)
    );
    match result.status {
      VideoBatchStatus::Canceled => {
        assert_eq!(result.items[0].error.as_ref().unwrap().code, "CANCELED");
        assert!(list_dir(&out).is_empty(), "no final output on cancel");
      }
      VideoBatchStatus::Completed => {
        // Race: the fake exited before the cancel landed. The empty partial
        // must have failed output inspection and been cleaned up.
        assert_eq!(
          result.items[0].error.as_ref().unwrap().code,
          "ENCODE_FAILED"
        );
        assert!(list_dir(&out).is_empty());
      }
    }
    assert!(!is_cancel_requested(&job_id), "job is unregistered on exit");

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
    std::fs::remove_dir_all(&fakes).unwrap();
  }

  #[test]
  fn exit_zero_with_garbage_output_fails_inspection() {
    let Some(staged) = staged_binaries() else {
      return;
    };
    let dir = temp_dir();
    let out = temp_dir();
    let fakes = temp_dir();
    let source = create_fixture(
      &staged,
      &dir,
      "input.mp4",
      &[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x120:rate=10:duration=0.3",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "35",
        "-pix_fmt",
        "yuv420p",
      ],
    );

    let binaries = VideoBinaries {
      ffmpeg: write_fake_cmd(
        &fakes,
        "ffmpeg.cmd",
        &fake_ffmpeg_body("echo not a video> \"%LAST%\""),
      ),
      ffprobe: staged.ffprobe,
    };

    let request = request(
      &unique_job_id(),
      &out,
      VideoOutputFormat::Mp4,
      VideoQualityPreset::Balanced,
      VideoResolutionPreset::Original,
    );
    let result = run_batch(&[source], &out, &request, &binaries, noop_emit).unwrap();

    assert_eq!(result.failed, 1);
    let error = result.items[0].error.as_ref().unwrap();
    assert_eq!(error.code, "ENCODE_FAILED");
    assert!(
      error.message.contains("invalid output"),
      "failure should report invalid output: {:?}",
      error
    );
    assert!(
      list_dir(&out).is_empty(),
      "partial must be removed: {:?}",
      list_dir(&out)
    );

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
    std::fs::remove_dir_all(&fakes).unwrap();
  }
}
