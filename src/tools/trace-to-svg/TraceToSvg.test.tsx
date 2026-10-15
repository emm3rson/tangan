import { beforeEach, describe, expect, it } from 'vitest'
import { act, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { mockTauri, renderWithProviders, resetMockTauri } from '@/test/setup'
import type { BatchResult, InputFile, ProgressHandler } from '@/services/tauri'
import { TraceToSvg } from './index'

const image = (path: string, extension = 'png'): InputFile => ({
  path,
  name: path.split('\\').pop() ?? path,
  extension,
  size: 4096,
  width: 120,
  height: 80,
  status: 'ready',
})

const batch: BatchResult = {
  total: 1,
  succeeded: 1,
  failed: 0,
  items: [
    {
      sourcePath: 'C:\\art.png',
      outputPath: 'C:\\out\\art.svg',
      success: true,
      originalSize: 4096,
      outputSize: 900,
    },
  ],
}

const preview = {
  svg: '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><circle cx="40" cy="40" r="20" fill="red"/></svg>',
  width: 120,
  height: 80,
}

async function addImages(paths: string[], files: InputFile[]) {
  mockTauri.pickFiles.mockResolvedValue(paths)
  mockTauri.inspectFiles.mockResolvedValue(files)
  const user = userEvent.setup()
  await user.click(screen.getByText('Drop images here'))
  return user
}

beforeEach(() => {
  resetMockTauri()
  localStorage.clear()
  localStorage.setItem(
    'toolbox.settings',
    JSON.stringify({ exportPath: 'C:\\out' }),
  )
})

describe('TraceToSvg', () => {
  it('shows the active file and freezes settings during a batch', async () => {
    let reportProgress!: ProgressHandler
    let finish!: (result: BatchResult) => void
    mockTauri.traceImages.mockImplementation((_request, onProgress) => {
      reportProgress = onProgress
      return new Promise<BatchResult>((resolve) => {
        finish = resolve
      })
    })
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(
      ['C:\\first.png', 'C:\\second.png'],
      [image('C:\\first.png'), image('C:\\second.png')],
    )
    expect(
      within(screen.getByRole('group', { name: 'Color mode' })).getByRole(
        'button',
        { name: 'Color' },
      ),
    ).toHaveAttribute('aria-pressed', 'true')
    await user.click(screen.getByRole('button', { name: 'Trace 2 images' }))
    expect(await screen.findByText('Tracing first.png…')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'High' })).toBeDisabled()
    act(() =>
      reportProgress({
        jobId: 'j',
        completed: 1,
        total: 2,
        currentFile: 'C:\\first.png',
      }),
    )
    expect(screen.getByText('Tracing second.png…')).toBeInTheDocument()
    await act(async () => finish(batch))
    expect(screen.getByText('1 image traced to SVG')).toBeInTheDocument()
  })

  it('uses the trace picker, rejects SVG, and sends default preview options', async () => {
    mockTauri.pickFiles.mockResolvedValue(['C:\\logo.png', 'C:\\vector.svg'])
    mockTauri.inspectFiles.mockResolvedValue([
      image('C:\\logo.png'),
      image('C:\\vector.svg', 'svg'),
    ])
    renderWithProviders(<TraceToSvg />)
    const user = userEvent.setup()
    await user.click(screen.getByText('Drop images here'))

    expect(mockTauri.pickFiles).toHaveBeenCalledWith('trace')
    expect(
      await screen.findByText(/SVG files cannot be traced/),
    ).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Trace 1 image' })).toBeEnabled()
    expect(mockTauri.inspectFiles).toHaveBeenCalledWith([
      'C:\\logo.png',
      'C:\\vector.svg',
    ])

    mockTauri.previewTrace.mockResolvedValue(preview)
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    await waitFor(() =>
      expect(mockTauri.previewTrace).toHaveBeenCalledWith({
        sourcePath: 'C:\\logo.png',
        colorMode: 'color',
        detail: 'balanced',
        smooth: true,
      }),
    )
    expect(
      await screen.findByRole('img', {
        name: 'Generated SVG preview of logo.png',
      }),
    ).toHaveAttribute('src', expect.stringContaining('data:image/svg+xml'))
  })

  it('invalidates a generated preview when settings change', async () => {
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(['C:\\art.png'], [image('C:\\art.png')])
    mockTauri.previewTrace.mockResolvedValue(preview)
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    expect(
      await screen.findByRole('img', {
        name: 'Generated SVG preview of art.png',
      }),
    ).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'High' }))
    expect(
      screen.queryByRole('img', { name: 'Generated SVG preview of art.png' }),
    ).not.toBeInTheDocument()
    expect(
      screen.getByText('Select Preview trace to see the result.'),
    ).toBeInTheDocument()
  })

  it('ignores a stale preview error after settings change', async () => {
    let rejectPreview!: (error: Error) => void
    mockTauri.previewTrace.mockReturnValueOnce(
      new Promise((_, reject) => {
        rejectPreview = reject
      }),
    )
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(['C:\\art.png'], [image('C:\\art.png')])
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    await user.click(screen.getByRole('button', { name: 'High' }))
    rejectPreview(new Error('old preview failed'))

    await waitFor(() =>
      expect(screen.queryByRole('alert')).not.toBeInTheDocument(),
    )
    expect(
      screen.getByText('Select Preview trace to see the result.'),
    ).toBeInTheDocument()
  })

  it('ignores a stale preview result after selecting another source', async () => {
    let resolvePreview!: (value: typeof preview) => void
    mockTauri.previewTrace.mockReturnValueOnce(
      new Promise((resolve) => {
        resolvePreview = resolve
      }),
    )
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(
      ['C:\\first.png', 'C:\\second.png'],
      [image('C:\\first.png'), image('C:\\second.png')],
    )
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    await user.click(screen.getByRole('button', { name: 'Select second.png' }))
    resolvePreview(preview)

    await waitFor(() =>
      expect(
        screen.queryByRole('img', {
          name: 'Generated SVG preview of first.png',
        }),
      ).not.toBeInTheDocument(),
    )
    expect(
      screen.getByText('Select Preview trace to see the result.'),
    ).toBeInTheDocument()
  })

  it('does not surface an old preview error over a completed export', async () => {
    let rejectPreview!: (error: Error) => void
    mockTauri.previewTrace.mockReturnValueOnce(
      new Promise((_, reject) => {
        rejectPreview = reject
      }),
    )
    mockTauri.traceImages.mockResolvedValue(batch)
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(['C:\\art.png'], [image('C:\\art.png')])
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    await user.click(screen.getByRole('button', { name: 'Trace 1 image' }))
    expect(await screen.findByText('1 image traced to SVG')).toBeInTheDocument()
    rejectPreview(new Error('late preview failure'))
    await waitFor(() =>
      expect(screen.queryByRole('alert')).not.toBeInTheDocument(),
    )
  })

  it('shows intake errors and preserves concurrent additions', async () => {
    mockTauri.pickFiles.mockRejectedValueOnce(new Error('picker unavailable'))
    renderWithProviders(<TraceToSvg />)
    const user = userEvent.setup()
    await user.click(screen.getByText('Drop images here'))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'picker unavailable',
    )

    mockTauri.pickFiles.mockResolvedValueOnce(['C:\\first.png'])
    mockTauri.inspectFiles.mockResolvedValueOnce([image('C:\\first.png')])
    await user.click(screen.getByText('Drop images here'))
    expect(
      await screen.findByRole('button', { name: 'Select first.png' }),
    ).toHaveAttribute('aria-pressed', 'true')

    let resolveSecond!: (value: InputFile[]) => void
    let resolveThird!: (value: InputFile[]) => void
    mockTauri.pickFiles
      .mockResolvedValueOnce(['C:\\second.png'])
      .mockResolvedValueOnce(['C:\\third.png'])
    mockTauri.inspectFiles
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveSecond = resolve
        }),
      )
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveThird = resolve
        }),
      )
    await user.click(screen.getByRole('button', { name: 'Add more' }))
    await user.click(screen.getByRole('button', { name: 'Add more' }))
    await waitFor(() => expect(mockTauri.inspectFiles).toHaveBeenCalledTimes(3))
    resolveThird([image('C:\\third.png')])
    resolveSecond([image('C:\\second.png')])

    expect(await screen.findByText('second.png')).toBeInTheDocument()
    expect(await screen.findByText('third.png')).toBeInTheDocument()
  })

  it('does not add files from an intake that finishes after clearing the queue', async () => {
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(['C:\\first.png'], [image('C:\\first.png')])
    let resolveInspect!: (value: InputFile[]) => void
    mockTauri.pickFiles.mockResolvedValueOnce(['C:\\late.png'])
    mockTauri.inspectFiles.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveInspect = resolve
      }),
    )
    await user.click(screen.getByRole('button', { name: 'Add more' }))
    await user.click(screen.getByRole('button', { name: 'Clear all' }))
    resolveInspect([image('C:\\late.png')])

    expect(await screen.findByText('Drop images here')).toBeInTheDocument()
    expect(screen.queryByText('late.png')).not.toBeInTheDocument()
  })

  it('recovers from preview and export errors, and sends the same current settings to export', async () => {
    renderWithProviders(<TraceToSvg />)
    const user = await addImages(['C:\\art.png'], [image('C:\\art.png')])
    mockTauri.previewTrace
      .mockRejectedValueOnce(new Error('preview unavailable'))
      .mockResolvedValueOnce(preview)
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'preview unavailable',
    )
    await user.click(screen.getByRole('button', { name: 'Preview trace' }))
    expect(
      await screen.findByRole('img', {
        name: 'Generated SVG preview of art.png',
      }),
    ).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Black & white' }))
    mockTauri.traceImages
      .mockRejectedValueOnce(new Error('export failed'))
      .mockResolvedValueOnce(batch)
    await user.click(screen.getByRole('button', { name: 'Trace 1 image' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('export failed')
    await user.click(screen.getByRole('button', { name: 'Trace 1 image' }))
    expect(await screen.findByText('1 image traced to SVG')).toBeInTheDocument()
    expect(mockTauri.traceImages).toHaveBeenLastCalledWith(
      {
        files: ['C:\\art.png'],
        outputDirectory: 'C:\\out',
        colorMode: 'binary',
        detail: 'balanced',
        smooth: true,
      },
      expect.any(Function),
    )
  })
})
