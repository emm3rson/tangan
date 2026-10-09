use std::io::BufReader;

use crate::models::InputFile;

const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "svg"];

/// Inspects each path and returns one `InputFile` per image.
///
/// A directory path is expanded shallowly: its direct children that look like
/// supported images are inspected (no recursion). A file path yields itself.
pub fn inspect_paths(paths: Vec<String>) -> Vec<InputFile> {
  let mut out = Vec::new();
  for path in paths {
    match std::fs::metadata(&path) {
      Ok(meta) if meta.is_dir() => out.extend(expand_directory(&path)),
      Ok(meta) if meta.is_file() => out.push(inspect_file(&path, meta.len())),
      Ok(_) => out.push(InputFile::invalid(&path, 0, "Not a regular file")),
      Err(_) => out.push(InputFile::invalid(&path, 0, "File not found")),
    }
  }
  out
}

fn expand_directory(path: &str) -> Vec<InputFile> {
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
      Some(inspect_file(&file_path.to_string_lossy(), size))
    })
    .collect()
}

fn inspect_file(path: &str, size: u64) -> InputFile {
  let mut file = InputFile::new(path, size);

  if file.extension == "svg" {
    return inspect_svg(file, path);
  }

  let mut reader = match image::ImageReader::open(path) {
    Ok(reader) => reader,
    Err(_) => {
      file.status = "invalid".into();
      file.error = Some("Could not read file".into());
      return file;
    }
  };
  reader = match reader.with_guessed_format() {
    Ok(reader) => reader,
    Err(_) => {
      file.status = "invalid".into();
      file.error = Some("Unsupported format".into());
      return file;
    }
  };
  let format = reader.format();

  match format {
    Some(image::ImageFormat::Png) | Some(image::ImageFormat::Jpeg) => {
      match reader.into_dimensions() {
        Ok((width, height)) => {
          file.width = width;
          file.height = height;
          file.status = "ready".into();
        }
        Err(_) => {
          file.status = "invalid".into();
          file.error = Some("Not a valid image".into());
        }
      }
    }
    Some(image::ImageFormat::WebP) => inspect_webp(&mut file, path),
    _ => {
      file.status = "invalid".into();
      file.error = Some("Unsupported format".into());
    }
  }

  file
}

fn inspect_webp(file: &mut InputFile, path: &str) {
  let Ok(handle) = std::fs::File::open(path) else {
    file.status = "invalid".into();
    file.error = Some("Could not read file".into());
    return;
  };
  match image_webp::WebPDecoder::new(BufReader::new(handle)) {
    Ok(decoder) => {
      let (width, height) = decoder.dimensions();
      file.width = width;
      file.height = height;
      file.status = "ready".into();
    }
    Err(_) => {
      file.status = "invalid".into();
      file.error = Some("Not a valid image".into());
    }
  }
}

fn inspect_svg(mut file: InputFile, path: &str) -> InputFile {
  let bytes = match std::fs::read(path) {
    Ok(bytes) => bytes,
    Err(_) => {
      file.status = "invalid".into();
      file.error = Some("Could not read file".into());
      return file;
    }
  };

  match crate::tools::image::svg::parse(&bytes) {
    Ok(tree) => {
      let (w, h) = crate::tools::image::svg::intrinsic_size(&tree);
      file.width = w;
      file.height = h;
      file.status = "ready".into();
    }
    Err(_) => {
      file.status = "invalid".into();
      file.error = Some("Not a valid SVG".into());
    }
  }

  file
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicU32, Ordering};

  use super::inspect_paths;

  static COUNTER: AtomicU32 = AtomicU32::new(0);

  fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "tangan-inspect-test-{}-{}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  fn write_fixture_png(path: &std::path::Path, width: u32, height: u32) {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
      image::Rgba([(x % 251) as u8, (y % 251) as u8, 120, 255])
    }));
    img.save(path).unwrap();
  }

  #[test]
  fn valid_png_is_ready_with_dimensions() {
    let dir = temp_dir();
    let source = dir.join("ok.png");
    write_fixture_png(&source, 40, 30);
    let file = inspect_paths(vec![source.to_string_lossy().into_owned()]).remove(0);
    assert_eq!(file.status, "ready");
    assert_eq!(file.width, 40);
    assert_eq!(file.height, 30);
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn corrupt_png_is_invalid_with_error() {
    let dir = temp_dir();
    let source = dir.join("broken.png");
    std::fs::write(&source, b"not an image").unwrap();
    let file = inspect_paths(vec![source.to_string_lossy().into_owned()]).remove(0);
    assert_eq!(file.status, "invalid");
    assert!(file.error.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn zero_byte_file_is_invalid_with_error() {
    let dir = temp_dir();
    let source = dir.join("empty.png");
    std::fs::write(&source, []).unwrap();
    let file = inspect_paths(vec![source.to_string_lossy().into_owned()]).remove(0);
    assert_eq!(file.status, "invalid");
    assert!(file.error.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn missing_path_is_invalid() {
    let dir = temp_dir();
    let source = dir.join("missing.png");
    let file = inspect_paths(vec![source.to_string_lossy().into_owned()]).remove(0);
    assert_eq!(file.status, "invalid");
    assert_eq!(file.error.as_deref(), Some("File not found"));
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn valid_svg_is_ready_with_dimensions() {
    let dir = temp_dir();
    let source = dir.join("vector.svg");
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 80">
      <rect width="120" height="80" fill="#ff6600" />
    </svg>"##;
    std::fs::write(&source, svg).unwrap();

    let file = inspect_paths(vec![source.to_string_lossy().into_owned()]).remove(0);
    assert_eq!(file.status, "ready");
    assert_eq!(file.width, 120);
    assert_eq!(file.height, 80);
    assert_eq!(file.extension, "svg");
    std::fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn corrupt_svg_is_invalid_with_error() {
    let dir = temp_dir();
    let source = dir.join("bad.svg");
    std::fs::write(&source, b"<svg><unclosed>").unwrap();

    let file = inspect_paths(vec![source.to_string_lossy().into_owned()]).remove(0);
    assert_eq!(file.status, "invalid");
    assert!(file.error.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
  }
}
