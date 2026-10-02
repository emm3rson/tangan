import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { openPath } from '@tauri-apps/plugin-opener'
import packageJson from '../../../package.json'
import type {
  BatchResult,
  ExportColorPaletteResult,
  ExtractColorPaletteResult,
  GenerateLogoPackResult,
  InputFile,
  LogoAssetDefinition,
  OptimizePdfBatchResult,
  ProcessingProgress,
  ProgressHandler,
  SequentialProcessingProgress,
  TauriAdapter,
  VideoBatchResult
} from './contracts'

const IMAGE_FILTERS = [
  { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }
]
const COMPRESS_FILTERS = [
  { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }
]
const PDF_FILTERS = [{ name: 'PDF Documents', extensions: ['pdf'] }]
const VIDEO_FILTERS = [
  { name: 'Videos', extensions: ['mp4', 'mov', 'mkv', 'webm', 'avi'] }
]
const PROCESSING_EVENT = 'processing-progress'
const VIDEO_PROGRESS_EVENT = 'video-progress'
const PDF_OPTIMIZE_PROGRESS_EVENT = 'pdf-optimize-progress'

async function invokeWithProgress<T, P extends { jobId: string }>(
  command: string,
  request: { jobId: string },
  eventName: string,
  onProgress?: (progress: P) => void
): Promise<T> {
  const unlisten = onProgress
    ? await listen<P>(eventName, (event) => {
        if (event.payload.jobId === request.jobId) onProgress(event.payload)
      })
    : undefined
  try {
    return await invoke<T>(command, { request })
  } finally {
    unlisten?.()
  }
}

async function runWithProgress<T>(
  command: string,
  request: object,
  onProgress?: ProgressHandler
): Promise<T> {
  return invokeWithProgress<T, ProcessingProgress>(
    command,
    { ...request, jobId: crypto.randomUUID() },
    PROCESSING_EVENT,
    onProgress
  )
}

export const realAdapter: TauriAdapter = {
  async pickFiles(mode) {
    const filters =
      mode === 'pdf'
        ? PDF_FILTERS
        : mode === 'video'
          ? VIDEO_FILTERS
          : mode === 'compress'
            ? COMPRESS_FILTERS
            : IMAGE_FILTERS
    const selected = await open({
      multiple: mode !== 'logo' && mode !== 'palette',
      directory: false,
      filters
    })
    return selected === null
      ? []
      : Array.isArray(selected)
        ? selected
        : [selected]
  },
  async pickFolder() {
    return open({ directory: true })
  },
  async openFolder(path) {
    await openPath(path)
  },
  async inspectFiles(paths) {
    return invoke<InputFile[]>('inspect_files', { paths })
  },
  async inspectPdfs(paths) {
    return invoke<InputFile[]>('inspect_pdfs', { paths })
  },
  async inspectPdfsForOptimization(paths) {
    return invoke<InputFile[]>('inspect_pdfs_for_optimization', { paths })
  },
  convertImages(request, onProgress) {
    return runWithProgress<BatchResult>('convert_images', request, onProgress)
  },
  compressImages(request, onProgress) {
    return runWithProgress<BatchResult>('compress_images', request, onProgress)
  },
  convertPdfs(request, onProgress) {
    return runWithProgress<BatchResult>('convert_pdfs', request, onProgress)
  },
  generateLogoPack(request, onProgress) {
    return runWithProgress<GenerateLogoPackResult>(
      'generate_logo_pack',
      request,
      onProgress
    )
  },
  processVideos(request, onProgress) {
    return invokeWithProgress<VideoBatchResult, SequentialProcessingProgress>(
      'process_videos',
      request,
      VIDEO_PROGRESS_EVENT,
      onProgress
    )
  },
  async cancelVideoJob(jobId) {
    await invoke('cancel_video_job', { jobId })
  },
  optimizePdfs(request, onProgress) {
    return invokeWithProgress<
      OptimizePdfBatchResult,
      SequentialProcessingProgress
    >('optimize_pdfs', request, PDF_OPTIMIZE_PROGRESS_EVENT, onProgress)
  },
  async cancelPdfOptimizationJob(jobId) {
    await invoke('cancel_pdf_optimization_job', { jobId })
  },
  async inspectVideos(paths) {
    return invoke<InputFile[]>('inspect_videos', { paths })
  },
  async getLogoPresets() {
    return invoke<LogoAssetDefinition[]>('get_logo_presets')
  },
  async extractColorPalette(request) {
    return invoke<ExtractColorPaletteResult>('extract_color_palette', {
      request
    })
  },
  async exportColorPalette(request) {
    return invoke<ExportColorPaletteResult>('export_color_palette', { request })
  },
  async getVersion() {
    try {
      return await getVersion()
    } catch {
      return packageJson.version
    }
  }
}
