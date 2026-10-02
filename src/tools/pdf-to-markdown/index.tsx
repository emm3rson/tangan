import { useState } from 'react'
import { Button, ProgressBar } from '@/components/ui'
import { DocumentIcon, FolderIcon, RotateCcwIcon } from '@/components/ui/icons'
import { Completion } from '@/components/processing/Completion'
import {
  DropZone,
  ExportLocation,
  FileListHeader,
  FileRow,
  SectionLabel,
  type RowStatus,
} from '@/components/workspace'
import {
  desktop,
  type BatchResult,
  type InputFile,
} from '@/services/tauri'
import { useSettings } from '@/app/providers/SettingsProvider'
import type { ToolDefinition } from '../types'
import { appendUniqueByPath, getErrorMessage } from '../shared/file-utils'

type Phase = 'empty' | 'editing' | 'processing' | 'done'

export function PdfToMarkdown() {
  const { exportPath, chooseExportPath } = useSettings()
  const [phase, setPhase] = useState<Phase>('empty')
  const [files, setFiles] = useState<InputFile[]>([])
  const [progress, setProgress] = useState(0)
  const [currentFile, setCurrentFile] = useState<string>()
  const [result, setResult] = useState<BatchResult>()
  const [error, setError] = useState<string>()

  const addFiles = async (paths?: string[]) => {
    const selected = paths ?? (await desktop.pickFiles('pdf'))
    if (selected.length === 0) return
    const inspected = await desktop.inspectPdfs(selected)
    if (inspected.length === 0) return
    setFiles((prev) => appendUniqueByPath(prev, inspected))
    setPhase('editing')
  }

  const reset = () => {
    setFiles([])
    setProgress(0)
    setCurrentFile(undefined)
    setResult(undefined)
    setError(undefined)
    setPhase('empty')
  }

  const removeFile = (path: string) => {
    const next = files.filter((item) => item.path !== path)
    setFiles(next)
    if (next.length === 0) {
      reset()
    }
  }

  const convert = async () => {
    const destination = exportPath || (await chooseExportPath())
    if (!destination) return
    const validPaths = files
      .filter((file) => file.status === 'ready')
      .map((file) => file.path)
    if (validPaths.length === 0) {
      setError('No valid PDF files to convert.')
      return
    }
    setError(undefined)
    setPhase('processing')
    setProgress(0)

    const onProgress = ({
      completed,
      total,
      currentFile: active,
    }: {
      completed: number
      total: number
      currentFile?: string
    }) => {
      setProgress(Math.round((completed / total) * 100))
      setCurrentFile(active)
    }

    try {
      const batchResult = await desktop.convertPdfs(
        {
          files: validPaths,
          outputDirectory: destination,
        },
        onProgress
      )
      setResult(batchResult)
      setPhase('done')
    } catch (e) {
      setPhase('editing')
      setError(getErrorMessage(e))
    }
  }

  if (phase === 'empty') {
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <DropZone
          onAdd={addFiles}
          label="Drop PDFs here"
          hint="Supports PDF files up to 100 MB. Drop a folder to add every PDF directly inside it."
        />
      </div>
    )
  }

  if (phase === 'done' && result) {
    const failures = result.items.filter((item) => !item.success)
    const warnings = result.items.filter(
      (item) => item.success && item.warnings && item.warnings.length > 0
    )
    const isAllFailed = result.succeeded === 0
    const isPartial =
      result.succeeded > 0 && (failures.length > 0 || warnings.length > 0)
    const status = isAllFailed ? 'error' : isPartial ? 'warning' : 'success'
    const headline = isAllFailed
      ? 'No files converted'
      : `${result.succeeded} ${result.succeeded === 1 ? 'file' : 'files'} converted to Markdown`

    return (
      <div className="mx-auto max-w-[760px]">
        <Completion
          status={status}
          headline={headline}
          metrics={
            !isAllFailed ? (
              <p className="font-mono text-[13px] text-muted-foreground">
                Saved to <span className="text-foreground">{exportPath}</span>
              </p>
            ) : null
          }
          warnings={warnings}
          failures={failures}
          actions={
            isAllFailed ? (
              <Button
                variant="primary"
                icon={<RotateCcwIcon size={15} />}
                onClick={reset}
              >
                Try again
              </Button>
            ) : (
              <>
                <Button
                  variant="primary"
                  icon={<FolderIcon size={16} />}
                  onClick={() => desktop.openFolder(exportPath)}
                >
                  Open folder
                </Button>
                <Button icon={<RotateCcwIcon size={15} />} onClick={reset}>
                  Convert more
                </Button>
              </>
            )
          }
        />
      </div>
    )
  }

  const statuses = (file: InputFile): RowStatus =>
    file.status === 'invalid'
      ? 'failed'
      : phase !== 'processing'
        ? 'idle'
        : result?.items.find((item) => item.sourcePath === file.path)?.success
          ? 'done'
          : currentFile === file.path
            ? 'processing'
            : 'idle'

  const skipped = files.filter((file) => file.status === 'invalid').length
  const readyCount = files.filter((file) => file.status === 'ready').length
  const doneCount = Math.round((progress / 100) * readyCount)

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
          <div className="mb-3.5">
            <div className="flex items-center justify-between mb-2">
              <span className="text-[13.5px] font-medium">
                Converting {Math.min(doneCount + 1, readyCount)} of{' '}
                {readyCount}
              </span>
              <span className="font-mono text-[12px] text-muted-foreground">
                {progress}%
              </span>
            </div>
            <ProgressBar value={progress} />
          </div>
        )}

        {error && (
          <div className="mb-3.5 rounded-[var(--radius)] border border-danger/25 bg-danger-surface/70 px-4 py-3 text-[13px] text-danger">
            {error}
          </div>
        )}

        {phase !== 'processing' && skipped > 0 && (
          <p className="mb-3.5 text-[12.5px] text-muted-foreground">
            {skipped} unsupported {skipped === 1 ? 'file' : 'files'} excluded from
            conversion
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
        <div className="rounded-[var(--radius-lg)] border border-border-strong bg-card p-4 space-y-2">
          <SectionLabel>Local extraction</SectionLabel>
          <p className="text-[12.5px] text-muted-foreground leading-relaxed">
            Native text is extracted directly on your machine. Scanned pages
            or images requiring OCR will be visibly marked in the generated
            Markdown.
          </p>
        </div>

        <ExportLocation path={exportPath} onChange={chooseExportPath} />

        <Button
          variant="primary"
          className="w-full h-11"
          disabled={phase === 'processing' || readyCount === 0}
          onClick={convert}
        >
          {phase === 'processing'
            ? 'Converting…'
            : `Convert ${readyCount} ${readyCount === 1 ? 'file' : 'files'}`}
        </Button>
      </aside>
    </div>
  )
}

export const pdfToMarkdownDefinition: ToolDefinition = {
  id: 'pdf-markdown',
  name: 'Convert PDF to Markdown',
  description: 'Turn local PDFs into clean, editable Markdown.',
  icon: DocumentIcon,
  component: PdfToMarkdown,
}
