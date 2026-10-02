import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { BatchProgress } from './BatchProgress'

describe('BatchProgress', () => {
  it('shows overall progress, current file detail, and a working cancel action', () => {
    const onCancel = vi.fn()
    render(
      <BatchProgress
        completedFiles={1}
        totalFiles={3}
        progress={47}
        currentFile="clip.mp4"
        currentFilePercent={38.5}
        cancelRequested={false}
        onCancel={onCancel}
      />
    )

    expect(screen.getByRole('status')).toHaveAttribute('aria-busy', 'true')
    expect(screen.getByText('Processing 2 of 3')).toBeInTheDocument()
    expect(screen.getByText('47%')).toBeInTheDocument()
    expect(screen.getByText('clip.mp4 · 39%')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onCancel).toHaveBeenCalledOnce()
  })

  it('omits per-file details when unavailable and disables an active cancellation request', () => {
    render(
      <BatchProgress
        completedFiles={0}
        totalFiles={1}
        progress={0}
        cancelRequested
        onCancel={vi.fn()}
      />
    )

    expect(screen.queryByText(/\.mp4/)).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Canceling…' })).toBeDisabled()
  })
})
