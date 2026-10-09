use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::errors::{ProcessingError, ProcessingErrorDto};
use crate::models::{
  OptimizePdfBatchResult, OptimizePdfFileResult, OptimizePdfsRequest, PdfOptimizationPreset,
  PdfOptimizationProgress, PdfOptimizeBatchStatus, PdfOptimizeOutcome,
};
use crate::services::export;

use super::qpdf;

pub const EVENT_PROGRESS: &str = "pdf-optimize-progress";

const STDERR_TAIL_LINES: usize = 10;
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const PARTIAL_HINT: &str = "part-";

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

/// Returns the exact qpdf arguments for a preset (excluding input/output and `--progress`).
///
/// Pin: all presets use `--compress-streams=y --decode-level=generalized --recompress-flate --compression-level=9 --object-streams=generate`.
/// Balanced additionally uses `--optimize-images --jpeg-quality=80`; Smaller uses `--optimize-images --jpeg-quality=55`.
/// Lossless must never include either lossy flag. We keep qpdf's default minimum image dimensions/area.
pub fn preset_args(preset: PdfOptimizationPreset) -> Vec<&'static str> {
  let mut args = vec![
    "--compress-streams=y",
    "--decode-level=generalized",
    "--recompress-flate",
    "--compression-level=9",
    "--object-streams=generate",
  ];
  match preset {
    PdfOptimizationPreset::Lossless => {}
    PdfOptimizationPreset::Balanced => {
      args.push("--optimize-images");
      args.push("--jpeg-quality=80");
    }
    PdfOptimizationPreset::Smaller => {
      args.push("--optimize-images");
      args.push("--jpeg-quality=55");
    }
  }
  args
}

/// Builds the full qpdf argument list for one file: options + `--progress` + infile + outfile.
pub fn build_qpdf_args(preset: PdfOptimizationPreset, input: &Path, output: &Path) -> Vec<String> {
  let mut args: Vec<String> = preset_args(preset)
    .into_iter()
    .map(|s| s.to_string())
    .collect();
  args.push("--progress".into());
  args.push(input.to_string_lossy().into_owned());
  args.push(output.to_string_lossy().into_owned());
  args
}

/// Parses a qpdf --progress line like `qpdf: /path/out.pdf: write progress: 17%`.
pub fn parse_progress_line(line: &str) -> Option<f64> {
  // Look for "write progress:" token.
  let idx = line.find("write progress:")?;
  let rest = &line[idx + "write progress:".len()..];
  let trimmed = rest.trim();
  // Expect like "17%" or "100%"
  let percent_str = trimmed.trim_end_matches('%').trim();
  // The line may contain extra after %, but spec shows just number%.
  // We'll extract leading number.
  let mut num_str = String::new();
  for ch in percent_str.chars() {
    if ch.is_ascii_digit() || ch == '.' {
      num_str.push(ch);
    } else if !num_str.is_empty() {
      break;
    }
  }
  if num_str.is_empty() {
    return None;
  }
  let val: f64 = num_str.parse().ok()?;
  Some(val.clamp(0.0, 100.0))
}

type EmitFn = Box<dyn Fn(&PdfOptimizationProgress) + Send + Sync>;

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
      Some((name, pct)) => (Some(name), Some(pct)),
      None => (None, None),
    };
    let progress = PdfOptimizationProgress {
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
  Optimized { output_size: u64 },
  AlreadyOptimized,
  Failed(ProcessingError),
  Canceled,
}

/// Processes PDF files sequentially through the bundled qpdf.
///
/// Each file is encoded to a uniquely named partial output in `output_dir`,
/// validated with `qpdf --check`, and only then committed to its collision-safe
/// final `-optimized.pdf` path when strictly smaller. Cancellation stops the
/// active child, prevents later files from starting, removes the partial, and
/// is reported as a typed `Canceled` batch status; files completed before
/// cancellation are kept. Unstarted files are not included in the result items
/// while the requested `total` retains the original file count.
pub fn run_batch<F>(
  files: &[PathBuf],
  output_dir: &Path,
  request: &OptimizePdfsRequest,
  emit: F,
) -> Result<OptimizePdfBatchResult, ProcessingError>
where
  F: Fn(&PdfOptimizationProgress) + Send + Sync + 'static,
{
  export::ensure_output_dir(output_dir)?;

  let qpdf_bin = qpdf::resolve_qpdf_binary()?;

  register_job(&request.job_id);
  let _guard = JobGuard(request.job_id.clone());

  let emitter = Arc::new(BatchEmitter {
    job_id: request.job_id.clone(),
    total_files: files.len() as u32,
    completed: AtomicU32::new(0),
    current_file: Mutex::new(None),
    emit: Box::new(emit),
  });

  let mut items: Vec<OptimizePdfFileResult> = Vec::new();
  let mut canceled = false;
  let total_requested = files.len() as u32;

  for (index, file) in files.iter().enumerate() {
    if is_cancel_requested(&request.job_id) {
      canceled = true;
      break;
    }

    let source_path = file.to_string_lossy().into_owned();
    let original_size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);

    // Revalidate constraints at processing boundary (frontend inspection is not trusted).
    if let Err(err) = super::inspect::validate_pdf_for_optimization(&qpdf_bin, file) {
      items.push(OptimizePdfFileResult {
        source_path,
        outcome: PdfOptimizeOutcome::Failed,
        original_size,
        output_path: None,
        output_size: None,
        error: Some(err.into_dto()),
        warnings: None,
      });
      emitter.completed.store(index as u32 + 1, Ordering::Relaxed);
      emitter.finish_file();
      continue;
    }

    // Reserve collision-safe final name: <stem>-optimized.pdf
    let final_path = match export::resolve_output_path(output_dir, file, "pdf", Some("-optimized"))
    {
      Ok(p) => p,
      Err(e) => {
        items.push(OptimizePdfFileResult {
          source_path,
          outcome: PdfOptimizeOutcome::Failed,
          original_size,
          output_path: None,
          output_size: None,
          error: Some(e.into_dto()),
          warnings: None,
        });
        emitter.completed.store(index as u32 + 1, Ordering::Relaxed);
        emitter.finish_file();
        continue;
      }
    };
    let partial_path = resolve_partial_path(output_dir, &final_path);

    let file_name = file
      .file_name()
      .map(|n| n.to_string_lossy().into_owned())
      .unwrap_or_else(|| source_path.clone());
    emitter.completed.store(index as u32, Ordering::Relaxed);
    emitter.start_file(&file_name);

    let outcome = optimize_one_file(
      &qpdf_bin,
      request.preset,
      file,
      &final_path,
      &partial_path,
      &emitter,
      &request.job_id,
    );

    let was_canceled = matches!(outcome, EncodeOutcome::Canceled);
    match outcome {
      EncodeOutcome::Optimized { output_size } => {
        items.push(OptimizePdfFileResult {
          source_path,
          outcome: PdfOptimizeOutcome::Optimized,
          original_size,
          output_path: Some(final_path.to_string_lossy().into_owned()),
          output_size: Some(output_size),
          error: None,
          warnings: None,
        });
      }
      EncodeOutcome::AlreadyOptimized => {
        items.push(OptimizePdfFileResult {
          source_path,
          outcome: PdfOptimizeOutcome::AlreadyOptimized,
          original_size,
          output_path: None,
          output_size: None,
          error: None,
          warnings: None,
        });
      }
      EncodeOutcome::Failed(err) => {
        items.push(OptimizePdfFileResult {
          source_path,
          outcome: PdfOptimizeOutcome::Failed,
          original_size,
          output_path: None,
          output_size: None,
          error: Some(err.into_dto()),
          warnings: None,
        });
      }
      EncodeOutcome::Canceled => {
        items.push(OptimizePdfFileResult {
          source_path,
          outcome: PdfOptimizeOutcome::Canceled,
          original_size,
          output_path: None,
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
    PdfOptimizeBatchStatus::Canceled
  } else {
    PdfOptimizeBatchStatus::Completed
  };

  let optimized = items
    .iter()
    .filter(|i| i.outcome == PdfOptimizeOutcome::Optimized)
    .count() as u32;
  let already_optimized = items
    .iter()
    .filter(|i| i.outcome == PdfOptimizeOutcome::AlreadyOptimized)
    .count() as u32;
  let failed = items
    .iter()
    .filter(|i| i.outcome == PdfOptimizeOutcome::Failed)
    .count() as u32;

  Ok(OptimizePdfBatchResult {
    status,
    total: total_requested,
    optimized,
    already_optimized,
    failed,
    items,
  })
}

fn optimize_one_file(
  qpdf_bin: &Path,
  preset: PdfOptimizationPreset,
  input: &Path,
  final_path: &Path,
  partial: &Path,
  emitter: &Arc<BatchEmitter>,
  job_id: &str,
) -> EncodeOutcome {
  let args = build_qpdf_args(preset, input, partial);

  let mut child = match Command::new(qpdf_bin)
    .args(&args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
  {
    Ok(c) => c,
    Err(_) => {
      return EncodeOutcome::Failed(ProcessingError::pdf_engine_unavailable(
        "Could not start PDF engine",
      ))
    }
  };

  let stdout = child.stdout.take();
  let stderr = child.stderr.take();

  let emitter_clone = Arc::clone(emitter);
  let stdout_handle = std::thread::spawn(move || {
    let Some(stdout) = stdout else {
      return;
    };
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
      let Ok(line) = line else { break };
      if let Some(pct) = parse_progress_line(&line) {
        emitter_clone.update_percent(pct);
      }
    }
  });

  let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
  let tail_clone = Arc::clone(&stderr_tail);
  let stderr_handle = std::thread::spawn(move || {
    let Some(stderr) = stderr else {
      return;
    };
    let reader = BufReader::new(stderr);
    for line in reader.lines() {
      let Ok(line) = line else { break };
      let sanitized = sanitize_stderr_line(&line);
      let mut tail = tail_clone.lock().unwrap();
      if tail.len() >= STDERR_TAIL_LINES {
        tail.pop_front();
      }
      tail.push_back(sanitized);
    }
  });

  // Poll for cancellation and child exit
  let mut canceled = false;
  let mut exit_status = None;
  let mut status_err: Option<std::io::Error> = None;
  loop {
    if is_cancel_requested(job_id) {
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
      Err(e) => {
        status_err = Some(e);
        let _ = child.kill();
        let _ = child.wait();
        break;
      }
    }
  }

  let _ = stdout_handle.join();
  let _ = stderr_handle.join();

  if canceled || is_cancel_requested(job_id) {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Canceled;
  }

  if let Some(e) = status_err {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::processing_failed(format!(
      "Could not read PDF engine status: {e}"
    )));
  }

  let Some(status) = exit_status else {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::processing_failed(
      "PDF engine exited unexpectedly",
    ));
  };

  // Strict warning handling: only exit 0 is success; 3 is warnings, 2 is errors.
  if !status.success() {
    let _ = std::fs::remove_file(partial);
    let code = status
      .code()
      .map(|c| c.to_string())
      .unwrap_or_else(|| "unknown".into());
    let mut msg = format!("PDF optimization failed (exit {code})");
    let tail = tail_message(&stderr_tail);
    if !tail.is_empty() {
      msg.push_str(&format!(": {tail}"));
    }
    // Map known error patterns to stable codes?
    // Keep as invalid_pdf for most, but preserve distinction for engine unavailable.
    return EncodeOutcome::Failed(ProcessingError::invalid_pdf(msg));
  }

  // Keep validation cancelable too: qpdf --check is another native process and
  // must not make a canceled batch wait indefinitely.
  let mut check_child = match Command::new(qpdf_bin)
    .arg("--check")
    .arg(partial)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
  {
    Ok(child) => child,
    Err(_) => {
      let _ = std::fs::remove_file(partial);
      return EncodeOutcome::Failed(ProcessingError::pdf_engine_unavailable(
        "Could not start PDF engine",
      ));
    }
  };
  loop {
    if is_cancel_requested(job_id) {
      let _ = check_child.kill();
      let _ = check_child.wait();
      let _ = std::fs::remove_file(partial);
      return EncodeOutcome::Canceled;
    }
    match check_child.try_wait() {
      Ok(Some(status)) if status.success() => break,
      Ok(Some(_)) => {
        let _ = std::fs::remove_file(partial);
        return EncodeOutcome::Failed(ProcessingError::invalid_pdf(
          "Optimized PDF failed validation",
        ));
      }
      Ok(None) => std::thread::sleep(POLL_INTERVAL),
      Err(e) => {
        let _ = check_child.kill();
        let _ = check_child.wait();
        let _ = std::fs::remove_file(partial);
        return EncodeOutcome::Failed(ProcessingError::processing_failed(format!(
          "Could not read PDF validation status: {e}"
        )));
      }
    }
  }

  // Compare byte sizes before commit: rename only when strictly smaller.
  let original_size = match std::fs::metadata(input) {
    Ok(meta) => meta.len(),
    Err(_) => {
      let _ = std::fs::remove_file(partial);
      return EncodeOutcome::Failed(ProcessingError::file_not_found(
        "Source PDF is no longer available",
      ));
    }
  };
  let partial_size = match std::fs::metadata(partial) {
    Ok(meta) => meta.len(),
    Err(e) => {
      return EncodeOutcome::Failed(ProcessingError::write_failed(format!(
        "Could not read optimized output: {e}"
      )));
    }
  };
  if partial_size == 0 {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::write_failed(
      "PDF engine produced an empty output",
    ));
  }
  if partial_size >= original_size {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::AlreadyOptimized;
  }

  if is_cancel_requested(job_id) {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Canceled;
  }

  if let Err(e) = std::fs::rename(partial, final_path) {
    let _ = std::fs::remove_file(partial);
    return EncodeOutcome::Failed(ProcessingError::write_failed(format!(
      "Could not finalize output '{}': {e}",
      final_path.display()
    )));
  }

  EncodeOutcome::Optimized {
    output_size: partial_size,
  }
}

fn sanitize_stderr_line(line: &str) -> String {
  line
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .chars()
    .take(500)
    .collect()
}

fn tail_message(tail: &Mutex<VecDeque<String>>) -> String {
  let tail = tail.lock().unwrap();
  tail.iter().cloned().collect::<Vec<_>>().join(" | ")
}

fn resolve_partial_path(output_dir: &Path, final_path: &Path) -> PathBuf {
  let stem = final_path
    .file_stem()
    .map(|s| s.to_string_lossy().into_owned())
    .unwrap_or_else(|| "output".into());
  loop {
    let n = PARTIAL_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    let candidate = output_dir.join(format!("{stem}.{PARTIAL_HINT}{n}.pdf"));
    if !candidate.exists() {
      return candidate;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::AtomicU32 as A32;

  #[test]
  fn preset_args_match_pinned_table() {
    let lossless = preset_args(PdfOptimizationPreset::Lossless);
    assert_eq!(
      lossless,
      vec![
        "--compress-streams=y",
        "--decode-level=generalized",
        "--recompress-flate",
        "--compression-level=9",
        "--object-streams=generate"
      ]
    );
    // Balanced must include lossy flags
    let balanced = preset_args(PdfOptimizationPreset::Balanced);
    assert!(balanced.contains(&"--optimize-images"));
    assert!(balanced.contains(&"--jpeg-quality=80"));
    assert_eq!(balanced.len(), 7);
    // Smaller
    let smaller = preset_args(PdfOptimizationPreset::Smaller);
    assert!(smaller.contains(&"--optimize-images"));
    assert!(smaller.contains(&"--jpeg-quality=55"));
    assert_eq!(smaller.len(), 7);
  }

  #[test]
  fn lossless_never_includes_lossy_flags() {
    let lossless = preset_args(PdfOptimizationPreset::Lossless);
    assert!(!lossless.iter().any(|a| a.contains("jpeg-quality")));
    assert!(!lossless.contains(&"--optimize-images"));
  }

  #[test]
  fn balanced_and_smaller_differ_only_in_quality() {
    let balanced = preset_args(PdfOptimizationPreset::Balanced);
    let smaller = preset_args(PdfOptimizationPreset::Smaller);
    assert_ne!(balanced, smaller);
    // Only jpeg-quality differs
    let b_without_quality: Vec<_> = balanced
      .into_iter()
      .filter(|a| !a.starts_with("--jpeg-quality"))
      .collect();
    let s_without_quality: Vec<_> = smaller
      .into_iter()
      .filter(|a| !a.starts_with("--jpeg-quality"))
      .collect();
    assert_eq!(b_without_quality, s_without_quality);
  }

  #[test]
  fn parses_progress_lines() {
    assert_eq!(
      parse_progress_line("qpdf: /tmp/out.pdf: write progress: 0%"),
      Some(0.0)
    );
    assert_eq!(
      parse_progress_line("qpdf: /tmp/out.pdf: write progress: 17%"),
      Some(17.0)
    );
    assert_eq!(
      parse_progress_line("qpdf: /tmp/out.pdf: write progress: 100%"),
      Some(100.0)
    );
    assert_eq!(parse_progress_line("not a progress line"), None);
    assert_eq!(
      parse_progress_line("qpdf: write progress:  42%  "),
      Some(42.0)
    );
  }

  #[test]
  fn stderr_sanitization_is_unicode_safe_and_bounded() {
    let input = format!("  qpdf   error\t{}  ", "é".repeat(600));
    let sanitized = sanitize_stderr_line(&input);
    assert!(!sanitized.contains("  "));
    assert_eq!(sanitized.chars().count(), 500);
    assert!(sanitized.starts_with("qpdf error "));
  }

  #[test]
  fn build_args_include_progress_and_paths() {
    let input = Path::new("C:/in/input.pdf");
    let output = Path::new("C:/out/input.part-1.pdf");
    let args = build_qpdf_args(PdfOptimizationPreset::Balanced, input, output);
    assert!(args.contains(&"--progress".to_string()));
    assert!(args.contains(&"C:/in/input.pdf".to_string()));
    assert!(args.contains(&"C:/out/input.part-1.pdf".to_string()));
    assert!(args.contains(&"--jpeg-quality=80".to_string()));
  }

  #[test]
  fn partial_paths_are_unique() {
    let dir = std::env::temp_dir().join(format!(
      "tangan-pdf-optimize-test-{}-{}",
      std::process::id(),
      A32::new(0).fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let final_path = dir.join("doc-optimized.pdf");
    let first = resolve_partial_path(&dir, &final_path);
    assert!(first.to_string_lossy().contains(".part-"));
    assert_eq!(first.extension().and_then(|s| s.to_str()), Some("pdf"));
    std::fs::write(&first, b"occupy").unwrap();
    let second = resolve_partial_path(&dir, &final_path);
    assert_ne!(first, second);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn cancel_unknown_job_returns_false() {
    assert!(!cancel_job("no-such-job-ever-pdf"));
  }

  #[test]
  fn cancel_running_job_flips_state() {
    let job_id = format!("pdf-test-job-{}-{}", std::process::id(), 12345);
    register_job(&job_id);
    assert!(!is_cancel_requested(&job_id));
    assert!(cancel_job(&job_id));
    assert!(is_cancel_requested(&job_id));
    assert!(!cancel_job(&job_id));
    unregister_job(&job_id);
    assert!(!is_cancel_requested(&job_id));
  }

  // Gated tests that require staged qpdf binary
  fn staged_qpdf() -> Option<PathBuf> {
    match qpdf::resolve_qpdf_binary() {
      Ok(p) => Some(p),
      Err(_) => {
        eprintln!("skipping gated test: qpdf not staged (run src-tauri/scripts/prepare-qpdf.ps1)");
        None
      }
    }
  }

  fn temp_dir() -> PathBuf {
    static COUNTER: A32 = A32::new(0);
    let dir = std::env::temp_dir().join(format!(
      "tangan-pdf-optimize-gated-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_minimal_pdf(path: &Path, text: &str) {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
      "Type" => "Font",
      "Subtype" => "Type1",
      "BaseFont" => "Helvetica",
      "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
      "Font" => dictionary! {
        "F1" => font_id,
      },
    });
    let content = Content {
      operations: vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 12.into()]),
        Operation::new("Td", vec![72.into(), 712.into()]),
        Operation::new("Tj", vec![Object::string_literal(text)]),
        Operation::new("ET", vec![]),
      ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
      "Type" => "Page",
      "Parent" => pages_id,
      "Contents" => content_id,
      "Resources" => resources_id,
      "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    let pages = dictionary! {
      "Type" => "Pages",
      "Kids" => vec![page_id.into()],
      "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
      "Type" => "Catalog",
      "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
  }

  fn write_multipage_pdf(path: &Path, count: usize, text: &str) {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
      "Type" => "Font",
      "Subtype" => "Type1",
      "BaseFont" => "Helvetica",
      "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add_object(dictionary! {
      "Font" => dictionary! {
        "F1" => font_id,
      },
    });
    let mut kids = Vec::new();
    for _ in 0..count {
      let content = Content {
        operations: vec![
          Operation::new("BT", vec![]),
          Operation::new("Tf", vec!["F1".into(), 12.into()]),
          Operation::new("Td", vec![72.into(), 712.into()]),
          Operation::new("Tj", vec![Object::string_literal(text)]),
          Operation::new("ET", vec![]),
        ],
      };
      let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
      let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
      });
      kids.push(page_id.into());
    }
    let pages = dictionary! {
      "Type" => "Pages",
      "Kids" => kids,
      "Count" => count as i64,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
      "Type" => "Catalog",
      "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
  }

  #[test]
  fn qpdf_json_signature_detection_no_sig() {
    let Some(bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let pdf = dir.join("nosig.pdf");
    write_minimal_pdf(&pdf, "no signature");
    let has_sig = super::super::inspect::has_signature_field(&bin, &pdf).unwrap();
    assert!(!has_sig);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn qpdf_encrypted_detection() {
    let Some(bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let plain = dir.join("plain.pdf");
    write_minimal_pdf(&plain, "plain");
    let enc = dir.join("enc.pdf");
    // encrypt with qpdf
    let out = Command::new(&bin)
      .args([
        "--encrypt",
        "user",
        "owner",
        "256",
        "--",
        &plain.to_string_lossy(),
        &enc.to_string_lossy(),
      ])
      .output()
      .unwrap();
    assert!(
      out.status.success(),
      "encrypt failed: {:?}",
      String::from_utf8_lossy(&out.stderr)
    );
    let plain_enc = super::super::inspect::is_pdf_encrypted(&bin, &plain).unwrap();
    assert!(!plain_enc);
    let enc_enc = super::super::inspect::is_pdf_encrypted(&bin, &enc).unwrap();
    assert!(enc_enc);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn successful_structural_optimization_and_validation() {
    let Some(_bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let out = temp_dir();
    let pdf = dir.join("doc.pdf");
    // Create a PDF with some text; qpdf should be able to optimize it (even if not much saving, it should at least run)
    write_minimal_pdf(&pdf, &"Hello ".repeat(200));
    let request = OptimizePdfsRequest {
      files: vec![pdf.to_string_lossy().into_owned()],
      output_directory: out.to_string_lossy().into_owned(),
      preset: PdfOptimizationPreset::Lossless,
      job_id: format!("test-{}-structural", std::process::id()),
    };
    let result = run_batch(std::slice::from_ref(&pdf), &out, &request, |_| {}).unwrap();
    // Should have 1 item, either optimized or alreadyOptimized, not failed
    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    let item = &result.items[0];
    assert!(
      matches!(
        item.outcome,
        PdfOptimizeOutcome::Optimized | PdfOptimizeOutcome::AlreadyOptimized
      ),
      "unexpected outcome {:?} error {:?}",
      item.outcome,
      item.error
    );
    if item.outcome == PdfOptimizeOutcome::Optimized {
      let out_path = item.output_path.as_ref().unwrap();
      assert!(Path::new(out_path).exists());
      // Validate qpdf --check passes (already done in optimize, but double check)
      let bin = staged_qpdf().unwrap();
      let check = Command::new(&bin)
        .arg("--check")
        .arg(out_path)
        .output()
        .unwrap();
      assert!(check.status.success());
      // No partial left
      assert!(!out.join("doc.part-").exists());
      assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
    } else {
      // Already optimized: no output file
      assert!(item.output_path.is_none());
      assert!(std::fs::read_dir(&out).unwrap().count() == 0);
    }
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn already_optimized_equality_cleanup() {
    let Some(_bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let out = temp_dir();
    // Create a PDF that is already heavily optimized: run qpdf lossless once, then try again
    let pdf = dir.join("source.pdf");
    write_minimal_pdf(&pdf, "small content");
    // First optimize to get an optimized file
    let bin = staged_qpdf().unwrap();
    let first_out = out.join("source-optimized.pdf");
    // Use qpdf directly to create an optimized version
    let args = build_qpdf_args(PdfOptimizationPreset::Lossless, &pdf, &first_out);
    let status = Command::new(&bin).args(&args).output().unwrap().status;
    assert!(status.success());
    // Now try to optimize the already-optimized file; it should be alreadyOptimized (no new file)
    // Place the optimized file as source and try to optimize again to same out dir (collision case will create -optimized-2.pdf if it were to succeed)
    let request = OptimizePdfsRequest {
      files: vec![first_out.to_string_lossy().into_owned()],
      output_directory: out.to_string_lossy().into_owned(),
      preset: PdfOptimizationPreset::Lossless,
      job_id: format!("test-{}-already", std::process::id()),
    };
    let result = run_batch(std::slice::from_ref(&first_out), &out, &request, |_| {}).unwrap();
    assert_eq!(result.items.len(), 1);
    // The second optimization may be optimized again or alreadyOptimized depending on size; but ensure no partial remains and at most one new file
    let files: Vec<_> = std::fs::read_dir(&out)
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
      .collect();
    // Should not have left partial files
    assert!(!files.iter().any(|n| n.contains("part-")));
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn collision_naming_is_incremental() {
    let Some(_bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let out = temp_dir();
    let pdf = dir.join("report.pdf");
    write_minimal_pdf(&pdf, &"content ".repeat(500));
    // Pre-create a collision file
    std::fs::write(out.join("report-optimized.pdf"), b"existing").unwrap();
    let request = OptimizePdfsRequest {
      files: vec![pdf.to_string_lossy().into_owned()],
      output_directory: out.to_string_lossy().into_owned(),
      preset: PdfOptimizationPreset::Lossless,
      job_id: format!("test-{}-collision", std::process::id()),
    };
    let result = run_batch(std::slice::from_ref(&pdf), &out, &request, |_| {}).unwrap();
    assert_eq!(result.items.len(), 1);
    if let Some(out_path) = &result.items[0].output_path {
      assert!(
        out_path.ends_with("report-optimized-2.pdf") || out_path.ends_with("report-optimized.pdf")
      );
      // If it was alreadyOptimized, no file; but we had a pre-existing collision, so if optimized, it should be -2
      if result.items[0].outcome == PdfOptimizeOutcome::Optimized {
        assert!(out_path.ends_with("report-optimized-2.pdf"));
        // Original collision preserved
        assert_eq!(
          std::fs::read(out.join("report-optimized.pdf")).unwrap(),
          b"existing"
        );
      }
    }
    // No partials
    let names: Vec<_> = std::fs::read_dir(&out)
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
      .collect();
    assert!(!names.iter().any(|n| n.contains("part-")));
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn batch_isolation_and_no_partial_residue() {
    let Some(_bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let out = temp_dir();
    let good = dir.join("good.pdf");
    write_minimal_pdf(&good, "good");
    let corrupt = dir.join("corrupt.pdf");
    std::fs::write(&corrupt, b"%PDF-1.4\ncorrupt\n%%EOF").unwrap();
    let second_good = dir.join("good2.pdf");
    std::fs::copy(&good, &second_good).unwrap();
    let files = vec![good, corrupt, second_good];
    let request = OptimizePdfsRequest {
      files: files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect(),
      output_directory: out.to_string_lossy().into_owned(),
      preset: PdfOptimizationPreset::Lossless,
      job_id: format!("test-{}-isolation", std::process::id()),
    };
    let result = run_batch(&files, &out, &request, |_| {}).unwrap();
    assert_eq!(result.total, 3);
    assert_eq!(result.items.len(), 3);
    // Ensure one failed (corrupt) and others are optimized or already
    let failed = result
      .items
      .iter()
      .filter(|i| i.outcome == PdfOptimizeOutcome::Failed)
      .count();
    assert_eq!(failed, 1);
    // No partial files
    let names: Vec<_> = std::fs::read_dir(&out)
      .unwrap()
      .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
      .collect();
    assert!(!names.iter().any(|n| n.contains("part-")));
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn over_page_limit_is_rejected() {
    let Some(_bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let out = temp_dir();
    let pdf = dir.join("many.pdf");
    write_multipage_pdf(&pdf, 501, "page");
    let request = OptimizePdfsRequest {
      files: vec![pdf.to_string_lossy().into_owned()],
      output_directory: out.to_string_lossy().into_owned(),
      preset: PdfOptimizationPreset::Lossless,
      job_id: format!("test-{}-pagelimit", std::process::id()),
    };
    let result = run_batch(std::slice::from_ref(&pdf), &out, &request, |_| {}).unwrap();
    assert_eq!(result.items[0].outcome, PdfOptimizeOutcome::Failed);
    assert!(result.items[0].error.as_ref().unwrap().code == "LIMIT_EXCEEDED");
    assert!(std::fs::read_dir(&out).unwrap().count() == 0);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn corrupt_pdf_is_failed_and_no_partial() {
    let Some(_bin) = staged_qpdf() else { return };
    let dir = temp_dir();
    let out = temp_dir();
    let pdf = dir.join("corrupt.pdf");
    std::fs::write(&pdf, b"%PDF-1.4\nxxx\n%%EOF").unwrap();
    let request = OptimizePdfsRequest {
      files: vec![pdf.to_string_lossy().into_owned()],
      output_directory: out.to_string_lossy().into_owned(),
      preset: PdfOptimizationPreset::Lossless,
      job_id: format!("test-{}-corrupt2", std::process::id()),
    };
    let result = run_batch(std::slice::from_ref(&pdf), &out, &request, |_| {}).unwrap();
    assert_eq!(result.items[0].outcome, PdfOptimizeOutcome::Failed);
    assert!(std::fs::read_dir(&out).unwrap().count() == 0);
    // No partial anywhere
    let out_files: Vec<_> = std::fs::read_dir(&out).unwrap().collect();
    assert!(out_files.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }
}
