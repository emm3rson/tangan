# Toolbox

Toolbox is a local-first Windows desktop utility application built with Tauri 2, Rust, and React. It brings together essential media conversion, compression, and extraction utilities into one unified interface where files never leave your machine.

## Core Capabilities

Toolbox includes seven focused utilities:

- **Image Converter**: Convert PNG, JPG, WebP, or SVG inputs to PNG, JPG, or WebP. Supports aspect-preserving resizing (fixed dimensions or percentage), lossy quality tuning, and lossless PNG optimization.
- **Image Compressor**: Format-aware file size reduction. Applies multi-step lossless optimization via `oxipng` for PNG files, and calibrated quality encoding for JPG and WebP.
- **Web Logo Pack**: Generates a standard web asset pack from a single square raster image or vector SVG. Produces multi-resolution `favicon.ico` (16, 32, 48 px), standard PNG favicons (16x16, 32x32, 192x192, 512x512), and `apple-touch-icon.png`, accompanied by a ready-to-paste HTML snippet.
- **PDF to Markdown**: Fast offline text and structure extraction from PDF documents to Markdown via `pdf-inspector`, with explicit markers for pages requiring OCR.
- **Video Processor**: Batch transcode MP4, MOV, MKV, WebM, and AVI files to MP4 (H.264 + AAC) or WebM (VP9 + Opus) using bundled FFmpeg. Offers orientation-aware resolution caps, quality presets, intra-file progress tracking, and batch cancellation.
- **Color Palette Extractor**: Deterministic Lab-space k-means color extraction from raster or SVG images into 3 to 12 swatch palettes. Features an interactive preview canvas with draggable pins, synchronized hex editing, undo and redo history, and export as PNG, JPG, or SVG palette sheets or formatted values (HEX, RGB, HSL, CSS, JSON).
- **PDF Optimizer**: Batch optimize and compress PDF documents via bundled qpdf. Provides Lossless, Balanced, and Smaller File compression presets while preserving searchable text, form fields, links, and annotations.

## Principles

- **Local-First & Private**: Processing occurs entirely on your device. The app makes no network calls, contains no telemetry, and uses self-hosted typography.
- **Non-Destructive**: Output files never overwrite source originals. Collisions are automatically avoided through numbered filename suffixes.
- **Transactional & Safe**: Batch jobs isolate file failures. Video and PDF optimization jobs verify temporary outputs before committing them and remove partial artifacts on cancellation.

## Requirements

- **Operating System**: Windows 10/11 (x64)
- **Node.js**: 24+ with npm
- **Rust**: 1.97+ (MSVC toolchain, `x86_64-pc-windows-msvc`)

## Setup

1. Clone the repository and install frontend dependencies:
   ```bash
   npm install
   ```

2. Stage the bundled runtime binaries (FFmpeg and qpdf):
   ```bash
   npm run prepare:binaries
   ```
   This PowerShell script downloads and verifies hash-pinned archives for FFmpeg and qpdf, extracting them to `src-tauri/resources/`.

## Commands

| Command | Purpose |
|---|---|
| `npm run tauri dev` | Launch the desktop application in development mode (Vite frontend + Tauri native runtime) |
| `npm run dev` | Run the frontend Vite dev server in isolation (browser UI only) |
| `npm run build` | Type-check TypeScript and build the production frontend bundle |
| `npm run test` | Run frontend unit and integration tests using Vitest |
| `npm run test:release` | Verify release version and lockfile updates in temporary copies |
| `npm run lint` | Run ESLint across frontend TypeScript sources |
| `npm run typecheck` | Check TypeScript without building |
| `npm run format:check` | Check frontend formatting with Prettier |
| `npm run prepare:binaries` | Download and stage hash-pinned FFmpeg and qpdf runtime binaries |
| `npm run tauri -- build` | Build optimized release binaries and package the per-user NSIS installer |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Check native Rust sources for compiler diagnostics |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Execute Rust unit and integration test suites |

## Architecture

- **Frontend**: React 19, TypeScript 5.9, Vite, and Tailwind CSS. The frontend manages user interaction, queue presentation, and tool configuration.
- **Native Core**: Tauri 2 with Rust. Handles file decoding, encoding, batch concurrency, and child process execution via typed IPC commands (`src-tauri/src/`).
- **Engines & Crates**: `oxipng` for PNG optimization, `image` and `resvg` for image and vector processing, `pdf-inspector` for PDF text extraction, `kmeans_colors` for palette clustering, bundled `ffmpeg` for video transcoding, and bundled `qpdf` for PDF stream optimization.

See [PROJECT.md](docs/PROJECT.md) for tool boundaries and maintenance,
[UI_RULES.md](docs/UI_RULES.md) for interface conventions, and
[RELEASE_RUNBOOK.md](docs/RELEASE_RUNBOOK.md) for release checks. The
[build helper](scripts/README.md) can synchronize versions, validate, and package
without launching the installer.

## License

This project is licensed under the [MIT License](LICENSE).
