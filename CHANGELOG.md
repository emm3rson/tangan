# Changelog

Outcome-focused history of completed releases and capabilities.

## Release v0.5.2 - 2026-10-02

### Shared processing and reliable preferences

- Consolidated file intake, error handling, native batch summaries, and progress subscriptions while preserving tool workflows and export behavior.
- Removed unused route metadata and picker modes, and reused shared RGB and sequential-progress contract types.
- Prevented duplicate queue entries from a single intake and guarded initial preference saves until native settings finish loading.
- Synchronized release lockfiles without refreshing unrelated dependencies, restored version files when updates fail, and removed the redundant frontend build from packaging.
- Brought project and release guidance into version control and reduced documentation to current behavior and maintenance rules.

## Release v0.5.1 — 2026-09-07

### Repository Audit & Quality Hardening
- Restored complete offline privacy by self-hosting typography (`@fontsource-variable/inter` and `@fontsource/jetbrains-mono`), removing external font network requests.
- Implemented strict Content Security Policy to prevent network leakage while permitting local loopback asset protocol thumbnails.
- Improved launcher responsiveness and accessibility across controls, progress regions, and canvas pins.
- Added release automation script (`scripts/build-and-install.ps1`) to synchronize version manifests, run checks, and package the NSIS installer.

### Repository Hygiene & Icon Relocation
- Consolidated the master application icon by moving `app-icon.png` into `src-tauri/icons/app-icon.png`, and updated documentation references.

### Licensing
- Added standard MIT license (`LICENSE`) and declared the license across package manifests.

## Release v0.5.0 — 2026-08-30

### Optimize PDFs
- Added local batch PDF size reduction via bundled qpdf 12.4.1 with Lossless, Balanced, and Smaller File compression presets.
- Preserved searchable text, form fields, outlines, links, and annotations while rejecting encrypted or digitally signed documents to protect integrity.
- Standardized launcher card titles, descriptions, and grid layouts.

## Release v0.4.0 — 2026-08-28

### Color Palette Extractor
- Added deterministic Lab-space k-means color extraction (3–12 swatches) from raster or vector images.
- Implemented interactive preview canvas with draggable sample pins, channel editing (HEX, RGB, HSL), and history undo/redo.
- Supported export to PNG, JPG, and SVG palette sheets as well as formatted values (HEX, RGB, HSL, CSS, JSON).

## Release v0.3.1 — 2026-08-28

### Video Processor Hardening
- Improved stream dimension validation, stream termination handling, and single-submit batch cancellation.
- Enabled installed application metadata version reporting in the title bar.

## Release v0.3.0 — 2026-08-28

### Video Processor
- Added local batch transcoding of MP4, MOV, MKV, WebM, and AVI to MP4 (H.264 + AAC) or WebM (VP9 + Opus) via bundled FFmpeg.
- Provided orientation-aware resolution caps, quality presets, intra-file progress tracking, and batch cancellation with partial artifact cleanup.

## Release v0.2.0 — 2026-08-16

### PDF to Markdown & Image Enhancements
- Added PDF to Markdown offline text and structure extraction via `pdf-inspector`, with explicit markers for pages requiring OCR.
- Added SVG vector input support for Image Converter and Web Logo Pack via native `resvg`.
- Added 3-step lossless PNG optimization slider via `oxipng` with dynamic settings panel adaptation for mixed queues.

## Release v0.1.1 — 2026-08-16

### Visual Polish
- Added real image thumbnails in file queues via Tauri native asset protocol.
- Added uncropped square preview with transparency grid in Web Logo Pack.
- Standardized bordered settings cards and glare-free neutral studio surfaces.

## Release v0.1.0 — 2026-08-15

### Initial Release
- Initial Windows desktop release featuring Image Converter, Image Compressor, and Web Logo Pack.
- Built on local-first Tauri 2 and Rust architecture with non-destructive auto-renaming exports and per-user NSIS packaging.
