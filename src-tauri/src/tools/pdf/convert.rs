use std::io::Write;
use std::path::Path;

use crate::errors::ProcessingError;
use crate::models::{FileResult, FileWarning};
use crate::services::export;

use super::inspect::{MAX_PDF_BYTES, MAX_PDF_PAGES};

/// Configure bundled CMap directory if not already set.
pub fn ensure_bcmaps_configured() {
  if std::env::var("PDF_INSPECTOR_BCMAPS_DIR").is_err() {
    let manifest_bcmaps = Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("resources")
      .join("bcmaps");
    if manifest_bcmaps.exists() {
      std::env::set_var("PDF_INSPECTOR_BCMAPS_DIR", manifest_bcmaps);
    }
  }
}

pub struct ConvertedPdf {
  pub markdown: String,
  pub ocr_pages: Vec<u32>,
}

/// Extracts Markdown from a single PDF path.
pub fn extract_pdf_markdown(source_path: &Path) -> Result<ConvertedPdf, ProcessingError> {
  ensure_bcmaps_configured();

  if !source_path.exists() {
    return Err(ProcessingError::file_not_found("File not found"));
  }

  let meta = std::fs::metadata(source_path)
    .map_err(|_| ProcessingError::permission_denied("Could not read file metadata"))?;

  if !meta.is_file() {
    return Err(ProcessingError::invalid_pdf("PDF path is not a file"));
  }

  let is_pdf_extension = source_path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
  if !is_pdf_extension {
    return Err(ProcessingError::unsupported_format(
      "Unsupported format (expected .pdf)",
    ));
  }

  if meta.len() == 0 {
    return Err(ProcessingError::invalid_pdf("PDF file is empty"));
  }

  if meta.len() > MAX_PDF_BYTES {
    return Err(ProcessingError::limit_exceeded(
      "PDF exceeds maximum size limit (100 MB)",
    ));
  }

  // Detect first so the page guard runs before full text/layout extraction.
  let detection = pdf_inspector::detect_pdf(source_path).map_err(map_parser_error)?;
  if detection.page_count == 0 {
    return Err(ProcessingError::invalid_pdf("No pages found in PDF"));
  }
  if detection.page_count > MAX_PDF_PAGES {
    return Err(ProcessingError::limit_exceeded(format!(
      "PDF exceeds maximum limit of {} pages (found {})",
      MAX_PDF_PAGES, detection.page_count
    )));
  }

  let extraction =
    pdf_inspector::extract_pages_markdown(source_path, None).map_err(map_parser_error)?;

  let pages = extraction.pages;
  let total_pages = pages.len();
  if total_pages != detection.page_count as usize {
    return Err(ProcessingError::invalid_pdf(
      "PDF page extraction returned an incomplete result",
    ));
  }

  let mut ocr_pages = Vec::new();
  let mut page_sections = Vec::new();

  for (idx, page) in pages.into_iter().enumerate() {
    if page.page != idx as u32 {
      return Err(ProcessingError::invalid_pdf(
        "PDF page extraction returned pages out of order",
      ));
    }
    let page_num = page.page + 1;
    if page.needs_ocr {
      ocr_pages.push(page_num);
      let reason = normalized_ocr_reason(page.ocr_reason.as_deref());
      let marker = format!(
        "<!-- PDF page {} requires OCR: {} -->\n\n> Page {} was not converted because OCR is not included in Tangan yet.",
        page_num, reason, page_num
      );
      page_sections.push(marker);
    } else {
      let content = page.markdown.trim();
      if content.is_empty() {
        ocr_pages.push(page_num);
        let marker = format!(
          "<!-- PDF page {} requires OCR: empty_content -->\n\n> Page {} was not converted because OCR is not included in Tangan yet.",
          page_num, page_num
        );
        page_sections.push(marker);
      } else {
        page_sections.push(content.to_string());
      }
    }
  }

  // If ALL pages require OCR, fail without writing an output
  if ocr_pages.len() == total_pages {
    return Err(ProcessingError::ocr_required(
      "All pages in the document require OCR to extract text",
    ));
  }

  let full_markdown = page_sections.join("\n\n");

  Ok(ConvertedPdf {
    markdown: full_markdown,
    ocr_pages,
  })
}

/// Converts one PDF file and writes the Markdown output into `output_dir`.
/// Follows non-destructive collision policy (.md, -2.md, ...).
pub fn convert_pdf_file(source: &Path, output_dir: &Path) -> FileResult {
  let source_string = source.to_string_lossy().into_owned();
  let original_size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);

  match convert_pdf_file_inner(source, output_dir) {
    Ok((output_path, output_size, warnings)) => FileResult {
      source_path: source_string,
      output_path: Some(output_path),
      success: true,
      original_size,
      output_size: Some(output_size),
      error: None,
      warnings,
    },
    Err(error) => FileResult {
      source_path: source_string,
      output_path: None,
      success: false,
      original_size,
      output_size: None,
      error: Some(error.into_dto()),
      warnings: None,
    },
  }
}

fn convert_pdf_file_inner(
  source: &Path,
  output_dir: &Path,
) -> Result<(String, u64, Option<Vec<FileWarning>>), ProcessingError> {
  export::ensure_output_dir(output_dir)?;

  let converted = extract_pdf_markdown(source)?;
  let bytes = converted.markdown.as_bytes();

  let output_path = export::resolve_output_path(output_dir, source, "md", None)?;

  write_new_output(&output_path, bytes)?;

  let warnings = if !converted.ocr_pages.is_empty() {
    let pages_str = converted
      .ocr_pages
      .iter()
      .map(|p| p.to_string())
      .collect::<Vec<_>>()
      .join(", ");
    Some(vec![FileWarning {
      code: "OCR_REQUIRED_PAGES".into(),
      message: format!("Pages {} require OCR and were marked", pages_str),
      pages: Some(converted.ocr_pages),
    }])
  } else {
    None
  };

  Ok((
    output_path.to_string_lossy().into_owned(),
    bytes.len() as u64,
    warnings,
  ))
}

fn write_new_output(output_path: &Path, bytes: &[u8]) -> Result<(), ProcessingError> {
  let mut file = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(output_path)
    .map_err(|error| map_write_error(output_path, error))?;

  if let Err(error) = file.write_all(bytes) {
    drop(file);
    let _ = std::fs::remove_file(output_path);
    return Err(map_write_error(output_path, error));
  }

  Ok(())
}

fn map_write_error(output_path: &Path, error: std::io::Error) -> ProcessingError {
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
}

fn map_parser_error(error: pdf_inspector::PdfError) -> ProcessingError {
  match error {
    pdf_inspector::PdfError::Encrypted => {
      ProcessingError::encrypted_pdf("PDF is password-protected or encrypted")
    }
    pdf_inspector::PdfError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
      ProcessingError::file_not_found("File not found")
    }
    pdf_inspector::PdfError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
      ProcessingError::permission_denied("Could not read PDF file")
    }
    pdf_inspector::PdfError::Io(_) => ProcessingError::invalid_pdf("This PDF could not be read"),
    pdf_inspector::PdfError::Parse(_)
    | pdf_inspector::PdfError::InvalidStructure
    | pdf_inspector::PdfError::NotAPdf(_) => {
      ProcessingError::invalid_pdf("This PDF could not be opened")
    }
  }
}

fn normalized_ocr_reason(reason: Option<&str>) -> &'static str {
  match reason.unwrap_or_default().to_ascii_lowercase().as_str() {
    value if value.contains("scanned") => "scanned",
    value if value.contains("image") => "image_only",
    value if value.contains("vector") => "vector_text",
    value if value.contains("garbled") || value.contains("encoding") => "unreadable_text",
    value if value.contains("no_text") || value.contains("no text") => "no_text",
    value if value.contains("low") || value.contains("quality") => "low_text_density",
    _ => "text_unavailable",
  }
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};
  use std::sync::atomic::{AtomicU32, Ordering};

  use super::*;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-pdf-convert-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_minimal_text_pdf(path: &Path, text: &str) {
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

  fn write_multipage_pdf(path: &Path, pages_content: &[Option<&str>]) {
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

    let mut page_ids = Vec::new();
    for opt_text in pages_content {
      let content_id = if let Some(text) = opt_text {
        let content = Content {
          operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.into()]),
            Operation::new("Td", vec![72.into(), 712.into()]),
            Operation::new("Tj", vec![Object::string_literal(*text)]),
            Operation::new("ET", vec![]),
          ],
        };
        doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()))
      } else {
        doc.add_object(Stream::new(dictionary! {}, vec![]))
      };

      let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
      });
      page_ids.push(page_id.into());
    }

    let pages = dictionary! {
      "Type" => "Pages",
      "Kids" => page_ids,
      "Count" => pages_content.len() as i64,
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
  fn converts_text_pdf_to_markdown_file() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("document.pdf");
    write_minimal_text_pdf(&source, "Hello Tangan PDF");

    let result = convert_pdf_file(&source, &out);
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let output_path = result.output_path.expect("output path should exist");
    assert!(output_path.ends_with("document.md"));
    assert!(Path::new(&output_path).exists());

    let content = std::fs::read_to_string(&output_path).unwrap();
    assert!(content.contains("Hello Tangan PDF"));
    assert!(result.warnings.is_none());

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn mixed_pdf_creates_partial_markdown_with_warning() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("mixed.pdf");
    // Page 1 has text, Page 2 is empty/scanned
    write_multipage_pdf(&source, &[Some("First page native text"), None]);

    let result = convert_pdf_file(&source, &out);
    assert!(result.success, "unexpected failure: {:?}", result.error);
    let output_path = result.output_path.expect("output path should exist");
    let content = std::fs::read_to_string(&output_path).unwrap();

    assert!(content.contains("First page native text"));
    assert!(content.contains("<!-- PDF page 2 requires OCR:"));
    assert!(
      content.contains("> Page 2 was not converted because OCR is not included in Tangan yet.")
    );

    let warnings = result.warnings.expect("should have warning for page 2");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "OCR_REQUIRED_PAGES");
    assert_eq!(warnings[0].pages, Some(vec![2]));

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn scanned_pdf_fails_with_ocr_required_and_no_output() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("scanned.pdf");
    // All pages empty / no text
    write_multipage_pdf(&source, &[None, None]);

    let result = convert_pdf_file(&source, &out);
    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert_eq!(result.error.unwrap().code, "OCR_REQUIRED");

    // Ensure no output file was created
    assert!(!out.join("scanned.md").exists());

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn batch_conversion_isolates_failures_and_reports_progress() {
    let dir = temp_dir();
    let out = temp_dir();
    let good = dir.join("good.pdf");
    write_minimal_text_pdf(&good, "Good PDF");
    let corrupt = dir.join("corrupt.pdf");
    std::fs::write(&corrupt, b"%PDF-1.4\ncorrupt").unwrap();
    let scanned = dir.join("scanned.pdf");
    write_multipage_pdf(&scanned, &[None]);

    let files = vec![good.clone(), corrupt.clone(), scanned.clone()];
    let progress_records = std::sync::Mutex::new(Vec::new());

    let batch = crate::services::batch::run_sequential_batch(
      &files,
      |source| convert_pdf_file(source, &out),
      |done, total, filename| {
        progress_records
          .lock()
          .unwrap()
          .push((done, total, filename.to_string()));
      },
    )
    .unwrap();

    assert_eq!(batch.total, 3);
    assert_eq!(batch.succeeded, 1);
    assert_eq!(batch.failed, 2);

    let progress = progress_records.into_inner().unwrap();
    assert_eq!(progress.len(), 3);
    assert_eq!(progress[0].0, 1);
    assert_eq!(progress[1].0, 2);
    assert_eq!(progress[2].0, 3);

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn collision_renames_incrementally() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("report.pdf");
    write_minimal_text_pdf(&source, "Report text");

    let first = convert_pdf_file(&source, &out);
    assert!(first.success);
    assert!(first.output_path.unwrap().ends_with("report.md"));

    let second = convert_pdf_file(&source, &out);
    assert!(second.success);
    assert!(second.output_path.unwrap().ends_with("report-2.md"));

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn zero_byte_pdf_fails_without_creating_output() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("empty.pdf");
    std::fs::write(&source, b"").unwrap();

    let result = convert_pdf_file(&source, &out);
    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert_eq!(result.error.unwrap().code, "INVALID_PDF");

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn corrupt_pdf_fails_without_creating_output() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("corrupt.pdf");
    std::fs::write(&source, b"%PDF-1.4\ncorrupted content\n%%EOF").unwrap();

    let result = convert_pdf_file(&source, &out);
    assert!(!result.success);
    assert!(result.output_path.is_none());
    assert!(result.error.is_some());

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn missing_pdf_fails_cleanly() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("nonexistent.pdf");

    let result = convert_pdf_file(&source, &out);
    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "FILE_NOT_FOUND");

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn valid_pdf_with_wrong_extension_is_rejected() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("document.txt");
    write_minimal_text_pdf(&source, "Valid PDF bytes");

    let result = convert_pdf_file(&source, &out);
    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "UNSUPPORTED_FORMAT");
    assert!(result.output_path.is_none());

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn over_page_limit_is_rejected_before_output() {
    let dir = temp_dir();
    let out = temp_dir();
    let source = dir.join("too-many-pages.pdf");
    let pages = vec![None; MAX_PDF_PAGES as usize + 1];
    write_multipage_pdf(&source, &pages);

    let result = convert_pdf_file(&source, &out);
    assert!(!result.success);
    assert_eq!(result.error.unwrap().code, "LIMIT_EXCEEDED");
    assert!(!out.join("too-many-pages.md").exists());

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&out).unwrap();
  }

  #[test]
  fn parser_errors_use_stable_user_messages() {
    let encrypted = map_parser_error(pdf_inspector::PdfError::Encrypted).into_dto();
    assert_eq!(encrypted.code, "ENCRYPTED_PDF");

    let corrupt = map_parser_error(pdf_inspector::PdfError::Parse(
      "sensitive low-level parser diagnostic".into(),
    ))
    .into_dto();
    assert_eq!(corrupt.code, "INVALID_PDF");
    assert!(!corrupt.message.contains("sensitive"));
  }

  #[test]
  fn ocr_reason_mapping_does_not_mislabel_unknown_pages_as_scanned() {
    assert_eq!(normalized_ocr_reason(Some("vector_text")), "vector_text");
    assert_eq!(
      normalized_ocr_reason(Some("suspected_garbled_text")),
      "unreadable_text"
    );
    assert_eq!(normalized_ocr_reason(None), "text_unavailable");
  }
}
