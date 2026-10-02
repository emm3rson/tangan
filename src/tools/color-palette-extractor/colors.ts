import type { RgbColor } from '@/services/tauri/contracts'

export type Rgb = RgbColor

export const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value))

const toHexByte = (value: number) =>
  clamp(Math.round(value), 0, 255).toString(16).padStart(2, '0').toUpperCase()

export function rgbToHex({ r, g, b }: Rgb): string {
  return `#${toHexByte(r)}${toHexByte(g)}${toHexByte(b)}`
}

export function hexToRgb(hex: string): Rgb | null {
  const match = /^#?([0-9a-fA-F]{6})$/.exec(hex.trim())
  if (!match) return null
  const value = parseInt(match[1], 16)
  return { r: (value >> 16) & 255, g: (value >> 8) & 255, b: value & 255 }
}

export function rgbToHsl({ r, g, b }: Rgb): { h: number; s: number; l: number } {
  const rn = r / 255
  const gn = g / 255
  const bn = b / 255
  const max = Math.max(rn, gn, bn)
  const min = Math.min(rn, gn, bn)
  const l = (max + min) / 2
  let h = 0
  let s = 0
  if (max !== min) {
    const d = max - min
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
    switch (max) {
      case rn:
        h = (gn - bn) / d + (gn < bn ? 6 : 0)
        break
      case gn:
        h = (bn - rn) / d + 2
        break
      default:
        h = (rn - gn) / d + 4
    }
    h /= 6
  }
  return { h: Math.round(h * 360), s: Math.round(s * 100), l: Math.round(l * 100) }
}

export function hslToRgb({ h, s, l }: { h: number; s: number; l: number }): Rgb {
  const hn = (((h % 360) + 360) % 360) / 360
  const sn = clamp(s, 0, 100) / 100
  const ln = clamp(l, 0, 100) / 100

  if (sn === 0) {
    const val = Math.round(ln * 255)
    return { r: val, g: val, b: val }
  }

  const hue2rgb = (p: number, q: number, t: number) => {
    let tn = t
    if (tn < 0) tn += 1
    if (tn > 1) tn -= 1
    if (tn < 1 / 6) return p + (q - p) * 6 * tn
    if (tn < 1 / 2) return q
    if (tn < 2 / 3) return p + (q - p) * (2 / 3 - tn) * 6
    return p
  }

  const q = ln < 0.5 ? ln * (1 + sn) : ln + sn - ln * sn
  const p = 2 * ln - q
  const r = Math.round(hue2rgb(p, q, hn + 1 / 3) * 255)
  const g = Math.round(hue2rgb(p, q, hn) * 255)
  const b = Math.round(hue2rgb(p, q, hn - 1 / 3) * 255)

  return { r: clamp(r, 0, 255), g: clamp(g, 0, 255), b: clamp(b, 0, 255) }
}

export function rgbString({ r, g, b }: Rgb): string {
  return `rgb(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)})`
}

export function hslString(rgb: Rgb): string {
  const { h, s, l } = rgbToHsl(rgb)
  return `hsl(${h}, ${s}%, ${l}%)`
}
