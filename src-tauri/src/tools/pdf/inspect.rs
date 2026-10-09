use std::path::Path;
use std::process::Command;

use crate::errors::ProcessingError;
use crate::models::InputFile;

pub const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024; // 100 MiB
pub const MAX_PDF_PAGES: u32 = 500;
const SUPPORTED_EXTENSIONS: &[&str] = &["pdf"];

/// Inspects each path and returns one `InputFile` per PDF.
///
/// A directory path is expanded shallowly: its direct children that look like
/// `.pdf` files are inspected (no recursion). A file path yields itself.
pub fn inspect_pdf_paths(paths: Vec<String>) -> Vec<InputFile> {
  let mut out = Vec::new();
  for path in paths {
    match std::fs::metadata(&path) {
      Ok(meta) if meta.is_dir() => out.extend(expand_pdf_directory(&path)),
      Ok(meta) if meta.is_file() => out.push(inspect_pdf_file(&path, meta.len())),
      Ok(_) => out.push(InputFile::invalid(&path, 0, "Not a regular file")),
      Err(_) => out.push(InputFile::invalid(&path, 0, "File not found")),
    }
  }
  out
}

fn expand_pdf_directory(path: &str) -> Vec<InputFile> {
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
      if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return None;
      }
      let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
      Some(inspect_pdf_file(&file_path.to_string_lossy(), size))
    })
    .collect()
}

pub fn inspect_pdf_file(path: &str, size: u64) -> InputFile {
  let mut file = InputFile::new(path, size);

  if size == 0 {
    file.status = "invalid".into();
    file.error = Some("File is empty".into());
    return file;
  }

  if size > MAX_PDF_BYTES {
    file.status = "invalid".into();
    file.error = Some("File exceeds maximum size limit (100 MB)".into());
    return file;
  }

  let extension = file.extension.to_lowercase();
  if extension != "pdf" {
    file.status = "invalid".into();
    file.error = Some("Unsupported format (expected .pdf)".into());
    return file;
  }

  // PDF readers commonly tolerate leading bytes, so inspect the first 1 KiB.
  let mut header = [0u8; 1024];
  match std::fs::File::open(path) {
    Ok(mut handle) => {
      use std::io::Read;
      match handle.read(&mut header) {
        Ok(bytes_read) if bytes_read >= 4 => {
          let slice = &header[..bytes_read];
          if slice.windows(5).any(|window| window == b"%PDF-") {
            file.status = "ready".into();
          } else {
            file.status = "invalid".into();
            file.error = Some("Not a valid PDF file".into());
          }
        }
        _ => {
          file.status = "invalid".into();
          file.error = Some("Could not read file header".into());
        }
      }
    }
    Err(_) => {
      file.status = "invalid".into();
      file.error = Some("Could not read file".into());
    }
  }

  file
}

/// Optimization-specific inspection that uses the bundled qpdf for strict checks.
///
/// Unlike `inspect_pdf_paths`, this path rejects encrypted PDFs and any PDF
/// containing a `/Sig` form field, enforces page limits via qpdf, and surfaces
/// `PDF_ENGINE_UNAVAILABLE` when the engine is not staged. The shared
/// lightweight inspection (`inspect_pdf_file`) remains unchanged so the existing
/// PDF-to-Markdown tool does not depend on qpdf.
pub fn inspect_pdfs_for_optimization(paths: Vec<String>) -> Vec<InputFile> {
  let mut out = Vec::new();
  for path in paths {
    match std::fs::metadata(&path) {
      Ok(meta) if meta.is_dir() => out.extend(expand_pdf_directory_for_optimization(&path)),
      Ok(meta) if meta.is_file() => out.push(inspect_pdf_file_for_optimization(&path, meta.len())),
      Ok(_) => out.push(InputFile::invalid(&path, 0, "Not a regular file")),
      Err(_) => out.push(InputFile::invalid(&path, 0, "File not found")),
    }
  }
  out
}

fn expand_pdf_directory_for_optimization(path: &str) -> Vec<InputFile> {
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
      if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return None;
      }
      let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
      Some(inspect_pdf_file_for_optimization(
        &file_path.to_string_lossy(),
        size,
      ))
    })
    .collect()
}

pub fn inspect_pdf_file_for_optimization(path: &str, size: u64) -> InputFile {
  // Start from the shared lightweight inspection.
  let base = inspect_pdf_file(path, size);
  if base.status == "invalid" {
    return base;
  }

  // Strict qpdf-dependent checks.
  let qpdf = match crate::tools::pdf::qpdf::resolve_qpdf_binary() {
    Ok(bin) => bin,
    Err(_) => {
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some("PDF engine is not available".into());
      return file;
    }
  };

  // Check encryption before commands that require opening the document so a
  // protected PDF receives the specific, actionable validation message.
  match is_pdf_encrypted(&qpdf, Path::new(path)) {
    Ok(true) => {
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some("PDF is password-protected or encrypted".into());
      return file;
    }
    Err(e) => {
      // Treat qpdf errors (corrupt, unreadable) as invalid PDF.
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some(map_qpdf_inspection_error(e));
      return file;
    }
    _ => {}
  }

  // Page limit via qpdf.
  match get_pdf_page_count(&qpdf, Path::new(path)) {
    Ok(count) if count > MAX_PDF_PAGES => {
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some(format!(
        "PDF exceeds maximum limit of {} pages (found {})",
        MAX_PDF_PAGES, count
      ));
      return file;
    }
    Err(e) => {
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some(map_qpdf_inspection_error(e));
      return file;
    }
    _ => {}
  }

  // Signature field check.
  match has_signature_field(&qpdf, Path::new(path)) {
    Ok(true) => {
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some("PDF contains a digital signature and cannot be optimized".into());
      return file;
    }
    Err(e) => {
      let mut file = base;
      file.status = "invalid".into();
      file.error = Some(map_qpdf_inspection_error(e));
      return file;
    }
    _ => {}
  }

  base
}

fn map_qpdf_inspection_error(err: ProcessingError) -> String {
  match err {
    ProcessingError::InvalidPdf { message } => message,
    ProcessingError::PdfEngineUnavailable { message } => message,
    ProcessingError::FileNotFound { message } => message,
    ProcessingError::PermissionDenied { message } => message,
    ProcessingError::EncryptedPdf { message } => message,
    _ => "PDF could not be inspected".into(),
  }
}

fn get_pdf_page_count(qpdf_bin: &Path, pdf: &Path) -> Result<u32, ProcessingError> {
  let output = Command::new(qpdf_bin)
    .arg("--show-npages")
    .arg(pdf)
    .output()
    .map_err(|_| ProcessingError::pdf_engine_unavailable("Could not start PDF engine"))?;
  if !output.status.success() {
    return Err(ProcessingError::invalid_pdf("This PDF could not be opened"));
  }
  let stdout = String::from_utf8_lossy(&output.stdout);
  let trimmed = stdout.trim();
  trimmed
    .parse::<u32>()
    .map_err(|_| ProcessingError::invalid_pdf("This PDF could not be opened"))
}

pub fn is_pdf_encrypted(qpdf_bin: &Path, pdf: &Path) -> Result<bool, ProcessingError> {
  let output = Command::new(qpdf_bin)
    .arg("--is-encrypted")
    .arg(pdf)
    .output()
    .map_err(|_| ProcessingError::pdf_engine_unavailable("Could not start PDF engine"))?;
  // qpdf --is-encrypted: 0 = encrypted, 2 = not encrypted. Any warning or
  // diagnostic output is rejected so damaged files are never silently accepted.
  if output.status.success() {
    return Ok(true);
  }
  // Exit code 2 means not encrypted for valid files; verify it wasn't a corrupt-file error.
  if let Some(code) = output.status.code() {
    if code == 2 && output.stderr.is_empty() {
      return Ok(false);
    }
    if code == 3 {
      return Err(ProcessingError::invalid_pdf("This PDF could not be opened"));
    }
  }
  // Fallback: treat as invalid pdf.
  Err(ProcessingError::invalid_pdf("This PDF could not be opened"))
}

pub fn has_signature_field(qpdf_bin: &Path, pdf: &Path) -> Result<bool, ProcessingError> {
  let output = Command::new(qpdf_bin)
    .args(["--json", "--json-key=acroform"])
    .arg(pdf)
    .output()
    .map_err(|_| ProcessingError::pdf_engine_unavailable("Could not start PDF engine"))?;
  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("invalid password") {
      return Err(ProcessingError::encrypted_pdf(
        "PDF is password-protected or encrypted",
      ));
    }
    // Treat warnings as invalid (corrupt) rather than silent repair.
    return Err(ProcessingError::invalid_pdf("This PDF could not be opened"));
  }
  let stdout = String::from_utf8_lossy(&output.stdout);
  let json: serde_json::Value = serde_json::from_str(&stdout)
    .map_err(|_| ProcessingError::invalid_pdf("This PDF could not be opened"))?;
  let fields = json
    .get("acroform")
    .and_then(|v| v.get("fields"))
    .and_then(|v| v.as_array());
  if let Some(fields) = fields {
    for field in fields {
      if let Some(ft) = field.get("fieldtype").and_then(|v| v.as_str()) {
        if ft == "/Sig" {
          return Ok(true);
        }
      }
    }
  }
  Ok(false)
}

// Exposed for processing revalidation (not via InputFile).
pub fn validate_pdf_for_optimization(qpdf_bin: &Path, path: &Path) -> Result<(), ProcessingError> {
  let meta = std::fs::metadata(path).map_err(|_| {
    if !path.exists() {
      ProcessingError::file_not_found("File not found")
    } else {
      ProcessingError::permission_denied("Could not read file")
    }
  })?;
  if !meta.is_file() {
    return Err(ProcessingError::invalid_pdf("PDF path is not a file"));
  }
  let size = meta.len();
  if size == 0 {
    return Err(ProcessingError::invalid_pdf("PDF file is empty"));
  }
  if size > MAX_PDF_BYTES {
    return Err(ProcessingError::limit_exceeded(
      "PDF exceeds maximum size limit (100 MB)",
    ));
  }
  let ext = path
    .extension()
    .and_then(|e| e.to_str())
    .unwrap_or("")
    .to_ascii_lowercase();
  if ext != "pdf" {
    return Err(ProcessingError::unsupported_format(
      "Unsupported format (expected .pdf)",
    ));
  }
  // Light header check (same as inspect).
  let mut header = [0u8; 1024];
  let mut f = std::fs::File::open(path)
    .map_err(|_| ProcessingError::permission_denied("Could not read file"))?;
  use std::io::Read;
  let n = f
    .read(&mut header)
    .map_err(|_| ProcessingError::invalid_pdf("Could not read file header"))?;
  if n < 4 || !header[..n].windows(5).any(|w| w == b"%PDF-") {
    return Err(ProcessingError::invalid_pdf("Not a valid PDF file"));
  }

  if is_pdf_encrypted(qpdf_bin, path)? {
    return Err(ProcessingError::encrypted_pdf(
      "PDF is password-protected or encrypted",
    ));
  }

  let pages = get_pdf_page_count(qpdf_bin, path)?;
  if pages == 0 {
    return Err(ProcessingError::invalid_pdf("No pages found in PDF"));
  }
  if pages > MAX_PDF_PAGES {
    return Err(ProcessingError::limit_exceeded(format!(
      "PDF exceeds maximum limit of {} pages (found {})",
      MAX_PDF_PAGES, pages
    )));
  }

  if has_signature_field(qpdf_bin, path)? {
    return Err(ProcessingError::invalid_pdf(
      "PDF contains a digital signature and cannot be optimized",
    ));
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicU32, Ordering};

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-pdf-inspect-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn valid_pdf_header_is_ready() {
    let dir = temp_dir();
    let file_path = dir.join("doc.pdf");
    std::fs::write(&file_path, b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n").unwrap();
    let res = inspect_pdf_paths(vec![file_path.to_string_lossy().into_owned()]);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "ready");
    assert_eq!(res[0].extension, "pdf");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn zero_byte_pdf_is_invalid() {
    let dir = temp_dir();
    let file_path = dir.join("empty.pdf");
    std::fs::write(&file_path, b"").unwrap();
    let res = inspect_pdf_paths(vec![file_path.to_string_lossy().into_owned()]);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "invalid");
    assert_eq!(res[0].error.as_deref(), Some("File is empty"));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn invalid_magic_pdf_is_invalid() {
    let dir = temp_dir();
    let file_path = dir.join("fake.pdf");
    std::fs::write(&file_path, b"NOT A PDF HEADER").unwrap();
    let res = inspect_pdf_paths(vec![file_path.to_string_lossy().into_owned()]);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "invalid");
    assert_eq!(res[0].error.as_deref(), Some("Not a valid PDF file"));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn incomplete_pdf_magic_is_invalid() {
    let dir = temp_dir();
    let file_path = dir.join("fake.pdf");
    std::fs::write(&file_path, b"prefix %PDFx is not a PDF header").unwrap();
    let res = inspect_pdf_paths(vec![file_path.to_string_lossy().into_owned()]);
    assert_eq!(res[0].status, "invalid");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn missing_pdf_is_invalid() {
    let dir = temp_dir();
    let file_path = dir.join("missing.pdf");
    let res = inspect_pdf_paths(vec![file_path.to_string_lossy().into_owned()]);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].status, "invalid");
    assert_eq!(res[0].error.as_deref(), Some("File not found"));
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
