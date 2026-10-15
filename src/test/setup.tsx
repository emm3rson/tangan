import '@testing-library/jest-dom/vitest'
import { afterEach, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { ReactElement } from 'react'
import { SettingsProvider } from '@/app/providers/SettingsProvider'
import type { TauriAdapter } from '@/services/tauri'

afterEach(() => cleanup())

const { mockDesktop } = vi.hoisted(() => {
  const desktop: Record<keyof TauriAdapter, ReturnType<typeof vi.fn>> = {
    inspectFiles: vi.fn(),
    inspectPdfs: vi.fn(),
    inspectPdfsForOptimization: vi.fn(),
    inspectVideos: vi.fn(),
    convertImages: vi.fn(),
    compressImages: vi.fn(),
    previewTrace: vi.fn(),
    traceImages: vi.fn(),
    convertPdfs: vi.fn(),
    generateLogoPack: vi.fn(),
    processVideos: vi.fn(),
    cancelVideoJob: vi.fn(),
    optimizePdfs: vi.fn(),
    cancelPdfOptimizationJob: vi.fn(),
    getLogoPresets: vi.fn(),
    extractColorPalette: vi.fn(),
    exportColorPalette: vi.fn(),
    pickFiles: vi.fn(),
    pickFolder: vi.fn(),
    openFolder: vi.fn(),
    getVersion: vi.fn(),
  }
  return { mockDesktop: () => desktop }
})

vi.mock('@/services/tauri', () => ({ desktop: mockDesktop() }))
vi.mock('@/services/settings', () => ({
  loadSettings: vi.fn().mockResolvedValue({}),
  saveSettings: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path.replace(/\\/g, '/')}`,
}))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(() => Promise.resolve(() => {})),
  }),
}))

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

export const mockTauri = mockDesktop()

const emptyBatch = { total: 0, succeeded: 0, failed: 0, items: [] }

export function resetMockTauri(): void {
  mockTauri.inspectFiles.mockReset().mockResolvedValue([])
  mockTauri.inspectPdfs.mockReset().mockResolvedValue([])
  mockTauri.inspectPdfsForOptimization.mockReset().mockResolvedValue([])
  mockTauri.inspectVideos.mockReset().mockResolvedValue([])
  mockTauri.convertImages.mockReset().mockResolvedValue(emptyBatch)
  mockTauri.compressImages.mockReset().mockResolvedValue(emptyBatch)
  mockTauri.previewTrace.mockReset().mockResolvedValue({
    svg: '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><path d="M0 0h100v100H0z"/></svg>',
    width: 100,
    height: 100,
  })
  mockTauri.traceImages.mockReset().mockResolvedValue(emptyBatch)
  mockTauri.convertPdfs.mockReset().mockResolvedValue(emptyBatch)
  mockTauri.generateLogoPack.mockReset().mockResolvedValue({
    packDirectory: 'C:\\out\\web-pack',
    batch: emptyBatch,
  })
  mockTauri.processVideos.mockReset().mockResolvedValue({
    status: 'completed',
    total: 0,
    succeeded: 0,
    failed: 0,
    items: [],
  })
  mockTauri.cancelVideoJob.mockReset().mockResolvedValue(undefined)
  mockTauri.optimizePdfs.mockReset().mockResolvedValue({
    status: 'completed',
    total: 0,
    optimized: 0,
    alreadyOptimized: 0,
    failed: 0,
    items: [],
  })
  mockTauri.cancelPdfOptimizationJob.mockReset().mockResolvedValue(undefined)
  mockTauri.getLogoPresets.mockReset().mockResolvedValue([])
  mockTauri.extractColorPalette.mockReset().mockResolvedValue({
    width: 100,
    height: 100,
    previewWidth: 100,
    previewHeight: 100,
    previewBase64: '',
    colors: [],
  })
  mockTauri.exportColorPalette.mockReset().mockResolvedValue({
    outputPath: 'C:\\out\\photo-palette.png',
  })
  mockTauri.pickFiles.mockReset().mockResolvedValue([])
  mockTauri.pickFolder.mockReset().mockResolvedValue(null)
  mockTauri.openFolder.mockReset().mockResolvedValue(undefined)
  mockTauri.getVersion.mockReset().mockResolvedValue('0.3.1')
}


export function renderWithProviders(ui: ReactElement) {
  return render(<SettingsProvider>{ui}</SettingsProvider>)
}
