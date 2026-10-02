import { useEffect, useState, type ReactNode } from 'react'
import { Button, Checkbox, ProgressBar, cx } from '@/components/ui'
import {
  AlertIcon,
  ChevronDown,
  CopyIcon,
  FolderIcon,
  ImageIcon,
  LogoPackIcon,
  RotateCcwIcon,
} from '@/components/ui/icons'
import { Completion } from '@/components/processing/Completion'
import {
  DropZone,
  ExportLocation,
  SectionLabel,
  formatBytes,
  toAssetUrl,
} from '@/components/workspace'
import {
  desktop,
  type GenerateLogoPackResult,
  type InputFile,
  type LogoAssetDefinition,
} from '@/services/tauri'
import { useSettings } from '@/app/providers/SettingsProvider'
import type { ToolDefinition } from '../types'
import { getErrorMessage } from '../shared/file-utils'

const FALLBACK_PRESETS: LogoAssetDefinition[] = [
  {
    id: 'favicon-ico',
    filename: 'favicon.ico',
    width: 0,
    height: 0,
    format: 'ico',
    defaultEnabled: true,
  },
  {
    id: 'favicon-16',
    filename: 'favicon-16x16.png',
    width: 16,
    height: 16,
    format: 'png',
    defaultEnabled: true,
  },
  {
    id: 'favicon-32',
    filename: 'favicon-32x32.png',
    width: 32,
    height: 32,
    format: 'png',
    defaultEnabled: true,
  },
  {
    id: 'apple-touch',
    filename: 'apple-touch-icon.png',
    width: 180,
    height: 180,
    format: 'png',
    defaultEnabled: true,
  },
  {
    id: 'icon-192',
    filename: 'icon-192.png',
    width: 192,
    height: 192,
    format: 'png',
    defaultEnabled: true,
  },
  {
    id: 'icon-512',
    filename: 'icon-512.png',
    width: 512,
    height: 512,
    format: 'png',
    defaultEnabled: true,
  },
]

type AssetRow = { id: string; file: string; size: string; on: boolean }

const toAssetRow = (preset: LogoAssetDefinition): AssetRow => ({
  id: preset.id,
  file: preset.filename,
  size:
    preset.format === 'ico'
      ? 'multi-res'
      : `${preset.width} × ${preset.height}`,
  on: preset.defaultEnabled,
})

const snippet = `<link rel="icon" href="/favicon.ico" sizes="any" />
<link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png" />
<link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png" />
<link rel="apple-touch-icon" href="/apple-touch-icon.png" />
<link rel="manifest" href="/site.webmanifest" />`

type Phase = 'empty' | 'invalid' | 'valid' | 'processing' | 'done'
type InvalidReason = 'unsupported' | 'non-square' | 'too-small'

const invalidCopy: Record<
  InvalidReason,
  { title: string; body: (source: InputFile) => ReactNode; action: string }
> = {
  unsupported: {
    title: 'This file is not a supported image',
    body: (source) => (
      <>
        <span className="font-mono">{source.name}</span> is not a PNG, JPG,
        WebP or SVG image{source.error ? `: ${source.error}` : '.'}
      </>
    ),
    action: 'Choose an image',
  },
  'non-square': {
    title: 'Source must be square',
    body: (source) => (
      <>
        <span className="font-mono">{source.name}</span> is{' '}
        <span className="font-mono font-medium">
          {source.width} × {source.height}
        </span>
        . Cropping is not applied automatically in V1.
      </>
    ),
    action: 'Choose a square image',
  },
  'too-small': {
    title: 'Source is too small',
    body: (source) => (
      <>
        <span className="font-mono">{source.name}</span> is only{' '}
        <span className="font-mono font-medium">
          {source.width} × {source.height}
        </span>
        . Use a source at least 512 × 512 px.
      </>
    ),
    action: 'Choose a larger image',
  },
}

export function WebLogoPack() {
  const { exportPath, chooseExportPath, tool, setToolPrefs } = useSettings()
  const [phase, setPhase] = useState<Phase>('empty')
  const [source, setSource] = useState<InputFile>()
  const [previewError, setPreviewError] = useState(false)
  const [invalidReason, setInvalidReason] =
    useState<InvalidReason>('non-square')
  const [presets, setPresets] =
    useState<LogoAssetDefinition[]>(FALLBACK_PRESETS)
  const savedIds = new Set(tool.webLogoPack?.selectedAssets ?? [])

  const toAssetRows = (list: LogoAssetDefinition[]): AssetRow[] =>
    list.map((preset) => ({
      ...toAssetRow(preset),
      on:
        savedIds.size > 0
          ? savedIds.has(preset.id)
          : preset.defaultEnabled,
    }))

  const [assets, setAssets] = useState<AssetRow[]>(() =>
    toAssetRows(FALLBACK_PRESETS)
  )
  const [progress, setProgress] = useState(0)
  const [result, setResult] = useState<GenerateLogoPackResult>()
  const [error, setError] = useState<string>()
  const [snippetOpen, setSnippetOpen] = useState(false)
  const [copied, setCopied] = useState(false)

  const persistAssets = (next: AssetRow[]) => {
    setAssets(next)
    setToolPrefs('webLogoPack', {
      selectedAssets: next.filter((asset) => asset.on).map((asset) => asset.id),
    })
  }

  const selected = assets.filter((asset) => asset.on)

  useEffect(() => {
    desktop
      .getLogoPresets()
      .then((list) => {
        if (list.length > 0) {
          setPresets(list)
          setAssets(toAssetRows(list))
        }
      })
      .catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const load = async (paths?: string[]) => {
    const selectedFiles = paths ?? (await desktop.pickFiles('logo'))
    if (selectedFiles.length === 0) return
    const [file] = await desktop.inspectFiles(selectedFiles)
    setSource(file)
    setPreviewError(false)
    const isSvg = file.extension.toLowerCase() === 'svg'
    if (file.status === 'invalid') {
      setInvalidReason('unsupported')
      setPhase('invalid')
      return
    }
    if (!isSvg && file.width !== file.height) {
      setInvalidReason('non-square')
      setPhase('invalid')
      return
    }
    if (!isSvg && file.width < 512) {
      setInvalidReason('too-small')
      setPhase('invalid')
      return
    }
    setError(undefined)
    setPhase('valid')
  }

  const reset = () => {
    setPhase('empty')
    setSource(undefined)
    setPreviewError(false)
    setAssets(toAssetRows(presets))
    setProgress(0)
    setResult(undefined)
    setError(undefined)
  }

  const generate = async () => {
    if (!source) return
    const destination = exportPath || (await chooseExportPath())
    if (!destination) return
    setError(undefined)
    setPhase('processing')
    setProgress(0)

    const onProgress = ({
      completed,
      total,
    }: {
      completed: number
      total: number
    }) => setProgress(Math.round((completed / total) * 100))

    try {
      const next = await desktop.generateLogoPack(
        {
          sourcePath: source.path,
          outputDirectory: destination,
          assetIds: selected.map((asset) => asset.id),
        },
        onProgress
      )
      setResult(next)
      setPhase('done')
    } catch (e) {
      setPhase('valid')
      setError(getErrorMessage(e))
    }
  }

  const previewUrl =
    source && !previewError ? toAssetUrl(source.path) : undefined

  if (phase === 'empty') {
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <DropZone
          onAdd={load}
          hint={
            <>
              Use a high-resolution{' '}
              <span className="text-foreground font-medium">square</span> logo or
              icon (at least 512 × 512 px for raster, or vector SVG). Transparent PNG or SVG works best.
            </>
          }
        />
      </div>
    )
  }

  if (phase === 'invalid' && source) {
    const copy = invalidCopy[invalidReason]
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <div className="rounded-[var(--radius-lg)] border border-danger/30 bg-danger-surface/50 p-5">
          <div className="flex items-start gap-3.5">
            <div className="grid w-[84px] h-7 place-items-center rounded-[var(--radius-sm)] border border-danger/30 bg-elevated text-muted-foreground">
              <ImageIcon size={18} />
            </div>
            <div className="flex-1">
              <div className="flex items-center gap-2 text-danger">
                <AlertIcon size={16} />
                <span className="text-[14px] font-semibold">{copy.title}</span>
              </div>
              <p className="mt-1.5 text-[13.5px] leading-relaxed">
                {copy.body(source)}
              </p>
            </div>
          </div>
        </div>
        <div className="mt-4 flex justify-center gap-2.5">
          <Button variant="primary" onClick={() => load()}>
            {copy.action}
          </Button>
          <Button variant="ghost" onClick={reset}>
            Cancel
          </Button>
        </div>
      </div>
    )
  }

  if (phase === 'done' && result) {
    const isAllFailed = result.batch.succeeded === 0
    const isPartial = result.batch.succeeded > 0 && result.batch.failed > 0
    const status = isAllFailed ? 'error' : isPartial ? 'warning' : 'success'
    const headline = isAllFailed
      ? 'No assets generated'
      : `${result.batch.succeeded} assets generated`

    return (
      <div className="mx-auto max-w-[760px]">
        <Completion
          status={status}
          headline={headline}
          metrics={
            !isAllFailed ? (
              <p className="font-mono text-[13px] text-muted-foreground">
                Saved to{' '}
                <span className="text-foreground font-medium">
                  {result.packDirectory}
                </span>
              </p>
            ) : null
          }
          failures={result.batch.items.filter((item) => !item.success)}
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
                  onClick={() => desktop.openFolder(result.packDirectory)}
                >
                  Open folder
                </Button>
                <Button
                  icon={<CopyIcon size={15} />}
                  onClick={async () => {
                    await navigator.clipboard?.writeText(snippet)
                    setCopied(true)
                    setTimeout(() => setCopied(false), 1600)
                  }}
                >
                  {copied ? 'Copied' : 'Copy snippet'}
                </Button>
                <Button
                  variant="ghost"
                  icon={<RotateCcwIcon size={15} />}
                  onClick={reset}
                >
                  Process another
                </Button>
              </>
            )
          }
        >
          {!isAllFailed && <div className="mt-6 rounded-[var(--radius)] border border-border bg-card/60 overflow-hidden">
            <button
              onClick={() => setSnippetOpen(!snippetOpen)}
              className="flex w-full items-center justify-between px-3.5 py-3 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <span className="text-[13px] font-medium">
                Integration snippet
              </span>
              <ChevronDown
                size={16}
                className={cx(
                  'text-muted-foreground transition-transform duration-200',
                  snippetOpen && 'rotate-180'
                )}
              />
            </button>
            <div
              className={cx(
                'grid transition-[grid-template-rows] duration-200',
                snippetOpen ? 'grid-rows-[1fr]' : 'grid-rows-[0fr]'
              )}
            >
              <div className="overflow-hidden">
                <pre className="px-3.5 pb-4 pt-3 border-t border-border font-mono text-[12px] leading-relaxed text-muted-foreground overflow-x-auto">
                  <code>{snippet}</code>
                </pre>
              </div>
            </div>
          </div>}
        </Completion>
      </div>
    )
  }

  if (!source) return null
  const processing = phase === 'processing'

  return (
    <div className="mx-auto grid max-w-[860px] gap-6 md:grid-cols-[240px_minmax(0,1fr)] md:items-start">
      <aside>
        <SectionLabel>Source</SectionLabel>
        <div className="rounded-[var(--radius-lg)] border border-border-strong bg-card p-4">
          <div className="aspect-square w-full rounded-[var(--radius-sm)] overflow-hidden bg-transparency-grid border border-border flex items-center justify-center p-3 mb-3.5">
            {previewUrl ? (
              <img
                src={previewUrl}
                alt={source.name}
                className="max-h-full max-w-full object-contain rounded-[4px] shadow-sm select-none"
                onError={() => setPreviewError(true)}
              />
            ) : (
              <div className="text-center text-muted-foreground">
                <ImageIcon size={28} className="mx-auto mb-1 opacity-70" />
                <span className="font-mono text-[10.5px]">preview unavailable</span>
              </div>
            )}
          </div>

          <div>
            <p className="truncate text-[13.5px] font-medium text-foreground">
              {source.name}
            </p>
            <p className="font-mono text-[11.5px] text-muted-foreground mt-0.5 tabular-nums">
              {source.width > 0 && source.height > 0 ? (
                <>
                  {source.width.toLocaleString()} × {source.height.toLocaleString()}
                  <span className="text-border-strong mx-1.5">·</span>
                </>
              ) : null}
              {formatBytes(source.size)}
            </p>
          </div>

          {!processing && (
            <div className="mt-3 pt-3 border-t border-border">
              <Button
                size="sm"
                variant="ghost"
                className="w-full justify-center text-[12px]"
                onClick={() => load()}
              >
                Replace image
              </Button>
            </div>
          )}
        </div>
      </aside>

      <section>
        {processing ? (
          <div className="mb-3.5">
            <div className="flex items-center justify-between mb-2">
              <span className="text-[13.5px] font-medium">
                Generating assets…
              </span>
              <span className="font-mono text-[12px] text-muted-foreground">
                {progress}%
              </span>
            </div>
            <ProgressBar value={progress} />
          </div>
        ) : (
          <SectionLabel
            hint={
              <button
                onClick={() =>
                  persistAssets(
                    assets.map((asset) => ({
                      ...asset,
                      on: selected.length < assets.length,
                    }))
                  )
                }
                className={cx(
                  'rounded-[var(--radius-sm)] px-2 py-1 text-[12px] font-medium transition-colors',
                  selected.length < assets.length
                    ? 'text-muted-foreground hover:bg-muted hover:text-foreground'
                    : 'text-muted-foreground hover:text-danger hover:bg-danger-surface'
                )}
              >
                {selected.length < assets.length ? 'Select all' : 'Clear all'}
              </button>
            }
          >
            Standard Web Pack
          </SectionLabel>
        )}

        {error && !processing && (
          <div className="mb-3.5 rounded-[var(--radius)] border border-danger/25 bg-danger-surface/70 px-4 py-3 text-[13px] text-danger">
            {error}
          </div>
        )}

        <div className="rounded-[var(--radius)] border border-border bg-card divide-y divide-border/70">
          {assets.map((asset, index) => (
            <label
              key={asset.id}
              className={cx(
                'flex items-center gap-3 px-3.5 py-2.5 transition-colors cursor-pointer',
                !processing && 'hover:bg-muted/50',
                !asset.on && 'opacity-55'
              )}
            >
              <Checkbox
                checked={asset.on}
                onChange={(on) =>
                  persistAssets(
                    assets.map((item, i) => (i === index ? { ...item, on } : item))
                  )
                }
              />
              <span className="flex-1 font-mono text-[13px]">{asset.file}</span>
              <span className="font-mono text-[11.5px] text-muted-foreground">
                {asset.size}
              </span>
            </label>
          ))}
        </div>

        <div className="mt-5 space-y-4">
          <ExportLocation path={exportPath} onChange={chooseExportPath} />
          <Button
            variant="primary"
            className="w-full h-11"
            disabled={processing || selected.length === 0}
            onClick={generate}
          >
            {processing
              ? 'Generating…'
              : `Generate ${selected.length} ${selected.length === 1 ? 'asset' : 'assets'}`}
          </Button>
        </div>
      </section>
    </div>
  )
}

export const webLogoPackDefinition: ToolDefinition = {
  id: 'logo',
  name: 'Generate Logo Pack',
  description: 'Create favicons and app icons from a single logo.',
  icon: LogoPackIcon,
  component: WebLogoPack,
}
