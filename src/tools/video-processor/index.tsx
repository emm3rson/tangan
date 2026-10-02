import { useRef, useState } from 'react'
import { Button, Segmented } from '@/components/ui'
import { FolderIcon, RotateCcwIcon, VideoIcon } from '@/components/ui/icons'
import { BatchProgress } from '@/components/processing/BatchProgress'
import { BeforeAfter, Completion } from '@/components/processing/Completion'
import {
  DropZone,
  ExportLocation,
  FileListHeader,
  FileRow,
  type RowStatus,
} from '@/components/workspace'
import {
  desktop,
  type InputFile,
  type VideoBatchResult,
  type VideoOutputFormat,
  type VideoProcessingProgress,
  type VideoQualityPreset,
  type VideoResolutionPreset,
} from '@/services/tauri'
import { useSettings } from '@/app/providers/SettingsProvider'
import type { ToolDefinition } from '../types'
import { appendUniqueByPath, getErrorMessage } from '../shared/file-utils'

type Phase = 'empty' | 'editing' | 'processing' | 'done'

export function VideoProcessor() {
  const { exportPath, chooseExportPath, tool, setToolPrefs } = useSettings()
  const [phase, setPhase] = useState<Phase>('empty')
  const [files, setFiles] = useState<InputFile[]>([])
  const [outputFormat, setOutputFormat] = useState<VideoOutputFormat>(
    tool.videoProcessor?.outputFormat ?? 'mp4'
  )
  const [quality, setQuality] = useState<VideoQualityPreset>(
    tool.videoProcessor?.quality ?? 'balanced'
  )
  const [resolution, setResolution] = useState<VideoResolutionPreset>('original')
  const [completedFiles, setCompletedFiles] = useState(0)
  const [totalFiles, setTotalFiles] = useState(0)
  const [currentFile, setCurrentFile] = useState<string>()
  const [progress, setProgress] = useState(0)
  const [runTotal, setRunTotal] = useState(0)
  const [result, setResult] = useState<VideoBatchResult>()
  const [error, setError] = useState<string>()
  const [cancelRequested, setCancelRequested] = useState(false)
  const jobIdRef = useRef<string>('')

  const changeFormat = (next: VideoOutputFormat) => {
    setOutputFormat(next)
    setToolPrefs('videoProcessor', { outputFormat: next })
  }

  const changeQuality = (next: VideoQualityPreset) => {
    setQuality(next)
    setToolPrefs('videoProcessor', { quality: next })
  }

  const addFiles = async (paths?: string[]) => {
    const selected = paths ?? (await desktop.pickFiles('video'))
    if (selected.length === 0) return
    const inspected = await desktop.inspectVideos(selected)
    if (inspected.length === 0) return
    setFiles((prev) => appendUniqueByPath(prev, inspected))
    setPhase('editing')
  }

  const reset = () => {
    setFiles([])
    setCompletedFiles(0)
    setTotalFiles(0)
    setCurrentFile(undefined)
    setProgress(0)
    setResult(undefined)
    setError(undefined)
    setCancelRequested(false)
    setPhase('empty')
  }

  const resetToEditing = () => {
    setCompletedFiles(0)
    setTotalFiles(0)
    setCurrentFile(undefined)
    setProgress(0)
    setResult(undefined)
    setError(undefined)
    setCancelRequested(false)
    setPhase('editing')
  }

  const removeFile = (path: string) => {
    const next = files.filter((item) => item.path !== path)
    setFiles(next)
    if (next.length === 0) {
      reset()
    }
  }

  const process = async () => {
    const destination = exportPath || (await chooseExportPath())
    if (!destination) return
    const validPaths = files
      .filter((file) => file.status === 'ready')
      .map((file) => file.path)
    if (validPaths.length === 0) {
      setError('No supported video files to process.')
      return
    }
    const jobId = crypto.randomUUID()
    jobIdRef.current = jobId
    setError(undefined)
    setPhase('processing')
    setProgress(0)
    setCompletedFiles(0)
    setTotalFiles(validPaths.length)
    setRunTotal(validPaths.length)
    setCancelRequested(false)

    const onProgress = ({
      completedFiles: completed,
      totalFiles: total,
      currentFile: active,
      currentFilePercent,
    }: VideoProcessingProgress) => {
      setCompletedFiles(completed)
      setTotalFiles(total)
      setCurrentFile(active)
      setProgress(
        Math.round(((completed + (currentFilePercent ?? 0) / 100) / total) * 100)
      )
    }

    try {
      const batchResult = await desktop.processVideos(
        {
          files: validPaths,
          outputDirectory: destination,
          outputFormat,
          resolution,
          quality,
          jobId,
        },
        onProgress
      )
      setResult(batchResult)
      setPhase('done')
    } catch (e) {
      setPhase('editing')
      setCompletedFiles(0)
      setTotalFiles(0)
      setCurrentFile(undefined)
      setProgress(0)
      setCancelRequested(false)
      setError(getErrorMessage(e))
    }
  }

  const cancel = async () => {
    if (!jobIdRef.current || cancelRequested) return
    setCancelRequested(true)
    try {
      await desktop.cancelVideoJob(jobIdRef.current)
    } catch (e) {
      setCancelRequested(false)
      setError(getErrorMessage(e))
    }
  }

  if (phase === 'empty') {
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <DropZone
          onAdd={addFiles}
          label="Drop videos here"
          hint="Supports MP4, MOV, MKV, WebM and AVI. Files are processed locally with a bundled FFmpeg engine: nothing is uploaded."
        />
      </div>
    )
  }

  if (phase === 'done' && result) {
    const canceled = result.status === 'canceled'
    const failures = result.items.filter((item) => !item.success)
    const warnings = result.items.filter(
      (item) => item.success && item.warnings && item.warnings.length > 0
    )
    const succeeded = result.succeeded
    const isAllFailed = !canceled && succeeded === 0
    const isPartial =
      !canceled && !isAllFailed && (failures.length > 0 || warnings.length > 0)
    const status = canceled
      ? 'warning'
      : isAllFailed
        ? 'error'
        : isPartial
          ? 'warning'
          : 'success'
    const headline = canceled
      ? 'Batch canceled'
      : isAllFailed
        ? 'No files processed'
        : `${succeeded} ${succeeded === 1 ? 'file' : 'files'} processed to ${outputFormat.toUpperCase()}`

    const successfulItems = result.items.filter((item) => item.success)
    const finishedBeforeCancel = result.items.filter(
      (item) => item.error?.code !== 'CANCELED'
    ).length
    const before = successfulItems.reduce((sum, item) => sum + item.originalSize, 0)
    const after = successfulItems.reduce(
      (sum, item) => sum + (item.outputSize ?? 0),
      0
    )

    return (
      <div className="mx-auto max-w-[760px]">
        <Completion
          status={status}
          headline={headline}
          metrics={
            succeeded > 0 ? (
              <div className="space-y-2">
                <BeforeAfter before={before} after={after} />
                <p className="font-mono text-[13px] text-muted-foreground">
                  Saved to <span className="text-foreground">{exportPath}</span>
                </p>
              </div>
            ) : null
          }
          warnings={warnings}
          failures={failures}
          actions={
            canceled ? (
              <>
                {succeeded > 0 && (
                  <Button
                    variant="secondary"
                    icon={<FolderIcon size={15} />}
                    onClick={() => void desktop.openFolder(exportPath)}
                  >
                    Open folder
                  </Button>
                )}
                <Button
                  variant="primary"
                  icon={<RotateCcwIcon size={15} />}
                  onClick={reset}
                >
                  Process more files
                </Button>
              </>
            ) : isAllFailed ? (
              <Button
                variant="primary"
                icon={<RotateCcwIcon size={15} />}
                onClick={resetToEditing}
              >
                Try again
              </Button>
            ) : (
              <>
                <Button
                  variant="secondary"
                  icon={<FolderIcon size={15} />}
                  onClick={() => void desktop.openFolder(exportPath)}
                >
                  Open folder
                </Button>
                <Button
                  variant={isPartial ? 'secondary' : 'primary'}
                  icon={<RotateCcwIcon size={15} />}
                  onClick={reset}
                >
                  Process more files
                </Button>
              </>
            )
          }
        >
          {canceled && (
            <p className="mt-4 text-[13px] text-muted-foreground">
              {finishedBeforeCancel} of {runTotal} files finished before
              cancellation. Partial outputs were discarded; completed files were
              kept.
            </p>
          )}
        </Completion>
      </div>
    )
  }

  const readyPaths = files
    .filter((file) => file.status === 'ready')
    .map((file) => file.path)
  const readyIndex = new Map(readyPaths.map((path, index) => [path, index]))

  const statuses = (file: InputFile): RowStatus => {
    if (file.status === 'invalid') return 'failed'
    if (phase !== 'processing') {
      const item = result?.items.find((entry) => entry.sourcePath === file.path)
      return item ? (item.success ? 'done' : 'failed') : 'idle'
    }
    const index = readyIndex.get(file.path)
    if (index !== undefined && index < completedFiles) return 'done'
    if (currentFile === file.path) return 'processing'
    return 'idle'
  }

  const skipped = files.filter((file) => file.status === 'invalid').length
  const readyCount = readyPaths.length

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_340px] lg:items-start">
      <section>
        {phase !== 'processing' ? (
          <FileListHeader
            count={files.length}
            onAddMore={addFiles}
            onClear={reset}
          />
        ) : (
          <BatchProgress
            completedFiles={completedFiles}
            totalFiles={totalFiles}
            progress={progress}
            cancelRequested={cancelRequested}
            onCancel={() => void cancel()}
          />
        )}

        {error && (
          <div className="mb-3.5 rounded-[var(--radius)] border border-danger/25 bg-danger-surface/70 px-4 py-3 text-[13px] text-danger">
            {error}
          </div>
        )}

        {phase !== 'processing' && skipped > 0 && (
          <p className="mb-3.5 text-[12.5px] text-muted-foreground">
            {skipped} unsupported {skipped === 1 ? 'file' : 'files'} excluded from
            processing
          </p>
        )}

        <div className="rounded-[var(--radius)] border border-border bg-card p-1.5 space-y-0.5 max-h-[440px] overflow-y-auto">
          {files.map((file) => (
            <FileRow
              key={file.path}
              file={file}
              status={statuses(file)}
              error={file.status === 'invalid' ? file.error : undefined}
              onRemove={
                phase === 'processing'
                  ? undefined
                  : () => removeFile(file.path)
              }
            />
          ))}
        </div>
      </section>

      <aside className="lg:sticky lg:top-20 space-y-5">
        <div className="rounded-[var(--radius-lg)] border border-border-strong bg-card p-4 space-y-4">
          <div>
            <span className="block text-[13px] font-medium mb-2.5">
              Output format
            </span>
            <Segmented
              value={outputFormat}
              onChange={changeFormat}
              options={[
                { value: 'mp4', label: 'MP4' },
                { value: 'webm', label: 'WebM' },
              ]}
            />
            <p className="mt-2.5 text-[12.5px] text-muted-foreground leading-relaxed">
              {outputFormat === 'mp4'
                ? 'H.264 + AAC. Widest compatibility.'
                : 'VP9 + Opus. Modern and efficient, slower to encode.'}
            </p>
          </div>

          <div className="pt-3.5 border-t border-border">
            <span className="block text-[13px] font-medium mb-2.5">Quality</span>
            <Segmented
              value={quality}
              onChange={changeQuality}
              options={[
                { value: 'high', label: 'High quality' },
                { value: 'balanced', label: 'Balanced' },
                { value: 'small', label: 'Smaller file' },
              ]}
            />
            <p className="mt-2.5 text-[12.5px] text-muted-foreground leading-relaxed">
              {quality === 'high'
                ? 'Best fidelity, larger files and slower encodes.'
                : quality === 'balanced'
                  ? 'Good balance of quality, size and speed.'
                  : 'Prioritizes file size over fidelity.'}
            </p>
          </div>

          <div className="pt-3.5 border-t border-border">
            <span className="block text-[13px] font-medium mb-2.5">
              Resolution
            </span>
            <Segmented
              value={resolution}
              onChange={setResolution}
              options={[
                { value: 'original', label: 'Original' },
                { value: '1080p', label: '1080p' },
                { value: '720p', label: '720p' },
                { value: '480p', label: '480p' },
              ]}
            />
            <p className="mt-2.5 text-[12.5px] text-muted-foreground leading-relaxed">
              Bounding box cap: larger videos are scaled down, smaller videos
              are never upscaled.
            </p>
          </div>
        </div>

        <ExportLocation path={exportPath} onChange={chooseExportPath} />

        <Button
          variant="primary"
          className="w-full h-11"
          disabled={phase === 'processing' || readyCount === 0}
          onClick={process}
        >
          {phase === 'processing'
            ? 'Processing…'
            : `Process ${readyCount} ${readyCount === 1 ? 'file' : 'files'}`}
        </Button>
      </aside>
    </div>
  )
}

export const videoProcessorDefinition: ToolDefinition = {
  id: 'video',
  name: 'Process Videos',
  description: 'Compress, resize, and convert videos to MP4 or WebM.',
  icon: VideoIcon,
  component: VideoProcessor,
}
