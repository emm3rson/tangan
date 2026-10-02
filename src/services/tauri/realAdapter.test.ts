import { beforeEach, describe, expect, it, vi } from 'vitest'
import { realAdapter } from './realAdapter'

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  open: vi.fn(),
  unlisten: vi.fn()
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: native.open }))

beforeEach(() => {
  vi.resetAllMocks()
  native.listen.mockResolvedValue(native.unlisten)
  native.invoke.mockResolvedValue({ items: [] })
})

const jobs = [
  {
    command: 'convert_images',
    event: 'processing-progress',
    run: (progress?: () => void) =>
      realAdapter.convertImages(
        {
          files: [],
          outputDirectory: 'C:\\out',
          outputFormat: 'png',
          resize: { mode: 'original' }
        },
        progress
      )
  },
  {
    command: 'compress_images',
    event: 'processing-progress',
    run: (progress?: () => void) =>
      realAdapter.compressImages(
        {
          files: [],
          outputDirectory: 'C:\\out',
          quality: 80,
          resize: { mode: 'original' }
        },
        progress
      )
  },
  {
    command: 'convert_pdfs',
    event: 'processing-progress',
    run: (progress?: () => void) =>
      realAdapter.convertPdfs(
        { files: [], outputDirectory: 'C:\\out' },
        progress
      )
  },
  {
    command: 'generate_logo_pack',
    event: 'processing-progress',
    run: (progress?: () => void) =>
      realAdapter.generateLogoPack(
        { sourcePath: 'logo.png', outputDirectory: 'C:\\out', assetIds: [] },
        progress
      )
  },
  {
    command: 'process_videos',
    event: 'video-progress',
    jobId: 'video-job',
    run: (progress?: () => void) =>
      realAdapter.processVideos(
        {
          files: [],
          outputDirectory: 'C:\\out',
          outputFormat: 'mp4',
          resolution: 'original',
          quality: 'balanced',
          jobId: 'video-job'
        },
        progress
      )
  },
  {
    command: 'optimize_pdfs',
    event: 'pdf-optimize-progress',
    jobId: 'pdf-job',
    run: (progress?: () => void) =>
      realAdapter.optimizePdfs(
        {
          files: [],
          outputDirectory: 'C:\\out',
          preset: 'balanced',
          jobId: 'pdf-job'
        },
        progress
      )
  }
]

describe.each(jobs)('$command progress lifecycle', (job) => {
  it('subscribes before invoking, filters other jobs, and cleans up', async () => {
    const progress = vi.fn()
    await job.run(progress)

    expect(native.listen).toHaveBeenCalledWith(job.event, expect.any(Function))
    expect(native.listen.mock.invocationCallOrder[0]).toBeLessThan(
      native.invoke.mock.invocationCallOrder[0]
    )
    const [command, { request }] = native.invoke.mock.calls[0]
    expect(command).toBe(job.command)
    expect(request.outputDirectory).toBe('C:\\out')
    expect(request.jobId).toEqual(job.jobId ?? expect.any(String))
    const listener = native.listen.mock.calls[0][1]
    listener({ payload: { jobId: 'other-job' } })
    expect(progress).not.toHaveBeenCalled()
    const payload = { jobId: request.jobId }
    listener({ payload })
    expect(progress).toHaveBeenCalledWith(payload)
    expect(native.unlisten).toHaveBeenCalledOnce()
  })

  it('cleans up and propagates processing failures', async () => {
    const failure = { code: 'PROCESSING_FAILED', message: 'Failed' }
    native.invoke.mockRejectedValue(failure)
    await expect(job.run(vi.fn())).rejects.toBe(failure)
    expect(native.unlisten).toHaveBeenCalledOnce()
  })

  it('skips event registration without a progress callback', async () => {
    await job.run()
    expect(native.listen).not.toHaveBeenCalled()
    expect(native.invoke).toHaveBeenCalledOnce()
  })

  it('does not start processing if event registration fails', async () => {
    native.listen.mockRejectedValue(new Error('Unavailable'))
    await expect(job.run(vi.fn())).rejects.toThrow('Unavailable')
    expect(native.invoke).not.toHaveBeenCalled()
  })
})

describe('file picker', () => {
  it.each([
    ['convert', true, ['png', 'jpg', 'jpeg', 'webp', 'svg']],
    ['compress', true, ['png', 'jpg', 'jpeg', 'webp']],
    ['logo', false, ['png', 'jpg', 'jpeg', 'webp', 'svg']],
    ['palette', false, ['png', 'jpg', 'jpeg', 'webp', 'svg']],
    ['pdf', true, ['pdf']],
    ['video', true, ['mp4', 'mov', 'mkv', 'webm', 'avi']]
  ] as const)(
    'preserves %s filters and selection mode',
    async (mode, multiple, extensions) => {
      native.open.mockResolvedValue(multiple ? ['source'] : 'source')
      expect(await realAdapter.pickFiles(mode)).toEqual(['source'])
      expect(native.open).toHaveBeenCalledWith({
        multiple,
        directory: false,
        filters: [{ name: expect.any(String), extensions }]
      })
    }
  )

  it('returns no paths when the picker is canceled', async () => {
    native.open.mockResolvedValue(null)
    expect(await realAdapter.pickFiles('logo')).toEqual([])
    expect(await realAdapter.pickFiles('pdf')).toEqual([])
  })
})
