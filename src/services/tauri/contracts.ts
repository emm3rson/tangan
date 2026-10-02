export type ImageFormat = 'png' | 'jpeg' | 'webp'
export type PngOptimizationLevel = 'fast' | 'balanced' | 'max'
export type ResizeOptions =
  | { mode: 'original' }
  | { mode: 'dimensions'; width: number; height: number; lockAspectRatio: boolean }
  | { mode: 'percentage'; percentage: number }

export type ProcessingErrorCode =
  | 'UNSUPPORTED_FORMAT' | 'INVALID_IMAGE' | 'INVALID_DIMENSIONS' | 'FILE_NOT_FOUND'
  | 'PERMISSION_DENIED' | 'OUTPUT_UNAVAILABLE' | 'ENCODE_FAILED' | 'DECODE_FAILED'
  | 'WRITE_FAILED' | 'PROCESSING_FAILED'
  | 'INVALID_PDF' | 'ENCRYPTED_PDF' | 'OCR_REQUIRED' | 'LIMIT_EXCEEDED'
  | 'INVALID_VIDEO' | 'VIDEO_ENGINE_UNAVAILABLE' | 'PDF_ENGINE_UNAVAILABLE' | 'CANCELED'

export interface ProcessingError { code: ProcessingErrorCode; message: string; detail?: string }
export type FileWarningCode =
  | 'OCR_REQUIRED_PAGES'
  | 'VIDEO_OMITTED_AUDIO' | 'VIDEO_OMITTED_SUBTITLES'
  | 'VIDEO_OMITTED_ATTACHMENTS' | 'VIDEO_OMITTED_DATA'
export interface FileWarning { code: FileWarningCode; message: string; pages?: number[] }
export interface InputFile {
  path: string; name: string; extension: string; size: number; width: number; height: number
  status: 'ready' | 'invalid'; error?: string; duration?: number
}
export interface FileResult {
  sourcePath: string; outputPath?: string; success: boolean; originalSize: number
  outputSize?: number; error?: ProcessingError; warnings?: FileWarning[]
}
export interface BatchResult { total: number; succeeded: number; failed: number; items: FileResult[] }
export interface ProcessingProgress { jobId: string; completed: number; total: number; currentFile?: string }
export type ProgressHandler = (progress: ProcessingProgress) => void

export interface ConvertImagesRequest {
  files: string[]
  outputDirectory: string
  outputFormat: ImageFormat
  quality?: number
  pngLevel?: PngOptimizationLevel
  resize: ResizeOptions
}
export interface CompressImagesRequest {
  files: string[]
  outputDirectory: string
  quality: number
  pngLevel?: PngOptimizationLevel
  resize: ResizeOptions
}
export interface ConvertPdfsRequest { files: string[]; outputDirectory: string; jobId?: string }
export interface GenerateLogoPackRequest { sourcePath: string; outputDirectory: string; assetIds: string[] }
export interface LogoAssetDefinition { id: string; filename: string; width: number; height: number; format: 'png' | 'ico'; defaultEnabled: boolean }
export interface GenerateLogoPackResult { packDirectory: string; batch: BatchResult }

export interface RgbColor { r: number; g: number; b: number }
export interface PaletteColor extends RgbColor { x: number; y: number }
export interface ExtractColorPaletteRequest { sourcePath: string; targetCount: number }
export interface ExtractColorPaletteResult {
  width: number
  height: number
  previewWidth: number
  previewHeight: number
  previewBase64: string
  colors: PaletteColor[]
  notice?: string
}
export type PaletteExportFormat = 'png' | 'jpg' | 'svg'
export interface ExportColorPaletteRequest {
  sourcePath: string
  colors: RgbColor[]
  outputDirectory: string
  format: PaletteExportFormat
}
export interface ExportColorPaletteResult { outputPath: string }

export type VideoOutputFormat = 'mp4' | 'webm'
export type VideoQualityPreset = 'high' | 'balanced' | 'small'
export type VideoResolutionPreset = 'original' | '1080p' | '720p' | '480p'
export type BatchStatus = 'completed' | 'canceled'
export type VideoBatchStatus = BatchStatus

export interface VideoProcessRequest {
  files: string[]
  outputDirectory: string
  outputFormat: VideoOutputFormat
  resolution: VideoResolutionPreset
  quality: VideoQualityPreset
  jobId: string
}
export interface SequentialProcessingProgress {
  jobId: string
  completedFiles: number
  totalFiles: number
  currentFile?: string
  currentFilePercent?: number
}
export type VideoProcessingProgress = SequentialProcessingProgress
export type VideoProgressHandler = (progress: VideoProcessingProgress) => void
export interface VideoBatchResult extends BatchResult {
  status: VideoBatchStatus
}

export type PdfOptimizationPreset = 'lossless' | 'balanced' | 'smaller'
export type PdfOptimizeOutcome = 'optimized' | 'alreadyOptimized' | 'failed' | 'canceled'
export type PdfOptimizeBatchStatus = BatchStatus

export interface OptimizePdfsRequest {
  files: string[]
  outputDirectory: string
  preset: PdfOptimizationPreset
  jobId: string
}
export interface OptimizePdfFileResult {
  sourcePath: string
  outcome: PdfOptimizeOutcome
  originalSize: number
  outputPath?: string
  outputSize?: number
  error?: ProcessingError
  warnings?: FileWarning[]
}
export interface OptimizePdfBatchResult {
  status: PdfOptimizeBatchStatus
  total: number
  optimized: number
  alreadyOptimized: number
  failed: number
  items: OptimizePdfFileResult[]
}
export type PdfOptimizationProgress = SequentialProcessingProgress
export type PdfOptimizeProgressHandler = (progress: PdfOptimizationProgress) => void

export interface TauriAdapter {
  inspectFiles(paths: string[]): Promise<InputFile[]>
  inspectPdfs(paths: string[]): Promise<InputFile[]>
  inspectPdfsForOptimization(paths: string[]): Promise<InputFile[]>
  inspectVideos(paths: string[]): Promise<InputFile[]>
  convertImages(request: ConvertImagesRequest, onProgress?: ProgressHandler): Promise<BatchResult>
  compressImages(request: CompressImagesRequest, onProgress?: ProgressHandler): Promise<BatchResult>
  convertPdfs(request: ConvertPdfsRequest, onProgress?: ProgressHandler): Promise<BatchResult>
  generateLogoPack(request: GenerateLogoPackRequest, onProgress?: ProgressHandler): Promise<GenerateLogoPackResult>
  processVideos(request: VideoProcessRequest, onProgress?: VideoProgressHandler): Promise<VideoBatchResult>
  cancelVideoJob(jobId: string): Promise<void>
  optimizePdfs(request: OptimizePdfsRequest, onProgress?: PdfOptimizeProgressHandler): Promise<OptimizePdfBatchResult>
  cancelPdfOptimizationJob(jobId: string): Promise<void>
  getLogoPresets(): Promise<LogoAssetDefinition[]>
  extractColorPalette(request: ExtractColorPaletteRequest): Promise<ExtractColorPaletteResult>
  exportColorPalette(request: ExportColorPaletteRequest): Promise<ExportColorPaletteResult>
  pickFiles(mode: 'convert' | 'compress' | 'logo' | 'pdf' | 'video' | 'palette'): Promise<string[]>
  pickFolder(): Promise<string | null>
  openFolder(path: string): Promise<void>
  getVersion(): Promise<string>
}
