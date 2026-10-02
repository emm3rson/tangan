import { Button, ProgressBar } from '../ui'
import { XIcon } from '../ui/icons'

export function BatchProgress({
  completedFiles,
  totalFiles,
  progress,
  cancelRequested,
  onCancel,
  currentFile,
  currentFilePercent
}: {
  completedFiles: number
  totalFiles: number
  progress: number
  cancelRequested: boolean
  onCancel: () => void
  currentFile?: string
  currentFilePercent?: number
}) {
  return (
    <div className="mb-3.5" role="status" aria-live="polite" aria-busy="true">
      <div className="flex items-center justify-between mb-2">
        <span className="text-[13.5px] font-medium">
          Processing {Math.min(completedFiles + 1, totalFiles)} of {totalFiles}
        </span>
        <span className="font-mono text-[12px] text-muted-foreground">
          {progress}%
        </span>
      </div>
      <ProgressBar value={progress} />
      {currentFile && (
        <p className="mt-2 font-mono text-[11.5px] text-muted-foreground truncate">
          {currentFile} · {Math.round(currentFilePercent ?? 0)}%
        </p>
      )}
      <div className="mt-2.5 flex justify-end">
        <Button
          variant="ghost"
          size="sm"
          icon={<XIcon size={14} />}
          disabled={cancelRequested}
          onClick={onCancel}
        >
          {cancelRequested ? 'Canceling…' : 'Cancel'}
        </Button>
      </div>
    </div>
  )
}
