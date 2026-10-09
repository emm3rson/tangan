use std::path::PathBuf;

use crate::errors::ProcessingError;

/// Resolves the bundled qpdf executable.
///
/// Order: the `TOOLBOX_QPDF_DIR` environment variable (set at app setup for
/// the installed resource directory), then the dev staging directory under the
/// crate's `resources/qpdf`. Returns `PDF_ENGINE_UNAVAILABLE` when neither
/// location contains the executable, without falling back to an arbitrary
/// PATH executable.
pub fn resolve_qpdf_binary() -> Result<PathBuf, ProcessingError> {
  let mut candidates: Vec<PathBuf> = Vec::new();
  if let Ok(dir) = std::env::var("TOOLBOX_QPDF_DIR") {
    candidates.push(PathBuf::from(dir).join("qpdf.exe"));
  }
  candidates.push(
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("resources")
      .join("qpdf")
      .join("qpdf.exe"),
  );

  resolve_from_candidates(candidates)
}

fn resolve_from_candidates(
  candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<PathBuf, ProcessingError> {
  candidates
    .into_iter()
    .find(|candidate| candidate.is_file())
    .ok_or_else(|| ProcessingError::pdf_engine_unavailable("PDF engine is not available"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn returns_unavailable_when_no_candidate_exists() {
    let result = resolve_from_candidates([PathBuf::from("C:/definitely/not/qpdf.exe")]);
    assert_eq!(
      result.unwrap_err().into_dto().code,
      "PDF_ENGINE_UNAVAILABLE"
    );
  }

  #[test]
  fn resolves_first_existing_candidate() {
    let dir = std::env::temp_dir().join(format!(
      "tangan-qpdf-resolve-test-{}-{}",
      std::process::id(),
      99999
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("qpdf.exe");
    std::fs::write(&fake, b"fake").unwrap();

    let resolved =
      resolve_from_candidates([dir.join("missing.exe"), fake.clone(), dir.join("later.exe")])
        .expect("should resolve an existing candidate");
    assert_eq!(resolved, fake);
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
