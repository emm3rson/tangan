use std::path::{Path, PathBuf};

use crate::errors::ProcessingError;

/// Fails with `OutputUnavailable` when `dir` does not exist or is not a
/// directory, so a missing/moved destination is reported before any work runs.
pub fn ensure_output_dir(dir: &Path) -> Result<(), ProcessingError> {
  if !dir.is_dir() {
    return Err(ProcessingError::output_unavailable(format!(
      "Export directory is not available: {}",
      dir.display()
    )));
  }
  Ok(())
}

/// Resolves the output path for a converted file.
///
/// Naming: source stem + optional suffix + new extension
/// (`photo.png` → `photo.webp`, or `photo.jpg` → `photo-compressed.jpg`).
/// Collision rule: always auto-rename incrementally (`photo-2.webp`,
/// `photo-3.webp`, ...). The source path itself always counts as a collision,
/// so a conversion can never resolve to (and thus overwrite) the source file.
pub fn resolve_output_path(
  output_dir: &Path,
  source_path: &Path,
  ext: &str,
  suffix: Option<&str>,
) -> Result<PathBuf, ProcessingError> {
  let stem = source_path
    .file_stem()
    .ok_or_else(|| {
      ProcessingError::processing_failed(format!(
        "Could not determine file name for {}",
        source_path.display()
      ))
    })?
    .to_string_lossy()
    .into_owned();

  let mut n: u32 = 0;
  loop {
    let name = if n == 0 {
      format!("{stem}{}.{ext}", suffix.unwrap_or(""))
    } else {
      format!("{stem}{}-{}.{ext}", suffix.unwrap_or(""), n + 1)
    };
    let candidate = output_dir.join(name);
    if candidate != source_path && !candidate.exists() {
      return Ok(candidate);
    }
    n += 1;
  }
}

/// Creates the output directory (and any missing parents) for generated
/// pack outputs, e.g. the Logo Pack's `web-pack` folder.
pub fn create_output_dir(dir: &Path) -> Result<(), ProcessingError> {
  std::fs::create_dir_all(dir).map_err(|error| {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
      ProcessingError::permission_denied(format!(
        "Permission denied creating directory '{}': {error}",
        dir.display()
      ))
    } else {
      ProcessingError::output_unavailable(format!(
        "Could not create directory '{}': {error}",
        dir.display()
      ))
    }
  })
}

/// Resolves a fresh directory path for generated pack outputs.
///
/// Naming follows the same collision rule as files: the first free candidate
/// of `web-pack`, `web-pack-2`, `web-pack-3`, ... is returned. Nothing is
/// created here; callers create the resolved directory.
pub fn resolve_output_dir(base: &Path) -> Result<PathBuf, ProcessingError> {
  let name = base.file_name().ok_or_else(|| {
    ProcessingError::processing_failed(format!(
      "Could not determine folder name for {}",
      base.display()
    ))
  })?;

  let mut n: u32 = 0;
  loop {
    let candidate = if n == 0 {
      base.to_path_buf()
    } else {
      base.with_file_name(format!("{}-{}", name.to_string_lossy(), n + 1))
    };
    if !candidate.exists() {
      return Ok(candidate);
    }
    n += 1;
  }
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};
  use std::sync::atomic::{AtomicU32, Ordering};

  use crate::errors::ProcessingError;

  use super::{create_output_dir, ensure_output_dir, resolve_output_dir, resolve_output_path};

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-export-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn ensure_output_dir_rejects_missing_path() {
    let dir = temp_dir();
    let missing = dir.join("does-not-exist");
    let err = ensure_output_dir(&missing).unwrap_err();
    assert!(matches!(err, ProcessingError::OutputUnavailable { .. }));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn ensure_output_dir_rejects_regular_file() {
    let dir = temp_dir();
    let file = dir.join("photo.png");
    std::fs::write(&file, b"x").unwrap();
    let err = ensure_output_dir(&file).unwrap_err();
    assert!(matches!(err, ProcessingError::OutputUnavailable { .. }));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn same_dir_same_extension_never_overwrites_source() {
    let dir = temp_dir();
    let source = dir.join("photo.png");
    std::fs::write(&source, b"original").unwrap();
    let out = resolve_output_path(&dir, &source, "png", None).unwrap();
    assert_ne!(out, source);
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "photo-2.png");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "original");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn fresh_output_uses_plain_name() {
    let dir = temp_dir();
    let source = Path::new("C:/photos/photo.png");
    let out = resolve_output_path(&dir, source, "webp", None).unwrap();
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "photo.webp");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn collision_renames_incrementally() {
    let dir = temp_dir();
    std::fs::write(dir.join("photo.webp"), b"x").unwrap();
    std::fs::write(dir.join("photo-2.webp"), b"x").unwrap();
    let source = Path::new("C:/photos/photo.png");
    let out = resolve_output_path(&dir, source, "webp", None).unwrap();
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "photo-3.webp");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn source_path_is_never_overwritten() {
    let dir = temp_dir();
    std::fs::write(dir.join("photo.png"), b"original").unwrap();
    let source = dir.join("photo.png");
    let out = resolve_output_path(&dir, &source, "png", None).unwrap();
    assert_ne!(out, source);
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "photo-2.png");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "original");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn jpeg_uses_jpg_extension() {
    let dir = temp_dir();
    let source = Path::new("C:/photos/photo.jpeg");
    let out = resolve_output_path(&dir, source, "jpg", None).unwrap();
    assert_eq!(out.file_name().unwrap().to_string_lossy(), "photo.jpg");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn suffix_names_output_and_renames_on_collision() {
    let dir = temp_dir();
    let source = Path::new("C:/photos/photo.jpg");
    let out = resolve_output_path(&dir, source, "jpg", Some("-compressed")).unwrap();
    assert_eq!(
      out.file_name().unwrap().to_string_lossy(),
      "photo-compressed.jpg"
    );
    std::fs::write(&out, b"x").unwrap();
    let second = resolve_output_path(&dir, source, "jpg", Some("-compressed")).unwrap();
    assert_eq!(
      second.file_name().unwrap().to_string_lossy(),
      "photo-compressed-2.jpg"
    );
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn create_output_dir_creates_missing_nested_path() {
    let dir = temp_dir();
    let nested = dir.join("a").join("b").join("web-pack");
    create_output_dir(&nested).unwrap();
    assert!(nested.is_dir());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn resolve_output_dir_prefers_plain_then_numbered() {
    let dir = temp_dir();
    let base = dir.join("web-pack");

    let first = resolve_output_dir(&base).unwrap();
    assert_eq!(first.file_name().unwrap().to_string_lossy(), "web-pack");
    std::fs::create_dir_all(&first).unwrap();

    let second = resolve_output_dir(&base).unwrap();
    assert_eq!(second.file_name().unwrap().to_string_lossy(), "web-pack-2");
    std::fs::create_dir_all(&second).unwrap();

    let third = resolve_output_dir(&base).unwrap();
    assert_eq!(third.file_name().unwrap().to_string_lossy(), "web-pack-3");
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
