import { useRef, useState } from 'react'
import { Button, Segmented } from '@/components/ui'
import { DocumentIcon, FolderIcon, RotateCcwIcon } from '@/components/ui/icons'
import { BatchProgress } from '@/components/processing/BatchProgress'
import { BeforeAfter, Completion } from '@/components/processing/Completion'
import {
  DropZone,
  ExportLocation,
  FileListHeader,
  FileRow,
  formatBytes,
  type RowStatus,
} from '@/components/workspace'
import {
  desktop,
  type FileResult,
  type InputFile,
  type OptimizePdfBatchResult,
  type PdfOptimizationPreset,
  type PdfOptimizationProgress,
} from '@/services/tauri'
import { useSettings } from '@/app/providers/SettingsProvider'
import type { ToolDefinition } from '../types'
import { appendUniqueByPath, getErrorMessage } from '../shared/file-utils'

type Phase = 'empty' | 'editing' | 'processing' | 'done'

const PRESET_COPY: Record<PdfOptimizationPreset, string> = {
  lossless: 'Optimizes structure without intentional quality loss.',
  balanced: 'May recompress supported images at good screen/share quality.',
  smaller: 'Applies stronger supported-image recompression for smaller files.',
}

export function PdfOptimizer() {
  const { exportPath, chooseExportPath, tool, setToolPrefs } = useSettings()
  const [phase, setPhase] = useState<Phase>('empty')
  const [files, setFiles] = useState<InputFile[]>([])
  const [preset, setPreset] = useState<PdfOptimizationPreset>(
    tool.pdfOptimizer?.preset ?? 'balanced'
  )
  const [completedFiles, setCompletedFiles] = useState(0)
  const [totalFiles, setTotalFiles] = useState(0)
  const [currentFile, setCurrentFile] = useState<string>()
  const [currentPercent, setCurrentPercent] = useState<number>(0)
  const [progress, setProgress] = useState(0)
  const [runTotal, setRunTotal] = useState(0)
  const [result, setResult] = useState<OptimizePdfBatchResult>()
  const [error, setError] = useState<string>()
  const [cancelRequested, setCancelRequested] = useState(false)
  const jobIdRef = useRef<string>('')

  const changePreset = (next: PdfOptimizationPreset) => {
    setPreset(next)
    setToolPrefs('pdfOptimizer', { preset: next })
  }

  const addFiles = async (paths?: string[]) => {
    const selected = paths ?? (await desktop.pickFiles('pdf'))
    if (selected.length === 0) return
    const inspected = await desktop.inspectPdfsForOptimization(selected)
    if (inspected.length === 0) return
    setFiles((prev) => appendUniqueByPath(prev, inspected))
    setPhase('editing')
  }

  const reset = () => {
    setFiles([])
    setCompletedFiles(0)
    setTotalFiles(0)
    setCurrentFile(undefined)
    setCurrentPercent(0)
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
    setCurrentPercent(0)
    setProgress(0)
    setResult(undefined)
    setError(undefined)
    setCancelRequested(false)
    setPhase('editing')
  }

  const removeFile = (path: string) => {
    const next = files.filter((item) => item.path !== path)
    setFiles(next)
    if (next.length === 0) reset()
  }

  const optimize = async () => {
    const destination = exportPath || (await chooseExportPath())
    if (!destination) return
    const validPaths = files.filter((f) => f.status === 'ready').map((f) => f.path)
    if (validPaths.length === 0) {
      setError('No valid PDF files to optimize.')
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
    setCurrentFile(undefined)
    setCurrentPercent(0)
    setCancelRequested(false)

    const onProgress = ({
      completedFiles: completed,
      totalFiles: total,
      currentFile: active,
      currentFilePercent,
    }: PdfOptimizationProgress) => {
      setCompletedFiles(completed)
      setTotalFiles(total)
      setCurrentFile(active)
      setCurrentPercent(currentFilePercent ?? 0)
      const pct = total > 0 ? ((completed + (currentFilePercent ?? 0) / 100) / total) * 100 : 0
      setProgress(Math.round(pct))
    }

    try {
      const batchResult = await desktop.optimizePdfs(
        {
          files: validPaths,
          outputDirectory: destination,
          preset,
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
      setCurrentPercent(0)
      setProgress(0)
      setCancelRequested(false)
      setError(getErrorMessage(e))
    }
  }

  const cancel = async () => {
    if (!jobIdRef.current || cancelRequested) return
    setCancelRequested(true)
    try {
      await desktop.cancelPdfOptimizationJob(jobIdRef.current)
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
          label="Drop PDFs here"
          hint="Supports PDF files up to 100 MB and 500 pages. Drop a folder to add every PDF directly inside it."
        />
      </div>
    )
  }

  if (phase === 'done' && result) {
    const canceled = result.status === 'canceled'
    const optimizedItems = result.items.filter((i) => i.outcome === 'optimized')
    const alreadyItems = result.items.filter((i) => i.outcome === 'alreadyOptimized')
    const failedItems = result.items.filter((i) => i.outcome === 'failed')
    const succeeded = result.optimized
    const isAllFailed = !canceled && succeeded === 0 && alreadyItems.length === 0 && failedItems.length > 0
    const isPartial = !canceled && !isAllFailed && (failedItems.length > 0 || alreadyItems.length > 0)

    const status = canceled ? 'warning' : isAllFailed ? 'error' : isPartial ? 'warning' : 'success'
    const headline = canceled
      ? 'Batch canceled'
      : isAllFailed
        ? 'No files optimized'
        : `${succeeded} ${succeeded === 1 ? 'file' : 'files'} optimized${alreadyItems.length > 0 ? `, ${alreadyItems.length} already well optimized` : ''}`

    const before = optimizedItems.reduce((sum, i) => sum + i.originalSize, 0)
    const after = optimizedItems.reduce((sum, i) => sum + (i.outputSize ?? 0), 0)
    const hasOutput = optimizedItems.length > 0
    const finishedBeforeCancel = result.items.filter((i) => i.outcome !== 'canceled').length

    // Convert to FileResult-like for Completion's failures/warnings display
    const failuresForDisplay: FileResult[] = failedItems.map((i) => ({
      sourcePath: i.sourcePath,
      outputPath: undefined,
      success: false,
      originalSize: i.originalSize,
      error: i.error,
      warnings: undefined,
    }))
    return (
      <div className="mx-auto max-w-[760px]">
        <Completion
          status={status}
          headline={headline}
          metrics={
            hasOutput ? (
              <div className="space-y-2">
                <BeforeAfter before={before} after={after} />
                <p className="font-mono text-[13px] text-muted-foreground">
                  Saved to <span className="text-foreground">{exportPath}</span>
                </p>
              </div>
            ) : !isAllFailed && alreadyItems.length > 0 ? (
              <p className="font-mono text-[13px] text-muted-foreground">
                No smaller file could be produced. Originals were kept.
              </p>
            ) : null
          }
          warnings={[]}
          failures={failuresForDisplay}
          actions={
            canceled ? (
              <>
                {hasOutput && (
                  <Button
                    variant="secondary"
                    icon={<FolderIcon size={15} />}
                    onClick={() => void desktop.openFolder(exportPath)}
                  >
                    Open folder
                  </Button>
                )}
                <Button variant="primary" icon={<RotateCcwIcon size={15} />} onClick={reset}>
                  Optimize more PDFs
                </Button>
              </>
            ) : isAllFailed ? (
              <Button variant="primary" icon={<RotateCcwIcon size={15} />} onClick={resetToEditing}>
                Try again
              </Button>
            ) : (
              <>
                {hasOutput && (
                  <Button
                    variant="secondary"
                    icon={<FolderIcon size={15} />}
                    onClick={() => void desktop.openFolder(exportPath)}
                  >
                    Open folder
                  </Button>
                )}
                <Button
                  variant={isPartial || canceled ? 'secondary' : 'primary'}
                  icon={<RotateCcwIcon size={15} />}
                  onClick={reset}
                >
                  Optimize more PDFs
                </Button>
              </>
            )
          }
        >
          {optimizedItems.length > 0 && (
            <div className="mt-5 rounded-[var(--radius)] border border-border bg-muted/30 px-4 py-3.5">
              <div className="text-[13px] font-medium">Optimized files</div>
              <ul className="mt-2.5 space-y-2">
                {optimizedItems.map((item) => {
                  const saved = item.originalSize > 0 && item.outputSize ? Math.round((1 - item.outputSize / item.originalSize) * 100) : 0
                  return (
                    <li key={item.sourcePath} className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-[13px]">
                      <span className="font-mono font-medium text-foreground break-all">
                        {item.sourcePath.split(/[/\\]/).pop()}
                      </span>
                      <span className="font-mono text-muted-foreground tabular-nums">
                        {formatBytes(item.originalSize)} → {formatBytes(item.outputSize ?? 0)}
                      </span>
                      {saved > 0 && (
                        <span className="ml-1 rounded-full bg-success-surface px-2 py-0.5 text-[11px] font-semibold text-success">
                          {saved}% smaller
                        </span>
                      )}
                    </li>
                  )
                })}
              </ul>
            </div>
          )}

          {alreadyItems.length > 0 && (
            <div className="mt-5 rounded-[var(--radius)] border border-border bg-muted/30 px-4 py-3.5">
              <div className="text-[13px] font-medium">Already well optimized</div>
              <p className="mt-1 text-[12.5px] text-muted-foreground">
                These files were already well optimized. No smaller copy was created.
              </p>
              <ul className="mt-2.5 space-y-1.5">
                {alreadyItems.map((item) => (
                  <li key={item.sourcePath} className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-[13px]">
                    <span className="font-mono font-medium text-foreground break-all">
                      {item.sourcePath.split(/[/\\]/).pop()}
                    </span>
                    <span className="font-mono text-muted-foreground tabular-nums">
                      {formatBytes(item.originalSize)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {canceled && (
            <p className="mt-4 text-[13px] text-muted-foreground">
              {finishedBeforeCancel} of {runTotal} files finished before cancellation. Partial outputs were discarded; completed files were kept.
            </p>
          )}
        </Completion>
      </div>
    )
  }

  const readyPaths = files.filter((f) => f.status === 'ready').map((f) => f.path)
  const readyIndex = new Map(readyPaths.map((p, i) => [p, i]))

  const statuses = (file: InputFile): RowStatus => {
    if (file.status === 'invalid') return 'failed'
    if (phase !== 'processing') {
      const item = result?.items.find((entry) => entry.sourcePath === file.path)
      if (!item) return 'idle'
      return item.outcome === 'optimized' ? 'done' : item.outcome === 'alreadyOptimized' ? 'done' : 'failed'
    }
    const idx = readyIndex.get(file.path)
    if (idx !== undefined && idx < completedFiles) return 'done'
    if (currentFile === file.path) return 'processing'
    return 'idle'
  }

  const skipped = files.filter((f) => f.status === 'invalid').length
  const readyCount = readyPaths.length

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_340px] lg:items-start">
      <section>
        {phase !== 'processing' ? (
          <FileListHeader count={files.length} onAddMore={addFiles} onClear={reset} />
        ) : (
          <BatchProgress
            completedFiles={completedFiles}
            totalFiles={totalFiles}
            progress={progress}
            currentFile={currentFile}
            currentFilePercent={currentPercent}
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
            {skipped} unsupported {skipped === 1 ? 'file' : 'files'} excluded from optimization
          </p>
        )}

        <div className="rounded-[var(--radius)] border border-border bg-card p-1.5 space-y-0.5 max-h-[440px] overflow-y-auto">
          {files.map((file) => (
            <FileRow
              key={file.path}
              file={file}
              status={statuses(file)}
              error={file.status === 'invalid' ? file.error : undefined}
              onRemove={phase === 'processing' ? undefined : () => removeFile(file.path)}
            />
          ))}
        </div>
      </section>

      <aside className="lg:sticky lg:top-20 space-y-5">
        <div className="rounded-[var(--radius-lg)] border border-border-strong bg-card p-4 space-y-4">
          <div>
            <span className="block text-[13px] font-medium mb-2.5">Optimization strength</span>
            <Segmented
              value={preset}
              onChange={changePreset}
              options={[
                { value: 'lossless', label: 'Lossless' },
                { value: 'balanced', label: 'Balanced' },
                { value: 'smaller', label: 'Smaller File' },
              ]}
            />
            <p className="mt-2.5 text-[12.5px] text-muted-foreground leading-relaxed">
              {PRESET_COPY[preset]}
            </p>
          </div>
        </div>

        <ExportLocation path={exportPath} onChange={chooseExportPath} />

        <Button variant="primary" className="w-full h-11" disabled={phase === 'processing' || readyCount === 0} onClick={optimize}>
          {phase === 'processing' ? 'Optimizing…' : `Optimize ${readyCount} ${readyCount === 1 ? 'file' : 'files'}`}
        </Button>
      </aside>
    </div>
  )
}

export const pdfOptimizerDefinition: ToolDefinition = {
  id: 'optimize-pdf',
  name: 'Optimize PDFs',
  description: 'Reduce PDF file size while preserving quality.',
  icon: DocumentIcon,
  component: PdfOptimizer,
}
