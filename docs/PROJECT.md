# Toolbox project

Toolbox is a local-first Windows desktop utility app. Users choose a tool, add
files, adjust settings, and export. Processing stays on-device; outputs never
overwrite source files, and filename collisions receive numbered suffixes.

## Tools

| Tool | Current behavior |
|---|---|
| Image Converter | PNG/JPG/WebP/SVG input to PNG/JPG/WebP, with optional aspect-preserving resize and format-aware encoding controls. |
| Image Compressor | Same-format PNG/JPG/WebP reduction. PNG uses lossless optimization; JPG/WebP use quality settings. |
| Web Logo Pack | Square raster images or scalable SVGs to selected favicon and web icon assets, with a copyable HTML snippet and a fresh numbered output folder. |
| PDF to Markdown | Offline extraction through `pdf-inspector`. Mixed documents include OCR markers; fully scanned documents report `OCR_REQUIRED`. No OCR engine is included. |
| Video Processor | MP4/MOV/MKV/WebM/AVI to MP4 or WebM through bundled FFmpeg. Resolution caps avoid upscaling. The first video/audio tracks are retained; omitted streams produce warnings. |
| Color Palette Extractor | Deterministic 3 to 12 color extraction from PNG/JPG/WebP/SVG, source-mapped editing, undo/redo/reset, value copying, and PNG/JPG/SVG palette-sheet export. |
| PDF Optimizer | Bundled qpdf with Lossless/Balanced/Smaller File presets. Rejects encrypted and signed PDFs, verifies output before commit, and keeps the original when optimization would not reduce its size. |

Shared intake supports drag-drop, file pickers, and shallow folder inspection.
Batch tools isolate per-file failures and report progress. Video processing and
PDF optimization support whole-batch cancellation, retain completed outputs,
and discard partial files. Image batches use at most four workers; PDFs and
videos process sequentially to limit memory.

## Architecture and boundaries

- React components own interaction state. Native processing goes through
  `src/services/tauri/`; `contracts.ts` and `src-tauri/src/models.rs` define the
  IPC payloads. `realAdapter.ts` owns dialogs, command calls, and progress
  subscription lifetimes.
- Each utility exports a `ToolDefinition` registered in `src/tools/registry.ts`.
  Shared workspace/completion components live in `src/components/`; tool intake
  and error helpers live in `src/tools/shared/`.
- Rust commands delegate to `src-tauri/src/services/` and `tools/`. Shared file
  metadata lives in `models.rs`; batch aggregation and export handling live in
  services. Processors retain format-specific validation and encoding.
- Preferences use plugin-store `settings.json`, with a localStorage startup
  cache. Native settings load before persistence begins. Theme, export folder,
  and selected tool preferences persist; queues and editing history are local
  to the current tool session.
- Fonts are self-hosted. There are no accounts, cloud processing, telemetry,
  database, or automatic updates. Rust revalidates frontend input. Tauri
  capabilities and CSP live in `src-tauri/capabilities/default.json` and
  `src-tauri/tauri.conf.json`.

Image dimension and pixel limits are defined in `tools/image/mod.rs`; PDF size
and page limits live in `tools/pdf/inspect.rs`. Keep these guards and
non-destructive export behavior intact when adding or changing processors.

## Bundled engines

FFmpeg and qpdf are staged by `src-tauri/scripts/prepare-*.ps1` from hash-pinned
archives. Runtime files are gitignored, shipped through Tauri's resource rule,
and resolved from installed resources or the development staging directory.
The app does not download engines at runtime. PDF character maps and their
license are checked-in resources required for offline extraction.

Engine versions, hashes, and runtime layouts belong in the staging scripts.
FFmpeg redistribution requires its bundled license and matching source/build
provenance; qpdf ships its license and NOTICE. Review these before distributing
an installer.

## Maintenance

Use [README.md](../README.md) for setup and checks,
[UI_RULES.md](UI_RULES.md) for interface conventions, and
[RELEASE_RUNBOOK.md](RELEASE_RUNBOOK.md) for releases. Current behavior is defined
by the registry, service contracts, native processors, and their tests.
[CHANGELOG.md](../CHANGELOG.md) records notable completed outcomes.

New tools should reuse shared intake, export, batch, and error patterns. Keep
the registry static and processing local; add dependencies only when they
materially simplify the implementation.
