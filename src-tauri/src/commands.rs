use std::path::PathBuf;

use tauri::Emitter;

use crate::errors::ProcessingErrorDto;
use crate::models::{
  BatchResult, CompressImagesRequest, ConvertImagesRequest, ConvertPdfsRequest,
  ExportColorPaletteRequest, ExportColorPaletteResult, ExtractColorPaletteRequest,
  ExtractColorPaletteResult, GenerateLogoPackRequest, GenerateLogoPackResult, InputFile,
  LogoAssetDefinition, OptimizePdfBatchResult, OptimizePdfsRequest, ProcessVideosRequest,
  ProcessingProgress, VideoBatchResult,
};
use crate::services;
use crate::tools;

fn emit_processing_progress(
  app: &tauri::AppHandle,
  job_id: &str,
  completed: u32,
  total: u32,
  filename: &str,
) {
  let _ = app.emit(
    services::batch::EVENT_PROGRESS,
    ProcessingProgress {
      job_id: job_id.to_string(),
      completed,
      total,
      current_file: Some(filename.to_string()),
    },
  );
}

#[tauri::command]
pub async fn extract_color_palette(
  request: ExtractColorPaletteRequest,
) -> Result<ExtractColorPaletteResult, ProcessingErrorDto> {
  let source = PathBuf::from(&request.source_path);
  let target = request.target_count;
  tauri::async_runtime::spawn_blocking(move || tools::image::palette::extract(&source, target))
    .await
    .map_err(|error| {
      ProcessingErrorDto::processing_failed(format!("Extraction task failed: {error}"))
    })?
    .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub async fn export_color_palette(
  request: ExportColorPaletteRequest,
) -> Result<ExportColorPaletteResult, ProcessingErrorDto> {
  let source = PathBuf::from(&request.source_path);
  let output_dir = PathBuf::from(&request.output_directory);
  let colors = request.colors;
  let format = request.format;
  tauri::async_runtime::spawn_blocking(move || {
    tools::image::palette::export(&source, &colors, &output_dir, format)
  })
  .await
  .map_err(|error| ProcessingErrorDto::processing_failed(format!("Export task failed: {error}")))?
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub fn inspect_files(paths: Vec<String>) -> Vec<InputFile> {
  services::inspect::inspect_paths(paths)
}

#[tauri::command]
pub async fn convert_images(
  app: tauri::AppHandle,
  request: ConvertImagesRequest,
) -> Result<BatchResult, ProcessingErrorDto> {
  let output_dir = PathBuf::from(&request.output_directory);
  services::export::ensure_output_dir(&output_dir)?;

  let job_id = request.job_id.clone();
  let files: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();
  let format = request.output_format;
  let quality = request.quality;
  let png_level = request.png_level;
  let resize = request.resize;

  tauri::async_runtime::spawn_blocking(move || {
    services::batch::run_batch(
      &files,
      |source| {
        tools::image::convert::convert_file(
          source,
          &output_dir,
          format,
          quality,
          png_level,
          &resize,
        )
      },
      move |completed, total, filename| {
        emit_processing_progress(&app, &job_id, completed, total, filename)
      },
    )
  })
  .await
  .map_err(|error| {
    ProcessingErrorDto::processing_failed(format!("Processing task failed: {error}"))
  })?
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub async fn compress_images(
  app: tauri::AppHandle,
  request: CompressImagesRequest,
) -> Result<BatchResult, ProcessingErrorDto> {
  let output_dir = PathBuf::from(&request.output_directory);
  services::export::ensure_output_dir(&output_dir)?;

  let job_id = request.job_id.clone();
  let files: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();
  let quality = request.quality;
  let png_level = request.png_level;
  let resize = request.resize;

  tauri::async_runtime::spawn_blocking(move || {
    services::batch::run_batch(
      &files,
      |source| {
        tools::image::compress::compress_file(source, &output_dir, quality, png_level, &resize)
      },
      move |completed, total, filename| {
        emit_processing_progress(&app, &job_id, completed, total, filename)
      },
    )
  })
  .await
  .map_err(|error| {
    ProcessingErrorDto::processing_failed(format!("Processing task failed: {error}"))
  })?
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub async fn generate_logo_pack(
  app: tauri::AppHandle,
  request: GenerateLogoPackRequest,
) -> Result<GenerateLogoPackResult, ProcessingErrorDto> {
  let source = PathBuf::from(&request.source_path);
  let output_dir = PathBuf::from(&request.output_directory);
  let job_id = request.job_id.clone();
  let asset_ids = request.asset_ids;

  tauri::async_runtime::spawn_blocking(move || {
    tools::image::logo_pack::generate(
      &source,
      &output_dir,
      &asset_ids,
      |completed, total, filename| {
        emit_processing_progress(&app, &job_id, completed, total, filename)
      },
    )
  })
  .await
  .map_err(|error| {
    ProcessingErrorDto::processing_failed(format!("Processing task failed: {error}"))
  })?
  .map(|(pack_dir, batch)| GenerateLogoPackResult {
    pack_directory: pack_dir.to_string_lossy().into_owned(),
    batch,
  })
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub fn inspect_pdfs(paths: Vec<String>) -> Vec<InputFile> {
  tools::pdf::inspect::inspect_pdf_paths(paths)
}

#[tauri::command]
pub async fn inspect_pdfs_for_optimization(
  paths: Vec<String>,
) -> Result<Vec<InputFile>, ProcessingErrorDto> {
  tauri::async_runtime::spawn_blocking(move || {
    tools::pdf::inspect::inspect_pdfs_for_optimization(paths)
  })
  .await
  .map_err(|e| ProcessingErrorDto::processing_failed(format!("PDF inspection failed: {e}")))
}

#[tauri::command]
pub async fn convert_pdfs(
  app: tauri::AppHandle,
  request: ConvertPdfsRequest,
) -> Result<BatchResult, ProcessingErrorDto> {
  let output_dir = PathBuf::from(&request.output_directory);
  services::export::ensure_output_dir(&output_dir)?;

  let job_id = request.job_id.clone();
  let files: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();

  tauri::async_runtime::spawn_blocking(move || {
    services::batch::run_sequential_batch(
      &files,
      |source| tools::pdf::convert::convert_pdf_file(source, &output_dir),
      move |completed, total, filename| {
        emit_processing_progress(&app, &job_id, completed, total, filename)
      },
    )
  })
  .await
  .map_err(|error| {
    ProcessingErrorDto::processing_failed(format!("Processing task failed: {error}"))
  })?
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub fn get_logo_presets() -> Vec<LogoAssetDefinition> {
  tools::image::logo_pack::STANDARD_WEB_PACK.to_vec()
}

#[tauri::command]
pub fn inspect_videos(paths: Vec<String>) -> Vec<InputFile> {
  tools::video::probe::inspect_video_paths(paths)
}

#[tauri::command]
pub async fn process_videos(
  app: tauri::AppHandle,
  request: ProcessVideosRequest,
) -> Result<VideoBatchResult, ProcessingErrorDto> {
  let output_dir = PathBuf::from(&request.output_directory);
  services::export::ensure_output_dir(&output_dir)?;

  let binaries = tools::video::probe::resolve_binaries().map_err(ProcessingErrorDto::from)?;

  let files: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();

  tauri::async_runtime::spawn_blocking(move || {
    tools::video::process::run_batch(&files, &output_dir, &request, &binaries, move |progress| {
      let _ = app.emit(tools::video::process::EVENT_PROGRESS, progress);
    })
  })
  .await
  .map_err(|error| {
    ProcessingErrorDto::processing_failed(format!("Processing task failed: {error}"))
  })?
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub fn cancel_video_job(job_id: String) {
  tools::video::process::cancel_job(&job_id);
}

#[tauri::command]
pub async fn optimize_pdfs(
  app: tauri::AppHandle,
  request: OptimizePdfsRequest,
) -> Result<OptimizePdfBatchResult, ProcessingErrorDto> {
  let output_dir = PathBuf::from(&request.output_directory);
  services::export::ensure_output_dir(&output_dir)?;

  let files: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();

  tauri::async_runtime::spawn_blocking(move || {
    tools::pdf::optimize::run_batch(&files, &output_dir, &request, move |progress| {
      let _ = app.emit(tools::pdf::optimize::EVENT_PROGRESS, progress);
    })
  })
  .await
  .map_err(|error| {
    ProcessingErrorDto::processing_failed(format!("Processing task failed: {error}"))
  })?
  .map_err(ProcessingErrorDto::from)
}

#[tauri::command]
pub fn cancel_pdf_optimization_job(job_id: String) {
  tools::pdf::optimize::cancel_job(&job_id);
}
