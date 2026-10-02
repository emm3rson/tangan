import { useEffect, useRef, useState } from 'react'
import { Button, Segmented, cx } from '@/components/ui'
import {
  AlertIcon,
  ArrowDown,
  ArrowUp,
  CheckIcon,
  ChevronDown,
  CopyIcon,
  FolderIcon,
  ImageIcon,
  PaletteIcon,
  PlusIcon,
  SpinnerIcon,
  XIcon,
} from '@/components/ui/icons'
import {
  DropZone,
  ExportLocation,
  SectionLabel,
  formatBytes,
} from '@/components/workspace'
import {
  desktop,
  type ExtractColorPaletteResult,
  type InputFile,
  type PaletteColor,
  type PaletteExportFormat,
  type RgbColor,
} from '@/services/tauri'
import { useSettings } from '@/app/providers/SettingsProvider'
import type { ToolDefinition } from '../types'
import { getErrorMessage } from '../shared/file-utils'
import { clamp, hexToRgb, hslString, hslToRgb, rgbString, rgbToHex, rgbToHsl } from './colors'
import {
  clientToNormalized,
  computeContentRect,
  sampleNeighborhood,
  type ImageBuffer,
} from './sampling'
import { colorsEqual, usePaletteSession } from './usePaletteSession'

const MIN_COLORS = 3
const MAX_COLORS = 12
const DEFAULT_COLORS = 6

type Phase = 'empty' | 'invalid' | 'extracting' | 'editing' | 'error'
type InvalidReason = 'multiple' | 'unsupported'

function getContrastColor(rgb: { r: number; g: number; b: number }): '#000000' | '#FFFFFF' {
  const yiq = (rgb.r * 299 + rgb.g * 587 + rgb.b * 114) / 1000
  return yiq >= 145 ? '#000000' : '#FFFFFF'
}

export function ColorPaletteExtractor() {
  const { exportPath, chooseExportPath } = useSettings()
  const [phase, setPhase] = useState<Phase>('empty')
  const [source, setSource] = useState<InputFile>()
  const [result, setResult] = useState<ExtractColorPaletteResult>()
  const [error, setError] = useState<string>()
  const [invalidReason, setInvalidReason] = useState<InvalidReason>('unsupported')
  const [reExtracting, setReExtracting] = useState(false)
  const [exporting, setExporting] = useState(false)
  const [exportFormat, setExportFormat] = useState<PaletteExportFormat>('png')
  const [exportedPath, setExportedPath] = useState<string>()
  const [copied, setCopied] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [selected, setSelected] = useState(0)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [box, setBox] = useState({ w: 0, h: 0 })
  const [hoverLoupe, setHoverLoupe] = useState<{
    x: number
    y: number
    hex: string
    rgb: { r: number; g: number; b: number }
  } | null>(null)

  // Draft states for live multi-format editing
  const [hexDraft, setHexDraft] = useState('')
  const [hexError, setHexError] = useState(false)
  const [rDraft, setRDraft] = useState('')
  const [gDraft, setGDraft] = useState('')
  const [bDraft, setBDraft] = useState('')
  const [hDraft, setHDraft] = useState('')
  const [sDraft, setSDraft] = useState('')
  const [lDraft, setLDraft] = useState('')

  const [session, dispatch] = usePaletteSession()
  const colors = session.present

  const previewRef = useRef<HTMLDivElement>(null)
  const bufferRef = useRef<ImageBuffer | null>(null)
  const dragStartRef = useRef<PaletteColor[]>([])
  const draggingRef = useRef<number | null>(null)
  const lastRequestedCountRef = useRef(DEFAULT_COLORS)

  const selectedIndex = colors.length === 0 ? 0 : Math.min(selected, colors.length - 1)
  const selectedColor = colors[selectedIndex]

  // Decode the bounded preview into an offscreen canvas once per extraction.
  useEffect(() => {
    if (!result) return
    let cancelled = false
    bufferRef.current = null
    const img = new Image()
    img.onload = () => {
      if (cancelled) return
      try {
        const canvas = document.createElement('canvas')
        canvas.width = img.naturalWidth
        canvas.height = img.naturalHeight
        const ctx = canvas.getContext('2d')
        if (!ctx) return
        ctx.drawImage(img, 0, 0)
        bufferRef.current = {
          data: ctx.getImageData(0, 0, canvas.width, canvas.height).data,
          width: canvas.width,
          height: canvas.height,
        }
      } catch {
        bufferRef.current = null
      }
    }
    img.onerror = () => {
      bufferRef.current = null
    }
    img.src = `data:image/png;base64,${result.previewBase64}`
    return () => {
      cancelled = true
    }
  }, [result])

  useEffect(() => {
    const el = previewRef.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0].contentRect
      setBox({ w: rect.width, h: rect.height })
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [phase])

  useEffect(() => {
    if (!adding) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setAdding(false)
        setHoverLoupe(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [adding])

  // Sync draft states when selectedColor changes
  useEffect(() => {
    if (!selectedColor) {
      setHexDraft('')
      setHexError(false)
      setRDraft('')
      setGDraft('')
      setBDraft('')
      setHDraft('')
      setSDraft('')
      setLDraft('')
      return
    }
    const hex = rgbToHex(selectedColor)
    const { h, s, l } = rgbToHsl(selectedColor)
    setHexDraft(hex)
    setHexError(false)
    setRDraft(String(Math.round(selectedColor.r)))
    setGDraft(String(Math.round(selectedColor.g)))
    setBDraft(String(Math.round(selectedColor.b)))
    setHDraft(String(h))
    setSDraft(String(s))
    setLDraft(String(l))
  }, [selectedColor])

  useEffect(() => {
    setExportedPath(undefined)
    setError(undefined)
  }, [colors])

  const runExtract = async (file: InputFile, count: number) => {
    lastRequestedCountRef.current = count
    setError(undefined)
    setPhase('extracting')
    try {
      const next = await desktop.extractColorPalette({
        sourcePath: file.path,
        targetCount: count,
      })
      setResult(next)
      dispatch({ type: 'replace', colors: next.colors })
      setSelected(0)
      setAdding(false)
      setHoverLoupe(null)
      setExportedPath(undefined)
      setPhase('editing')
    } catch (e) {
      setError(getErrorMessage(e))
      setPhase('error')
    }
  }

  const load = async (paths?: string[], count?: number) => {
    const target = count ?? DEFAULT_COLORS
    const selectedFiles = paths ?? (await desktop.pickFiles('palette'))
    if (selectedFiles.length === 0) return
    if (selectedFiles.length > 1) {
      setSource(undefined)
      setResult(undefined)
      setInvalidReason('multiple')
      setPhase('invalid')
      return
    }
    const [file] = await desktop.inspectFiles(selectedFiles)
    setSource(file)
    if (file.status === 'invalid') {
      setResult(undefined)
      setInvalidReason('unsupported')
      setPhase('invalid')
      return
    }
    await runExtract(file, target)
  }

  const resetAll = () => {
    setPhase('empty')
    setSource(undefined)
    setResult(undefined)
    setError(undefined)
    setReExtracting(false)
    setExporting(false)
    setExportFormat('png')
    setExportedPath(undefined)
    setAdding(false)
    setHoverLoupe(null)
    setSelected(0)
    setHoveredIndex(null)
    dispatch({ type: 'replace', colors: [] })
  }

  const replaceImage = () => {
    setPhase('empty')
    setSource(undefined)
    setResult(undefined)
    setError(undefined)
    setExportFormat('png')
    setExportedPath(undefined)
    setAdding(false)
    setHoverLoupe(null)
    setSelected(0)
    setHoveredIndex(null)
    dispatch({ type: 'replace', colors: [] })
    void load(undefined, DEFAULT_COLORS)
  }

  const runReExtract = async (count: number) => {
    if (!source) return
    const requested = Math.max(MIN_COLORS, Math.min(MAX_COLORS, count))
    lastRequestedCountRef.current = requested
    setReExtracting(true)
    setError(undefined)
    try {
      const next = await desktop.extractColorPalette({
        sourcePath: source.path,
        targetCount: requested,
      })
      setResult(next)
      dispatch({ type: 'replace', colors: next.colors })
      setSelected(0)
      setAdding(false)
      setHoverLoupe(null)
      setExportedPath(undefined)
    } catch (e) {
      setError(getErrorMessage(e))
    } finally {
      setReExtracting(false)
    }
  }

  const samplePoint = (clientX: number, clientY: number) => {
    const el = previewRef.current
    if (!el || !result) return null
    const rect = el.getBoundingClientRect()
    const content = computeContentRect(
      rect.width,
      rect.height,
      result.previewWidth,
      result.previewHeight
    )
    const { nx, ny } = clientToNormalized(rect.left, rect.top, content, clientX, clientY)
    const sampled = bufferRef.current
      ? sampleNeighborhood(bufferRef.current, nx, ny)
      : null
    return { nx, ny, sampled, rect, content }
  }

  const beginDrag = (index: number, event: React.PointerEvent) => {
    event.stopPropagation()
    dragStartRef.current = colors
    draggingRef.current = index
    setSelected(index)
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  }

  const dragMove = (event: React.PointerEvent) => {
    const index = draggingRef.current
    if (index == null) return
    const point = samplePoint(event.clientX, event.clientY)
    if (!point) return
    const current = colors[index]
    const nextRgb = point.sampled ?? current
    dispatch({
      type: 'move',
      index,
      color: {
        r: nextRgb.r,
        g: nextRgb.g,
        b: nextRgb.b,
        x: point.nx,
        y: point.ny,
      },
    })
    const elRect = point.rect
    setHoverLoupe({
      x: event.clientX - elRect.left,
      y: event.clientY - elRect.top,
      hex: rgbToHex(nextRgb),
      rgb: nextRgb,
    })
  }

  const endDrag = () => {
    const index = draggingRef.current
    if (index == null) return
    draggingRef.current = null
    setHoverLoupe(null)
    dispatch({ type: 'commitMove', before: dragStartRef.current })
  }

  const onPointerMovePreview = (event: React.PointerEvent) => {
    if (!adding) return
    const point = samplePoint(event.clientX, event.clientY)
    if (!point || !point.sampled) {
      setHoverLoupe(null)
      return
    }
    const elRect = point.rect
    setHoverLoupe({
      x: event.clientX - elRect.left,
      y: event.clientY - elRect.top,
      hex: rgbToHex(point.sampled),
      rgb: point.sampled,
    })
  }

  const onPreviewClick = (event: React.MouseEvent) => {
    if (!adding) return
    const point = samplePoint(event.clientX, event.clientY)
    if (!point || !point.sampled) return
    dispatch({
      type: 'add',
      color: { r: point.sampled.r, g: point.sampled.g, b: point.sampled.b, x: point.nx, y: point.ny },
    })
    setSelected(colors.length)
    setAdding(false)
    setHoverLoupe(null)
  }

  // Live HEX handler
  const handleHexChange = (value: string) => {
    setHexDraft(value)
    setHexError(false)
    const parsed = hexToRgb(value)
    if (parsed && selectedColor) {
      const { h, s, l } = rgbToHsl(parsed)
      setRDraft(String(parsed.r))
      setGDraft(String(parsed.g))
      setBDraft(String(parsed.b))
      setHDraft(String(h))
      setSDraft(String(s))
      setLDraft(String(l))
      dispatch({
        type: 'edit',
        index: selectedIndex,
        color: { ...selectedColor, r: parsed.r, g: parsed.g, b: parsed.b },
      })
    }
  }

  const commitHex = () => {
    if (!selectedColor) return
    const parsed = hexToRgb(hexDraft)
    if (!parsed) {
      setHexError(true)
      return
    }
    const normalized = rgbToHex(parsed)
    setHexDraft(normalized)
    setHexError(false)
    const { h, s, l } = rgbToHsl(parsed)
    setRDraft(String(parsed.r))
    setGDraft(String(parsed.g))
    setBDraft(String(parsed.b))
    setHDraft(String(h))
    setSDraft(String(s))
    setLDraft(String(l))
    dispatch({
      type: 'edit',
      index: selectedIndex,
      color: { ...selectedColor, r: parsed.r, g: parsed.g, b: parsed.b },
    })
  }

  // Live RGB handler
  const handleRgbChange = (channel: 'r' | 'g' | 'b', value: string) => {
    if (!selectedColor) return
    const rVal = channel === 'r' ? value : rDraft
    const gVal = channel === 'g' ? value : gDraft
    const bVal = channel === 'b' ? value : bDraft

    if (channel === 'r') setRDraft(value)
    if (channel === 'g') setGDraft(value)
    if (channel === 'b') setBDraft(value)

    const rn = parseInt(rVal, 10)
    const gn = parseInt(gVal, 10)
    const bn = parseInt(bVal, 10)

    if (!isNaN(rn) && !isNaN(gn) && !isNaN(bn)) {
      const cr = clamp(rn, 0, 255)
      const cg = clamp(gn, 0, 255)
      const cb = clamp(bn, 0, 255)
      const rgb = { r: cr, g: cg, b: cb }
      const hex = rgbToHex(rgb)
      const { h, s, l } = rgbToHsl(rgb)
      setHexDraft(hex)
      setHexError(false)
      setHDraft(String(h))
      setSDraft(String(s))
      setLDraft(String(l))
      dispatch({
        type: 'edit',
        index: selectedIndex,
        color: { ...selectedColor, r: cr, g: cg, b: cb },
      })
    }
  }

  const commitRgb = () => {
    if (!selectedColor) return
    setRDraft(String(Math.round(selectedColor.r)))
    setGDraft(String(Math.round(selectedColor.g)))
    setBDraft(String(Math.round(selectedColor.b)))
  }

  // Live HSL handler
  const handleHslChange = (channel: 'h' | 's' | 'l', value: string) => {
    if (!selectedColor) return
    const hVal = channel === 'h' ? value : hDraft
    const sVal = channel === 's' ? value : sDraft
    const lVal = channel === 'l' ? value : lDraft

    if (channel === 'h') setHDraft(value)
    if (channel === 's') setSDraft(value)
    if (channel === 'l') setLDraft(value)

    const hn = parseInt(hVal, 10)
    const sn = parseInt(sVal, 10)
    const ln = parseInt(lVal, 10)

    if (!isNaN(hn) && !isNaN(sn) && !isNaN(ln)) {
      const ch = clamp(hn, 0, 360)
      const cs = clamp(sn, 0, 100)
      const cl = clamp(ln, 0, 100)
      const rgb = hslToRgb({ h: ch, s: cs, l: cl })
      const hex = rgbToHex(rgb)
      setHexDraft(hex)
      setHexError(false)
      setRDraft(String(rgb.r))
      setGDraft(String(rgb.g))
      setBDraft(String(rgb.b))
      dispatch({
        type: 'edit',
        index: selectedIndex,
        color: { ...selectedColor, ...rgb },
      })
    }
  }

  const commitHsl = () => {
    if (!selectedColor) return
    const { h, s, l } = rgbToHsl(selectedColor)
    setHDraft(String(h))
    setSDraft(String(s))
    setLDraft(String(l))
  }

  const copyText = async (text: string, key: string) => {
    try {
      await navigator.clipboard?.writeText(text)
    } catch {
      // clipboard may be unavailable outside the app shell
    }
    setCopied(key)
    setTimeout(() => setCopied((current) => (current === key ? null : current)), 1500)
  }

  const doExport = async () => {
    if (!source || exporting) return
    const destination = exportPath || (await chooseExportPath())
    if (!destination) return
    setExporting(true)
    setError(undefined)
    try {
      const rgbColors: RgbColor[] = colors.map((color) => ({
        r: Math.round(color.r),
        g: Math.round(color.g),
        b: Math.round(color.b),
      }))
      const exported = await desktop.exportColorPalette({
        sourcePath: source.path,
        colors: rgbColors,
        outputDirectory: destination,
        format: exportFormat,
      })
      setExportedPath(exported.outputPath)
    } catch (e) {
      setError(getErrorMessage(e))
    } finally {
      setExporting(false)
    }
  }

  const openExportedFolder = () => {
    if (!exportedPath) return
    const index = Math.max(exportedPath.lastIndexOf('\\'), exportedPath.lastIndexOf('/'))
    const dir = index >= 0 ? exportedPath.slice(0, index) : exportedPath
    void desktop.openFolder(dir)
  }

  if (phase === 'empty') {
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <DropZone
          onAdd={load}
          label="Drop an image here"
          hint="Supports PNG, JPG, WebP, SVG · Local offline extraction"
        />
      </div>
    )
  }

  if (phase === 'invalid') {
    const isMultiple = invalidReason === 'multiple'
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <div className="rounded-[var(--radius-lg)] border border-danger/30 bg-danger-surface/50 p-5">
          <div className="flex items-start gap-3.5">
            <div className="grid h-8 w-8 shrink-0 place-items-center rounded-[var(--radius-sm)] border border-danger/30 bg-elevated text-danger">
              <ImageIcon size={18} />
            </div>
            <div className="flex-1">
              <div className="flex items-center gap-2 text-danger">
                <AlertIcon size={16} />
                <span className="text-[14px] font-semibold">
                  {isMultiple ? 'One image at a time' : 'This file is not a supported image'}
                </span>
              </div>
              <p className="mt-1.5 text-[13px] leading-relaxed text-muted-foreground">
                {isMultiple
                  ? 'Color Palette Extractor works with a single image. Drop or choose exactly one PNG, JPG, WebP or SVG.'
                  : null}
                {!isMultiple && source ? (
                  <>
                    <span className="font-mono text-foreground">{source.name}</span> is not a PNG,
                    JPG, WebP or SVG image{source.error ? `: ${source.error}` : '.'}
                  </>
                ) : null}
              </p>
            </div>
          </div>
        </div>
        <div className="mt-4 flex justify-center gap-2.5">
          <Button variant="primary" onClick={() => load()}>
            Choose an image
          </Button>
          <Button variant="ghost" onClick={resetAll}>
            Cancel
          </Button>
        </div>
      </div>
    )
  }

  if (phase === 'extracting') {
    return (
      <div
        className="mx-auto max-w-[560px] pt-12 text-center"
        role="status"
        aria-live="polite"
        aria-busy="true"
      >
        <SpinnerIcon size={28} className="mx-auto animate-spin text-muted-foreground" />
        <p className="mt-4 text-[14px] font-medium text-foreground">Extracting colors…</p>
        <p className="mt-1 text-[12px] text-muted-foreground">Analyzing color distribution locally</p>
      </div>
    )
  }

  if (phase === 'error') {
    return (
      <div className="mx-auto max-w-[560px] pt-6">
        <div className="rounded-[var(--radius-lg)] border border-danger/30 bg-danger-surface/50 p-5">
          <div className="flex items-center gap-2 text-danger">
            <AlertIcon size={16} />
            <span className="text-[14px] font-semibold">Could not extract a palette</span>
          </div>
          <p className="mt-1.5 text-[13px] leading-relaxed text-muted-foreground">{error}</p>
        </div>
        <div className="mt-4 flex justify-center gap-2.5">
          <Button
            variant="primary"
            onClick={() => source && runExtract(source, lastRequestedCountRef.current)}
          >
            Try again
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              setPhase('empty')
              setSource(undefined)
              setResult(undefined)
              setError(undefined)
            }}
          >
            Choose another image
          </Button>
        </div>
      </div>
    )
  }

  const previewUrl = result ? `data:image/png;base64,${result.previewBase64}` : undefined
  const contentRect = result
    ? computeContentRect(box.w, box.h, result.previewWidth, result.previewHeight)
    : { left: 0, top: 0, width: 0, height: 0 }
  const canUndo = session.past.length > 0
  const canRedo = session.future.length > 0
  const canReset = !colorsEqual(colors, session.baseline)

  const paletteHex = colors.map(rgbToHex)

  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_340px] lg:items-start">
      <section className="space-y-5">
        {/* Main Connected Canvas & Ribbon Card */}
        <div className="overflow-hidden rounded-[var(--radius-lg)] border border-border-strong bg-card shadow-sm">
          {/* Header Bar */}
          <div className="flex items-center gap-3 border-b border-border px-4 py-3 bg-muted/10">
            <div className="grid h-9 w-9 shrink-0 place-items-center overflow-hidden rounded-[var(--radius-sm)] border border-border bg-transparency-grid">
              {previewUrl ? (
                <img src={previewUrl} alt="" className="h-full w-full object-cover" />
              ) : (
                <ImageIcon size={17} className="text-muted-foreground" />
              )}
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-[13.5px] font-medium leading-tight">{source?.name}</p>
              <p className="mt-0.5 font-mono text-[11.5px] tabular-nums text-muted-foreground">
                {result && result.width > 0 && result.height > 0 ? (
                  <>
                    {result.width.toLocaleString()} × {result.height.toLocaleString()}
                    <span className="mx-1.5 text-border-strong">·</span>
                  </>
                ) : null}
                {source ? formatBytes(source.size) : null}
              </p>
            </div>
            <Button size="sm" variant="ghost" onClick={replaceImage}>
              Replace image
            </Button>
          </div>

          {/* Interactive Preview Canvas */}
          <div className="p-3.5">
            <div
              ref={previewRef}
              onClick={onPreviewClick}
              onPointerMove={onPointerMovePreview}
              onPointerLeave={() => {
                if (adding) setHoverLoupe(null)
              }}
              className={cx(
                'relative h-[360px] w-full select-none overflow-hidden rounded-[var(--radius)] border border-border bg-transparency-grid transition-colors',
                adding && !reExtracting && 'cursor-crosshair',
                reExtracting && 'pointer-events-none opacity-70'
              )}
            >
              {previewUrl && (
                <img
                  src={previewUrl}
                  alt={source?.name}
                  draggable={false}
                  className="h-full w-full object-contain"
                />
              )}

              {/* Sample Handles on Canvas */}
              {colors.map((color, index) => {
                const isSelected = selectedIndex === index
                const isHovered = hoveredIndex === index
                const contrast = getContrastColor(color)
                const pinLeft = contentRect.left + color.x * contentRect.width
                const pinTop = contentRect.top + color.y * contentRect.height

                return (
                  <button
                    key={index}
                    aria-label={`Sample handle ${index + 1}: ${rgbToHex(color)}`}
                    aria-valuetext={rgbToHex(color)}
                    onPointerDown={(event) => beginDrag(index, event)}
                    onPointerMove={dragMove}
                    onPointerUp={endDrag}
                    onPointerCancel={endDrag}
                    onLostPointerCapture={endDrag}
                    onMouseEnter={() => setHoveredIndex(index)}
                    onMouseLeave={() => setHoveredIndex(null)}
                    onClick={(event) => {
                      event.stopPropagation()
                      setSelected(index)
                    }}
                    className={cx(
                      'absolute z-10 flex h-[22px] w-[22px] -translate-x-1/2 -translate-y-1/2 items-center justify-center touch-none rounded-full border-2 border-white shadow-[0_2px_6px_rgba(0,0,0,0.35)] transition-transform duration-100',
                      (isSelected || isHovered) &&
                        'z-20 scale-125 ring-2 ring-foreground/60 shadow-[0_0_0_2px_rgba(255,255,255,0.9),0_4px_10px_rgba(0,0,0,0.4)]'
                    )}
                    style={{
                      left: pinLeft,
                      top: pinTop,
                      backgroundColor: rgbString(color),
                      color: contrast,
                    }}
                  >
                    <span className="font-mono text-[9.5px] font-bold leading-none select-none">
                      {index + 1}
                    </span>
                  </button>
                )
              })}

              {/* Live Loupe / Tooltip (during Drag or Pick) */}
              {hoverLoupe && (
                <div
                  className="pointer-events-none absolute z-30 flex items-center gap-1.5 rounded-full border border-border/80 bg-popover/95 px-2.5 py-1 text-[11px] font-mono shadow-md backdrop-blur-sm -translate-x-1/2 -translate-y-[135%]"
                  style={{
                    left: hoverLoupe.x,
                    top: hoverLoupe.y,
                  }}
                >
                  <span
                    className="h-3 w-3 rounded-full border border-black/20"
                    style={{ backgroundColor: hoverLoupe.hex }}
                  />
                  <span className="font-semibold text-popover-foreground">{hoverLoupe.hex}</span>
                </div>
              )}

              {/* Eyedropper / Pick Active Banner */}
              {adding && (
                <div className="pointer-events-none absolute inset-x-0 top-3 flex justify-center">
                  <span className="rounded-full bg-foreground/90 px-3.5 py-1 text-[11.5px] font-medium text-background shadow-md">
                    Click image to pick color · Esc to cancel
                  </span>
                </div>
              )}
            </div>
          </div>

          {/* Palette Controls Toolbar */}
          <div className="border-t border-border bg-muted/20 px-4 py-3 flex flex-wrap items-center justify-between gap-3">
            {/* Left: Palette Title and Clean Count Dropdown */}
            <div className="flex items-center gap-2.5">
              <span className="text-[13px] font-semibold text-foreground">Palette</span>
              <div className="relative inline-flex items-center">
                <select
                  value={colors.length}
                  disabled={reExtracting || !source}
                  onChange={(e) => runReExtract(Number(e.target.value))}
                  aria-label="Palette size"
                  className="h-7 cursor-pointer appearance-none rounded-[var(--radius-sm)] border border-border bg-elevated pl-2.5 pr-6 font-mono text-[12px] font-medium text-foreground outline-none transition-colors hover:border-border-strong hover:bg-muted focus:ring-2 focus:ring-ring/45 disabled:pointer-events-none disabled:opacity-40"
                >
                  {Array.from({ length: MAX_COLORS - MIN_COLORS + 1 }, (_, i) => i + MIN_COLORS).map((num) => (
                    <option key={num} value={num} className="bg-popover text-popover-foreground">
                      {num} {num === 1 ? 'color' : 'colors'}
                    </option>
                  ))}
                </select>
                <ChevronDown size={12} className="pointer-events-none absolute right-1.5 text-muted-foreground" />
              </div>
              {reExtracting && (
                <span className="flex items-center gap-1.5 font-mono text-[11px] text-muted-foreground">
                  <SpinnerIcon size={12} className="animate-spin" />
                  Extracting…
                </span>
              )}
            </div>

            {/* Right: Custom Operations (Add Color, History) */}
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant={adding ? 'primary' : 'secondary'}
                disabled={colors.length >= MAX_COLORS || reExtracting}
                icon={<PlusIcon size={13} />}
                onClick={() => {
                  setAdding(!adding)
                  if (adding) setHoverLoupe(null)
                }}
              >
                {adding ? 'Cancel' : 'Add color'}
              </Button>

              <div className="flex items-center gap-0.5 border-l border-border pl-2">
                <button
                  onClick={() => dispatch({ type: 'undo' })}
                  disabled={!canUndo}
                  className="rounded-[var(--radius-sm)] px-2 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
                >
                  Undo
                </button>
                <button
                  onClick={() => dispatch({ type: 'redo' })}
                  disabled={!canRedo}
                  className="rounded-[var(--radius-sm)] px-2 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
                >
                  Redo
                </button>
                <button
                  onClick={() => dispatch({ type: 'reset' })}
                  disabled={!canReset}
                  className="rounded-[var(--radius-sm)] px-2 py-1 text-[12px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
                >
                  Reset
                </button>
              </div>
            </div>
          </div>

          {reExtracting && (
            <div className="border-t border-border/60 bg-muted/10 px-4 py-2 flex items-center gap-2 text-[12px] text-muted-foreground">
              <SpinnerIcon size={13} className="animate-spin text-muted-foreground" />
              Re-extracting automatic palette…
            </div>
          )}
          {result?.notice && !reExtracting && (
            <div className="border-t border-border/60 bg-muted/10 px-4 py-2 text-[12px] text-muted-foreground">
              {result.notice}
            </div>
          )}

          {/* Seamless Connected Palette Ribbon (Uniform Grid) */}
          <div className="border-t border-border p-3">
            <div
              className="grid gap-2"
              style={{
                gridTemplateColumns: `repeat(${Math.min(colors.length, 6)}, minmax(0, 1fr))`,
              }}
            >
              {colors.map((color, index) => {
                const isSelected = selectedIndex === index
                const isHovered = hoveredIndex === index
                const hex = rgbToHex(color)

                return (
                  <button
                    key={index}
                    onClick={() => setSelected(index)}
                    onMouseEnter={() => setHoveredIndex(index)}
                    onMouseLeave={() => setHoveredIndex(null)}
                    aria-label={`Select color ${index + 1}`}
                    className={cx(
                      'group relative flex min-w-0 flex-col overflow-hidden rounded-[var(--radius-sm)] border bg-elevated text-left transition-all duration-150',
                      isSelected
                        ? 'border-foreground/50 ring-2 ring-foreground/20 shadow-sm'
                        : isHovered
                          ? 'border-border-strong -translate-y-0.5'
                          : 'border-border hover:border-border-strong'
                    )}
                  >
                    <span
                      className="h-10 w-full border-b border-black/10 transition-transform group-hover:scale-[1.02]"
                      style={{ backgroundColor: rgbString(color) }}
                    />
                    <div className="flex items-center justify-between px-2 py-1.5">
                      <span className="font-mono text-[10px] font-medium text-muted-foreground">
                        {String(index + 1).padStart(2, '0')}
                      </span>
                      <span className="font-mono text-[11.5px] font-semibold tracking-tight">
                        {hex}
                      </span>
                    </div>
                  </button>
                )
              })}
            </div>
          </div>
        </div>
      </section>

      {/* Right Column: 340px Inspector & Export Hub */}
      <aside className="space-y-4">
        {error && (
          <div className="rounded-[var(--radius)] border border-danger/25 bg-danger-surface/70 px-4 py-3 text-[13px] text-danger">
            {error}
          </div>
        )}

        {/* Card 1: Selected Color Inspector */}
        {selectedColor && (
          <div className="rounded-[var(--radius-lg)] border border-border-strong bg-card p-4 shadow-sm">
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-[11px] font-semibold uppercase tracking-[.09em] text-subtle-foreground">
                Selected color
              </h2>
              <span className="font-mono text-[10.5px] font-medium text-muted-foreground">
                {String(selectedIndex + 1).padStart(2, '0')} / {String(colors.length).padStart(2, '0')}
              </span>
            </div>

            {/* Full-width Rectangular Color Swatch */}
            <div
              className="h-10 w-full rounded-[var(--radius-sm)] border border-black/10 shadow-sm transition-colors"
              style={{ backgroundColor: rgbString(selectedColor) }}
            />

            {/* Aligned Multi-Format Channel Rows (HEX, RGB, HSL) */}
            <div className="mt-3.5 space-y-2">
              {/* HEX Row */}
              <div className="flex items-center gap-2">
                <span className="w-9 text-[11px] font-medium uppercase tracking-[.07em] text-subtle-foreground">
                  HEX
                </span>
                <div className="flex flex-1 items-center gap-1.5">
                  <input
                    id="hex-input"
                    value={hexDraft}
                    onChange={(event) => handleHexChange(event.target.value)}
                    onBlur={commitHex}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') (event.currentTarget as HTMLInputElement).blur()
                    }}
                    aria-label="HEX color"
                    aria-invalid={hexError}
                    aria-describedby={hexError ? 'hex-error' : undefined}
                    className={cx(
                      'h-7 min-w-0 flex-1 rounded-[5px] border bg-elevated px-2 font-mono text-[12px] font-medium uppercase outline-none transition-colors focus:ring-2 focus:ring-ring/45',
                      hexError ? 'border-danger' : 'border-border-strong'
                    )}
                  />
                  <button
                    onClick={() => copyText(rgbToHex(selectedColor), 'hex')}
                    aria-label="Copy HEX"
                    className="inline-flex h-7 items-center gap-1 rounded-[5px] px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  >
                    {copied === 'hex' ? (
                      <CheckIcon size={13} className="text-success" />
                    ) : (
                      <CopyIcon size={13} />
                    )}
                    {copied === 'hex' ? 'Copied' : ''}
                  </button>
                </div>
              </div>
              {hexError && (
                <p id="hex-error" className="pl-11 text-[11px] text-danger" role="alert">
                  Enter a 6-digit HEX like #RRGGBB
                </p>
              )}

              {/* RGB Channel Row */}
              <div className="flex items-center gap-2">
                <span className="w-9 text-[11px] font-medium uppercase tracking-[.07em] text-subtle-foreground">
                  RGB
                </span>
                <div className="flex flex-1 items-center gap-1.5">
                  <div className="flex flex-1 items-center gap-1">
                    <input
                      type="number"
                      min={0}
                      max={255}
                      value={rDraft}
                      onChange={(e) => handleRgbChange('r', e.target.value)}
                      onBlur={commitRgb}
                      aria-label="Red channel"
                      placeholder="R"
                      className="h-7 w-full min-w-0 rounded-[5px] border border-border-strong bg-elevated px-1 text-center font-mono text-[12px] font-medium outline-none transition-colors focus:ring-2 focus:ring-ring/45 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    />
                    <input
                      type="number"
                      min={0}
                      max={255}
                      value={gDraft}
                      onChange={(e) => handleRgbChange('g', e.target.value)}
                      onBlur={commitRgb}
                      aria-label="Green channel"
                      placeholder="G"
                      className="h-7 w-full min-w-0 rounded-[5px] border border-border-strong bg-elevated px-1 text-center font-mono text-[12px] font-medium outline-none transition-colors focus:ring-2 focus:ring-ring/45 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    />
                    <input
                      type="number"
                      min={0}
                      max={255}
                      value={bDraft}
                      onChange={(e) => handleRgbChange('b', e.target.value)}
                      onBlur={commitRgb}
                      aria-label="Blue channel"
                      placeholder="B"
                      className="h-7 w-full min-w-0 rounded-[5px] border border-border-strong bg-elevated px-1 text-center font-mono text-[12px] font-medium outline-none transition-colors focus:ring-2 focus:ring-ring/45 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    />
                  </div>
                  <button
                    onClick={() => copyText(rgbString(selectedColor), 'rgb')}
                    aria-label="Copy RGB"
                    className="inline-flex h-7 items-center gap-1 rounded-[5px] px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  >
                    {copied === 'rgb' ? (
                      <CheckIcon size={13} className="text-success" />
                    ) : (
                      <CopyIcon size={13} />
                    )}
                    {copied === 'rgb' ? 'Copied' : ''}
                  </button>
                </div>
              </div>

              {/* HSL Channel Row */}
              <div className="flex items-center gap-2">
                <span className="w-9 text-[11px] font-medium uppercase tracking-[.07em] text-subtle-foreground">
                  HSL
                </span>
                <div className="flex flex-1 items-center gap-1.5">
                  <div className="flex flex-1 items-center gap-1">
                    <input
                      type="number"
                      min={0}
                      max={360}
                      value={hDraft}
                      onChange={(e) => handleHslChange('h', e.target.value)}
                      onBlur={commitHsl}
                      aria-label="Hue channel"
                      placeholder="H°"
                      className="h-7 w-full min-w-0 rounded-[5px] border border-border-strong bg-elevated px-1 text-center font-mono text-[12px] font-medium outline-none transition-colors focus:ring-2 focus:ring-ring/45 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    />
                    <input
                      type="number"
                      min={0}
                      max={100}
                      value={sDraft}
                      onChange={(e) => handleHslChange('s', e.target.value)}
                      onBlur={commitHsl}
                      aria-label="Saturation channel"
                      placeholder="S%"
                      className="h-7 w-full min-w-0 rounded-[5px] border border-border-strong bg-elevated px-1 text-center font-mono text-[12px] font-medium outline-none transition-colors focus:ring-2 focus:ring-ring/45 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    />
                    <input
                      type="number"
                      min={0}
                      max={100}
                      value={lDraft}
                      onChange={(e) => handleHslChange('l', e.target.value)}
                      onBlur={commitHsl}
                      aria-label="Lightness channel"
                      placeholder="L%"
                      className="h-7 w-full min-w-0 rounded-[5px] border border-border-strong bg-elevated px-1 text-center font-mono text-[12px] font-medium outline-none transition-colors focus:ring-2 focus:ring-ring/45 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                    />
                  </div>
                  <button
                    onClick={() => copyText(hslString(selectedColor), 'hsl')}
                    aria-label="Copy HSL"
                    className="inline-flex h-7 items-center gap-1 rounded-[5px] px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  >
                    {copied === 'hsl' ? (
                      <CheckIcon size={13} className="text-success" />
                    ) : (
                      <CopyIcon size={13} />
                    )}
                    {copied === 'hsl' ? 'Copied' : ''}
                  </button>
                </div>
              </div>
            </div>

            {/* Reorder and Remove Actions */}
            <div className="mt-3.5 flex items-center gap-1.5 border-t border-border pt-3">
              <button
                aria-label="Move color up"
                disabled={selectedIndex === 0}
                onClick={() => {
                  if (selectedIndex > 0) {
                    dispatch({ type: 'moveUp', index: selectedIndex })
                    setSelected(selectedIndex - 1)
                  }
                }}
                className="grid h-8 w-8 place-items-center rounded-[var(--radius-sm)] border border-border bg-elevated text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
              >
                <ArrowUp size={15} />
              </button>
              <button
                aria-label="Move color down"
                disabled={selectedIndex >= colors.length - 1}
                onClick={() => {
                  if (selectedIndex < colors.length - 1) {
                    dispatch({ type: 'moveDown', index: selectedIndex })
                    setSelected(selectedIndex + 1)
                  }
                }}
                className="grid h-8 w-8 place-items-center rounded-[var(--radius-sm)] border border-border bg-elevated text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
              >
                <ArrowDown size={15} />
              </button>
              <button
                aria-label="Remove color"
                disabled={colors.length <= MIN_COLORS}
                onClick={() => {
                  dispatch({ type: 'remove', index: selectedIndex })
                  setSelected(Math.max(0, selectedIndex - 1))
                }}
                className="ml-auto inline-flex h-8 items-center gap-1.5 rounded-[var(--radius-sm)] px-2.5 text-[12px] text-muted-foreground transition-colors hover:bg-danger-surface hover:text-danger disabled:pointer-events-none disabled:opacity-40"
              >
                <XIcon size={14} />
                Remove
              </button>
            </div>
          </div>
        )}

        {/* Card 2: Export Hub */}
        <div className="rounded-[var(--radius-lg)] border border-border-strong bg-card p-4 shadow-sm">
          <SectionLabel>Copy palette</SectionLabel>
          <div className="grid grid-cols-3 gap-2">
            <Button
              size="sm"
              variant="secondary"
              className="w-full"
              onClick={() => copyText(paletteHex.join('\n'), 'all-hex')}
            >
              {copied === 'all-hex' ? 'Copied' : 'HEX'}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="w-full"
              onClick={() =>
                copyText(
                  colors.map((color, index) => `--color-${index + 1}: ${rgbToHex(color)};`).join('\n'),
                  'all-css'
                )
              }
            >
              {copied === 'all-css' ? 'Copied' : 'CSS'}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              className="w-full"
              onClick={() => copyText(JSON.stringify(paletteHex), 'all-json')}
            >
              {copied === 'all-json' ? 'Copied' : 'JSON'}
            </Button>
          </div>

          <div className="mt-4 border-t border-border pt-4">
            <SectionLabel>Export</SectionLabel>
            <div className="space-y-3">
              <ExportLocation path={exportPath} onChange={chooseExportPath} />
              <div className="flex items-center justify-between gap-2">
                <span className="text-[12px] font-medium text-muted-foreground">Format</span>
                <Segmented
                  value={exportFormat}
                  onChange={(format) => {
                    setExportFormat(format)
                    setExportedPath(undefined)
                  }}
                  options={[
                    { value: 'png', label: 'PNG' },
                    { value: 'jpg', label: 'JPG' },
                    { value: 'svg', label: 'SVG' },
                  ]}
                />
              </div>
              <Button
                variant="primary"
                className="h-10 w-full font-medium"
                disabled={exporting || reExtracting || colors.length === 0}
                onClick={doExport}
              >
                {exporting ? 'Exporting…' : `Export as ${exportFormat.toUpperCase()}`}
              </Button>
            </div>
          </div>

          {exportedPath && (
            <div className="mt-3 rounded-[var(--radius)] border border-success/30 bg-success-surface/60 px-3.5 py-3">
              <p className="flex items-center gap-2 text-[12.5px] font-medium text-foreground">
                <CheckIcon size={14} className="text-success" />
                Palette saved
              </p>
              <p className="mt-1 truncate font-mono text-[11.5px] text-muted-foreground">
                {exportedPath}
              </p>
              <Button
                size="sm"
                variant="ghost"
                icon={<FolderIcon size={14} />}
                className="mt-2"
                onClick={openExportedFolder}
              >
                Open folder
              </Button>
            </div>
          )}
        </div>
      </aside>
    </div>
  )
}

export const colorPaletteExtractorDefinition: ToolDefinition = {
  id: 'palette',
  name: 'Extract Color Palette',
  description: 'Extract and refine a reusable palette from any image.',
  icon: PaletteIcon,
  component: ColorPaletteExtractor,
}
