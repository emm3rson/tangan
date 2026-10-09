use tauri::Manager;

mod commands;
mod errors;
mod models;
mod services;
mod tools;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_store::Builder::new().build())
    .setup(|app| {
      if let Ok(resource_dir) = app.path().resource_dir() {
        let bcmaps_dir = resource_dir.join("resources").join("bcmaps");
        if bcmaps_dir.exists() {
          std::env::set_var("PDF_INSPECTOR_BCMAPS_DIR", &bcmaps_dir);
        }
        let ffmpeg_dir = resource_dir.join("resources").join("ffmpeg");
        if ffmpeg_dir.join("ffmpeg.exe").exists() {
          std::env::set_var("TOOLBOX_FFMPEG_DIR", &ffmpeg_dir);
        }
        let qpdf_dir = resource_dir.join("resources").join("qpdf");
        if qpdf_dir.join("qpdf.exe").exists() {
          std::env::set_var("TOOLBOX_QPDF_DIR", &qpdf_dir);
        }
      }
      tools::pdf::convert::ensure_bcmaps_configured();
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      commands::inspect_files,
      commands::inspect_pdfs,
      commands::inspect_pdfs_for_optimization,
      commands::inspect_videos,
      commands::convert_images,
      commands::compress_images,
      commands::convert_pdfs,
      commands::generate_logo_pack,
      commands::get_logo_presets,
      commands::extract_color_palette,
      commands::export_color_palette,
      commands::process_videos,
      commands::cancel_video_job,
      commands::optimize_pdfs,
      commands::cancel_pdf_optimization_job
    ])
    .run(tauri::generate_context!())
    .expect("error while running Tangan");
}
