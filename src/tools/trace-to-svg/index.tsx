import { useRef, useState, type ReactNode } from 'react'
import { Button, Checkbox, ProgressBar, Segmented } from '@/components/ui'
import {
  FolderIcon,
  RotateCcwIcon,
  TraceIcon,
  XIcon,
} from '@/components/ui/icons'
import { Completion } from '@/components/processing/Completion'
import {
  DropZone,
  ExportLocation,
  FileListHeader,
  FileRow,
  toAssetUrl,
  type RowStatus,
} from '@/components/workspace'
import {
  desktop,
  type BatchResult,
  type InputFile,
  type TraceColorMode,
  type TraceDetail,
} from '@/services/tauri'
import { useSettings } from '@/app/providers/SettingsProvider'
import type { ToolDefinition } from '../types'
import { appendUniqueByPath, getErrorMessage } from '../shared/file-utils'

type Phase = 'empty' | 'editing' | 'processing' | 'done'
type TraceSettings = {
  colorMode: TraceColorMode
  detail: TraceDetail
  smooth: boolean
}

const DEFAULT_SETTINGS: TraceSettings = {
  colorMode: 'color',
  detail: 'balanced',
  smooth: true,
}

const SUPPORTED_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp'])
const MAX_PIXELS = 4_000_000

function settingsKey(settings: TraceSettings) {
  return `${settings.colorMode}:${settings.detail}:${settings.smooth}`
}

function svgDataUrl(svg: string) {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
}

export function TraceToSvg() {
  const { exportPath, chooseExportPath } = useSettings()
  const [phase, setPhase] = useState<Phase>('empty')
  const [files, setFiles] = useState<InputFile[]>([])
  const [selectedPath, setSelectedPath] = useState<string>()
  const [settings, setSettings] = useState<TraceSettings>(DEFAULT_SETTINGS)
  const [preview, setPreview] = useState<{
    sourcePath: string
    settingsKey: string
    svg: string
    width: number
    height: number
  }>()
  const [previewing, setPreviewing] = useState(false)
  const [progress, setProgress] = useState(0)
  const [currentFile, setCurrentFile] = useState<string>()
  const [result, setResult] = useState<BatchResult>()
  const [error, setError] = useState<string>()
  const previewRequestRef = useRef(0)
  const previewInFlightRef = useRef(false)
  const intakeGenerationRef = useRef(0)
  const exportInFlightRef = useRef(false)
  const phaseRef = useRef<Phase>(phase)
  phaseRef.current = phase
  const [resultDestination, setResultDestination] = useState('')

  const updatePhase = (next: Phase) => {
    phaseRef.current = next
    setPhase(next)
  }
  const intakeIsActive = () =>
    phaseRef.current !== 'processing' && phaseRef.current !== 'done'

  const invalidatePreview = () => {
    previewRequestRef.current += 1
    previewInFlightRef.current = false
    setPreview(undefined)
    setPreviewing(false)
    setError(undefined)
  }

  const addFiles = async (paths?: string[]) => {
    if (phaseRef.current === 'processing' || phaseRef.current === 'done') return
    const intakeGeneration = intakeGenerationRef.current
    try {
      const selected = paths ?? (await desktop.pickFiles('trace'))
      if (
        selected.length === 0 ||
        intakeGeneration !== intakeGenerationRef.current ||
        !intakeIsActive()
      )
        return
      const inspected = await desktop.inspectFiles(selected)
      if (intakeGeneration !== intakeGenerationRef.current || !intakeIsActive())
        return
      if (inspected.length === 0) return
      const accepted = inspected.map((file) => {
        const extension = file.extension.toLowerCase()
        if (!SUPPORTED_EXTENSIONS.has(extension)) {
          return {
            ...file,
            status: 'invalid' as const,
            error:
              extension === 'svg'
                ? 'SVG files cannot be traced. Choose a PNG, JPG, or WebP image.'
                : 'Only PNG, JPG, and WebP images can be traced.',
          }
        }
        if (file.status === 'ready' && file.width * file.height > MAX_PIXELS) {
          return {
            ...file,
            status: 'invalid' as const,
            error:
              'Image exceeds the 4,000,000 pixel limit. Resize it and add it again.',
          }
        }
        return file
      })
      setFiles((current) => appendUniqueByPath(current, accepted))
      setSelectedPath(
        (current) =>
          current ?? accepted.find((file) => file.status === 'ready')?.path,
      )
      setResult(undefined)
      setError(undefined)
      updatePhase('editing')
    } catch (cause) {
      if (
        intakeGeneration === intakeGenerationRef.current &&
        intakeIsActive()
      ) {
        setError(getErrorMessage(cause))
      }
    }
  }

  const reset = () => {
    intakeGenerationRef.current += 1
    invalidatePreview()
    setFiles([])
    setSelectedPath(undefined)
    setProgress(0)
    setCurrentFile(undefined)
    setResult(undefined)
    setError(undefined)
    updatePhase('empty')
  }

  const removeFile = (path: string) => {
    const remaining = files.filter((file) => file.path !== path)
    setFiles(remaining)
    if (path === selectedPath) {
      invalidatePreview()
      setSelectedPath(remaining.find((file) => file.status === 'ready')?.path)
    }
    if (remaining.length === 0) reset()
  }

  const selectFile = (path: string) => {
    if (path === selectedPath) return
    invalidatePreview()
    setSelectedPath(path)
  }

  const changeSettings = (next: Partial<TraceSettings>) => {
    invalidatePreview()
    setSettings((current) => ({ ...current, ...next }))
  }

  const selectedFile = files.find(
    (file) => file.path === selectedPath && file.status === 'ready',
  )
  const validFiles = files.filter((file) => file.status === 'ready')
  const activePreview =
    preview &&
    selectedFile &&
    preview.sourcePath === selectedFile.path &&
    preview.settingsKey === settingsKey(settings)
      ? preview
      : undefined

  const makePreview = async (source = selectedFile) => {
    if (
      !source ||
      previewInFlightRef.current ||
      phaseRef.current === 'processing'
    )
      return
    const requestId = ++previewRequestRef.current
    previewInFlightRef.current = true
    setPreview(undefined)
    setPreviewing(true)
    setError(undefined)
    try {
      const next = await desktop.previewTrace({
        sourcePath: source.path,
        ...settings,
      })
      if (requestId !== previewRequestRef.current) return
      setPreview({
        sourcePath: source.path,
        settingsKey: settingsKey(settings),
        ...next,
      })
    } catch (cause) {
      if (requestId === previewRequestRef.current)
        setError(getErrorMessage(cause))
    } finally {
      if (requestId === previewRequestRef.current) {
        previewInFlightRef.current = false
        setPreviewing(false)
      }
    }
  }

  const trace = async () => {
    if (exportInFlightRef.current || phaseRef.current === 'processing') return
    const paths = validFiles.map((file) => file.path)
    if (paths.length === 0) {
      setError('No supported images to trace.')
      return
    }
    exportInFlightRef.current = true
    invalidatePreview()
    setError(undefined)
    updatePhase('processing')
    setProgress(0)
    setCurrentFile(paths[0])
    try {
      const destination = exportPath || (await chooseExportPath())
      if (!destination) {
        updatePhase('editing')
        return
      }
      setResultDestination(destination)
      const next = await desktop.traceImages(
        { files: paths, outputDirectory: destination, ...settings },
        ({ completed, total }) => {
          setProgress(total > 0 ? Math.round((completed / total) * 100) : 0)
          setCurrentFile(paths[Math.min(completed, paths.length - 1)])
        },
      )
      setResult(next)
      updatePhase('done')
    } catch (cause) {
      setError(getErrorMessage(cause))
      updatePhase('editing')
    } finally {
      exportInFlightRef.current = false
    }
  }

  if (phase === 'empty') {
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        {error && (
          <div
            role="alert"
            className="mb-3.5 rounded-[var(--radius)] border border-danger/25 bg-danger-surface/70 px-4 py-3 text-[13px] text-danger"
          >
            {error}
          </div>
        )}
        <DropZone
          onAdd={addFiles}
          hint="Best for logos, icons, and illustrations. Photos become stylized shapes and may lose detail. PNG, JPG, and WebP only."
        />
      </div>
    )
  }

  if (phase === 'done' && result) {
    const failures = result.items.filter((item) => !item.success)
    const allFailed = result.succeeded === 0
    const partial = result.succeeded > 0 && failures.length > 0
    return (
      <div className="mx-auto max-w-[760px]">
        <Completion
          status={allFailed ? 'error' : partial ? 'warning' : 'success'}
          headline={
            allFailed
              ? 'No images traced'
              : `${result.succeeded} ${result.succeeded === 1 ? 'image' : 'images'} traced to SVG`
          }
          metrics={
            !allFailed ? (
              <p className="font-mono text-[13px] text-muted-foreground">
                Saved to{' '}
                <span className="text-foreground">{resultDestination}</span>
              </p>
            ) : null
          }
          failures={failures}
          actions={
            allFailed ? (
              <Button
                variant="primary"
                icon={<RotateCcwIcon size={15} />}
                onClick={() => updatePhase('editing')}
              >
                Try again
              </Button>
            ) : (
              <>
                <Button
                  variant="secondary"
                  icon={<FolderIcon size={15} />}
                  onClick={() => void desktop.openFolder(resultDestination)}
                >
                  Open destination folder
                </Button>
                <Button
                  variant={partial ? 'secondary' : 'primary'}
                  icon={<RotateCcwIcon size={15} />}
                  onClick={reset}
                >
                  {partial ? 'Done' : 'Trace more images'}
                </Button>
              </>
            )
          }
        />
      </div>
    )
  }

  const statuses = (file: InputFile): RowStatus =>
    file.status === 'invalid' ? 'failed' : 'idle'
  const invalidCount = files.length - validFiles.length

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_340px] lg:items-start">
      <section>
        {phase === 'processing' ? (
          <div
            className="mb-3.5"
            role="status"
            aria-live="polite"
            aria-busy="true"
          >
            <div className="mb-2 flex items-center justify-between">
              <span className="text-[13.5px] font-medium">
                Tracing{' '}
                {currentFile ? currentFile.split(/[/\\]/).pop() : 'images'}…
              </span>
              <span className="font-mono text-[12px] text-muted-foreground">
                {progress}%
              </span>
            </div>
            <ProgressBar value={progress} />
          </div>
        ) : (
          <FileListHeader
            count={files.length}
            onAddMore={addFiles}
            onClear={reset}
          />
        )}

        {error && (
          <div
            role="alert"
            className="mb-3.5 rounded-[var(--radius)] border border-danger/25 bg-danger-surface/70 px-4 py-3 text-[13px] text-danger"
          >
            {error}
          </div>
        )}
        {invalidCount > 0 && (
          <p className="mb-3.5 text-[12.5px] text-muted-foreground">
            {invalidCount} unsupported {invalidCount === 1 ? 'file' : 'files'}{' '}
            excluded from export
          </p>
        )}

        <div className="rounded-[var(--radius)] border border-border bg-card p-1.5 space-y-0.5 max-h-[440px] overflow-y-auto">
          {files.map((file) => (
            <div
              key={file.path}
              className={`flex items-center gap-1 rounded-[var(--radius-sm)] ${file.path === selectedPath ? 'bg-muted/50 ring-1 ring-border-strong' : ''}`}
            >
              <div className="min-w-0 flex-1">
                <FileRow
                  file={file}
                  status={statuses(file)}
                  error={file.status === 'invalid' ? file.error : undefined}
                />
              </div>
              {file.status === 'ready' && (
                <Button
                  size="sm"
                  variant={file.path === selectedPath ? 'secondary' : 'ghost'}
                  disabled={phase === 'processing'}
                  onClick={() => selectFile(file.path)}
                  aria-label={`Select ${file.name}`}
                  aria-pressed={file.path === selectedPath}
                >
                  {file.path === selectedPath ? 'Selected' : 'Select'}
                </Button>
              )}
              {phase !== 'processing' && (
                <button
                  type="button"
                  aria-label={`Remove ${file.name}`}
                  onClick={() => removeFile(file.path)}
                  className="mr-1 grid h-7 w-7 shrink-0 place-items-center rounded text-subtle-foreground hover:bg-danger-surface hover:text-danger"
                >
                  <XIcon size={14} />
                </button>
              )}
            </div>
          ))}
        </div>

        {selectedFile && (
          <div className="mt-6">
            <div className="mb-2.5 flex items-baseline justify-between">
              <h3 className="text-[11px] font-semibold uppercase tracking-[.09em] text-subtle-foreground">
                Preview comparison
              </h3>
              <div className="flex min-w-0 items-center gap-3 pl-3">
                <span className="truncate font-mono text-[11.5px] text-muted-foreground">
                  {selectedFile.name}
                </span>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={phase === 'processing' || previewing}
                  onClick={() => void makePreview()}
                >
                  {previewing ? 'Tracing preview…' : 'Preview trace'}
                </Button>
              </div>
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <PreviewCard
                label="Source image"
                description={`${(activePreview?.width ?? selectedFile.width).toLocaleString()} × ${(activePreview?.height ?? selectedFile.height).toLocaleString()} px`}
              >
                <img
                  src={toAssetUrl(selectedFile.path)}
                  alt={`Source ${selectedFile.name}`}
                  className="max-h-full max-w-full object-contain"
                />
              </PreviewCard>
              <PreviewCard
                label="Generated SVG"
                description={
                  activePreview
                    ? `${activePreview.width.toLocaleString()} × ${activePreview.height.toLocaleString()} px`
                    : 'Generate a preview to compare'
                }
              >
                {activePreview ? (
                  <img
                    src={svgDataUrl(activePreview.svg)}
                    alt={`Generated SVG preview of ${selectedFile.name}`}
                    className="max-h-full max-w-full object-contain"
                  />
                ) : (
                  <div className="px-4 text-center text-[12.5px] text-muted-foreground">
                    {previewing
                      ? 'Generating trace preview…'
                      : 'Select Preview trace to see the result.'}
                  </div>
                )}
              </PreviewCard>
            </div>
            <p className="mt-2.5 text-[12px] text-muted-foreground">
              Partially transparent pixels are flattened onto white. Fully
              transparent areas stay transparent.
            </p>
          </div>
        )}
      </section>

      <aside className="space-y-5 lg:sticky lg:top-20">
        <fieldset
          disabled={phase === 'processing'}
          className="space-y-5 disabled:opacity-70"
        >
          <div className="space-y-4 rounded-[var(--radius-lg)] border border-border-strong bg-card p-4">
            <div>
              <span className="mb-2.5 block text-[13px] font-medium">
                Color mode
              </span>
              <Segmented
                ariaLabel="Color mode"
                value={settings.colorMode}
                onChange={(colorMode) => changeSettings({ colorMode })}
                options={[
                  { value: 'color', label: 'Color' },
                  { value: 'binary', label: 'Black & white' },
                ]}
              />
            </div>
            <div className="border-t border-border pt-3.5">
              <span className="mb-2.5 block text-[13px] font-medium">
                Detail
              </span>
              <Segmented
                ariaLabel="Detail"
                value={settings.detail}
                onChange={(detail) => changeSettings({ detail })}
                options={[
                  { value: 'low', label: 'Low' },
                  { value: 'balanced', label: 'Balanced' },
                  { value: 'high', label: 'High' },
                ]}
              />
              <p className="mt-2.5 text-[12.5px] leading-relaxed text-muted-foreground">
                Higher detail can create larger, more complex SVG files.
              </p>
            </div>
            <div className="border-t border-border pt-3.5">
              <label className="flex cursor-pointer items-center gap-2.5 text-[13px] font-medium">
                <Checkbox
                  checked={settings.smooth}
                  onChange={(smooth) => changeSettings({ smooth })}
                />
                Smooth curves
              </label>
            </div>
          </div>
          <ExportLocation path={exportPath} onChange={chooseExportPath} />
          <Button
            variant="primary"
            className="h-11 w-full"
            disabled={phase === 'processing' || validFiles.length === 0}
            onClick={trace}
          >
            {phase === 'processing'
              ? 'Tracing…'
              : `Trace ${validFiles.length} ${validFiles.length === 1 ? 'image' : 'images'}`}
          </Button>
        </fieldset>
      </aside>
    </div>
  )
}

function PreviewCard({
  label,
  description,
  children,
}: {
  label: string
  description: string
  children: ReactNode
}) {
  return (
    <div className="overflow-hidden rounded-[var(--radius)] border border-border bg-card">
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <span className="text-[12.5px] font-medium">{label}</span>
        <span className="font-mono text-[10.5px] text-muted-foreground">
          {description}
        </span>
      </div>
      <div className="flex aspect-[4/3] items-center justify-center overflow-hidden bg-transparency-grid p-4">
        {children}
      </div>
    </div>
  )
}

export const traceToSvgDefinition: ToolDefinition = {
  id: 'trace',
  name: 'Trace to SVG',
  description: 'Convert raster images into scalable SVG artwork.',
  icon: TraceIcon,
  component: TraceToSvg,
}
