import { beforeEach, describe, expect, it } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { mockTauri, renderWithProviders, resetMockTauri } from '@/test/setup'
import type { InputFile, OptimizePdfBatchResult } from '@/services/tauri'
import { PdfOptimizer, pdfOptimizerDefinition } from './index'

const validPdf = (path: string, name: string): InputFile => ({
  path,
  name,
  extension: 'pdf',
  size: 2_048_000,
  width: 0,
  height: 0,
  status: 'ready',
})

const invalidPdf: InputFile = {
  path: 'C:\\bad.pdf',
  name: 'bad.pdf',
  extension: 'pdf',
  size: 0,
  width: 0,
  height: 0,
  status: 'invalid',
  error: 'Not a valid PDF file',
}

const optimizedItem = (path: string, originalSize = 2_048_000, outputSize = 1_024_000) => ({
  sourcePath: path,
  outcome: 'optimized' as const,
  originalSize,
  outputSize,
  outputPath: 'C:\\out\\' + path.split(/[/\\]/).pop()!.replace('.pdf', '-optimized.pdf'),
})

const alreadyItem = (path: string) => ({
  sourcePath: path,
  outcome: 'alreadyOptimized' as const,
  originalSize: 1_000_000,
})

const failedItem = (path: string, message = 'This PDF could not be opened') => ({
  sourcePath: path,
  outcome: 'failed' as const,
  originalSize: 500_000,
  error: { code: 'INVALID_PDF' as const, message },
})

const completedBatch = (items: OptimizePdfBatchResult['items']): OptimizePdfBatchResult => ({
  status: 'completed',
  total: items.length,
  optimized: items.filter((i) => i.outcome === 'optimized').length,
  alreadyOptimized: items.filter((i) => i.outcome === 'alreadyOptimized').length,
  failed: items.filter((i) => i.outcome === 'failed').length,
  items,
})

const canceledBatch = (items: OptimizePdfBatchResult['items']): OptimizePdfBatchResult => ({
  status: 'canceled',
  total: 2,
  optimized: items.filter((i) => i.outcome === 'optimized').length,
  alreadyOptimized: items.filter((i) => i.outcome === 'alreadyOptimized').length,
  failed: items.filter((i) => i.outcome === 'failed').length,
  items,
})

beforeEach(() => {
  resetMockTauri()
  localStorage.clear()
  localStorage.setItem('toolbox.settings', JSON.stringify({ exportPath: 'C:\\out' }))
})

describe('pdf optimizer registration', () => {
  it('exposes correct launcher copy', () => {
    expect(pdfOptimizerDefinition.name).toBe('Optimize PDFs')
    expect(pdfOptimizerDefinition.description).toBe('Reduce PDF file size while preserving quality.')
    expect(pdfOptimizerDefinition.id).toBe('optimize-pdf')
  })
})

describe('PdfOptimizer', () => {
  it('shows drop zone with PDF hints when empty', () => {
    renderWithProviders(<PdfOptimizer />)
    expect(screen.getByText('Drop PDFs here')).toBeInTheDocument()
    expect(screen.getByText(/Supports PDF files up to 100 MB/i)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /optimize/i })).not.toBeInTheDocument()
  })

  it('shows invalid feedback and disables optimize when all invalid', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\bad.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([invalidPdf])
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    expect(await screen.findByText('bad.pdf')).toBeInTheDocument()
    expect(screen.getByText(/Not a valid PDF file/)).toBeInTheDocument()
    expect(screen.getByText('1 unsupported file excluded from optimization')).toBeInTheDocument()
    const btn = screen.getByRole('button', { name: /optimize 0 files/i })
    expect(btn).toBeDisabled()
  })

  it('optimizes with default balanced preset and generated jobId', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([validPdf('C:\\a.pdf', 'a.pdf')])
    mockTauri.optimizePdfs.mockResolvedValue(completedBatch([optimizedItem('C:\\a.pdf')]))
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 1 file/i }))
    await screen.findByText(/1 file optimized/i)
    expect(mockTauri.optimizePdfs).toHaveBeenCalledTimes(1)
    const [req] = mockTauri.optimizePdfs.mock.calls[0]
    expect(req.preset).toBe('balanced')
    expect(req.files).toEqual(['C:\\a.pdf'])
    expect(typeof req.jobId).toBe('string')
    expect(req.jobId.length).toBeGreaterThan(0)
  })

  it('seeds preset from persisted prefs and persists only preset', async () => {
    localStorage.setItem(
      'toolbox.settings',
      JSON.stringify({ exportPath: 'C:\\out', tool: { pdfOptimizer: { preset: 'lossless' } } })
    )
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([validPdf('C:\\a.pdf', 'a.pdf')])
    mockTauri.optimizePdfs.mockResolvedValue(completedBatch([optimizedItem('C:\\a.pdf')]))
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    // lossless should be selected
    expect(screen.getByRole('button', { name: 'Lossless' })).toHaveClass('bg-elevated')
    await user.click(screen.getByRole('button', { name: 'Smaller File' }))
    await waitFor(() => {
      const prefs = JSON.parse(localStorage.getItem('toolbox.settings') ?? '{}').tool.pdfOptimizer
      expect(prefs).toEqual({ preset: 'smaller' })
    })
    await user.click(screen.getByRole('button', { name: /optimize 1 file/i }))
    const [req] = mockTauri.optimizePdfs.mock.calls[0]
    expect(req.preset).toBe('smaller')
  })

  it('shows progress and cancel while optimizing', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf', 'C:\\b.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([
      validPdf('C:\\a.pdf', 'a.pdf'),
      validPdf('C:\\b.pdf', 'b.pdf'),
    ])
    let resolve!: (v: OptimizePdfBatchResult) => void
    mockTauri.optimizePdfs.mockImplementation((_req, onProgress) => {
      onProgress?.({ jobId: 'j', completedFiles: 0, totalFiles: 2, currentFile: 'C:\\a.pdf', currentFilePercent: 30 })
      return new Promise((res) => { resolve = res })
    })
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 2 files/i }))
    expect(await screen.findByText('Processing 1 of 2')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument()
    resolve(completedBatch([optimizedItem('C:\\a.pdf'), optimizedItem('C:\\b.pdf')]))
    await screen.findByText(/2 files optimized/i)
  })

  it('cancels with same jobId and shows canceled summary', async () => {
    const user = userEvent.setup()
    let captured = ''
    let resolve!: (v: OptimizePdfBatchResult) => void
    mockTauri.optimizePdfs.mockImplementation((req) => {
      captured = req.jobId
      return new Promise((res) => { resolve = res })
    })
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf', 'C:\\b.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([
      validPdf('C:\\a.pdf', 'a.pdf'),
      validPdf('C:\\b.pdf', 'b.pdf'),
    ])
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 2 files/i }))
    await screen.findByText(/Processing/)
    await user.click(screen.getByRole('button', { name: /cancel/i }))
    expect(mockTauri.cancelPdfOptimizationJob).toHaveBeenCalledWith(captured)
    expect(screen.getByRole('button', { name: 'Canceling…' })).toBeDisabled()
    resolve(
      canceledBatch([
        optimizedItem('C:\\a.pdf'),
        {
          sourcePath: 'C:\\b.pdf',
          outcome: 'canceled',
          originalSize: 1000,
          error: { code: 'CANCELED', message: 'Processing canceled' },
        } as unknown as ReturnType<typeof optimizedItem>,
      ])
    )
    expect(await screen.findByText('Batch canceled')).toBeInTheDocument()
    expect(screen.getByText(/1 of 2 files finished before cancellation/i)).toBeInTheDocument()
  })

  it('shows mixed optimized, already, failed with size math and action visibility', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf', 'C:\\b.pdf', 'C:\\c.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([validPdf('C:\\a.pdf', 'a.pdf'), validPdf('C:\\b.pdf', 'b.pdf'), validPdf('C:\\c.pdf', 'c.pdf')])
    mockTauri.optimizePdfs.mockResolvedValue(
      completedBatch([
        optimizedItem('C:\\a.pdf', 2_000_000, 1_000_000),
        alreadyItem('C:\\b.pdf'),
        failedItem('C:\\c.pdf'),
      ])
    )
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 3 files/i }))
    await screen.findByText(/1 file optimized, 1 already well optimized/i)
    expect(screen.getByText('Optimized files')).toBeInTheDocument()
    expect(screen.getByText('Already well optimized')).toBeInTheDocument()
    expect(screen.getByText('1 file could not be processed')).toBeInTheDocument()
    // Size math: 2 MB -> 0.98 MB? Actually formatBytes 2_000_000 ~ 1.9 MB
    expect(screen.getByText(/Saved to/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /open folder/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /optimize more pdfs/i })).toBeInTheDocument()
  })

  it('hides open folder when no output exists', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([validPdf('C:\\a.pdf', 'a.pdf')])
    mockTauri.optimizePdfs.mockResolvedValue(completedBatch([alreadyItem('C:\\a.pdf')]))
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 1 file/i }))
    expect(await screen.findByText('Already well optimized')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /open folder/i })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /optimize more pdfs/i })).toBeInTheDocument()
  })

  it('shows all-failed without output actions', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([validPdf('C:\\a.pdf', 'a.pdf')])
    mockTauri.optimizePdfs.mockResolvedValue(completedBatch([failedItem('C:\\a.pdf')]))
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 1 file/i }))
    expect(await screen.findByText('No files optimized')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /open folder/i })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /try again/i })).toBeInTheDocument()
  })

  it('resets to empty after optimize more', async () => {
    const user = userEvent.setup()
    mockTauri.pickFiles.mockResolvedValue(['C:\\a.pdf'])
    mockTauri.inspectPdfsForOptimization.mockResolvedValue([validPdf('C:\\a.pdf', 'a.pdf')])
    mockTauri.optimizePdfs.mockResolvedValue(completedBatch([optimizedItem('C:\\a.pdf')]))
    renderWithProviders(<PdfOptimizer />)
    await user.click(screen.getByText('Drop PDFs here'))
    await screen.findByText('a.pdf')
    await user.click(screen.getByRole('button', { name: /optimize 1 file/i }))
    await screen.findByText(/1 file optimized/i)
    await user.click(screen.getByRole('button', { name: /optimize more pdfs/i }))
    expect(screen.getByText('Drop PDFs here')).toBeInTheDocument()
  })
})
