import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import {
  loadSettings,
  saveSettings,
  type PersistedSettings
} from '@/services/settings'
import { renderWithProviders } from '@/test/setup'
import { useSettings } from './SettingsProvider'

function Preferences() {
  const settings = useSettings()
  return (
    <>
      <output data-testid="settings">
        {JSON.stringify({
          theme: settings.theme,
          exportPath: settings.exportPath,
          tool: settings.tool
        })}
      </output>
      <button onClick={() => settings.setTheme('light')}>Light</button>
    </>
  )
}

beforeEach(() => {
  localStorage.clear()
  vi.mocked(loadSettings).mockReset().mockResolvedValue({})
  vi.mocked(saveSettings).mockClear()
})

describe('settings persistence', () => {
  it('waits for native preferences before saving the initial state', async () => {
    let resolve!: (value: Partial<PersistedSettings>) => void
    vi.mocked(loadSettings).mockReturnValue(
      new Promise((done) => {
        resolve = done
      })
    )
    renderWithProviders(<Preferences />)
    expect(saveSettings).not.toHaveBeenCalled()
    expect(localStorage.getItem('toolbox.settings')).toBeNull()

    const stored = {
      theme: 'dark' as const,
      exportPath: 'C:\\exports',
      tool: { pdfOptimizer: { preset: 'lossless' as const } }
    }
    await act(async () => resolve(stored))
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(stored))
    expect(JSON.parse(localStorage.getItem('toolbox.settings')!)).toEqual(
      stored
    )

    await userEvent.click(screen.getByRole('button', { name: 'Light' }))
    await waitFor(() =>
      expect(saveSettings).toHaveBeenLastCalledWith({
        ...stored,
        theme: 'light'
      })
    )
  })

  it('retains cached preferences when the native store has no settings', async () => {
    const cached = {
      theme: 'dark',
      exportPath: 'C:\\cached',
      tool: { imageCompressor: { quality: 70 } }
    }
    localStorage.setItem('toolbox.settings', JSON.stringify(cached))
    renderWithProviders(<Preferences />)
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(cached))
    expect(JSON.parse(screen.getByTestId('settings').textContent!)).toEqual(
      cached
    )
  })

  it('ignores a native load that finishes after unmount', async () => {
    let resolve!: (value: Partial<PersistedSettings>) => void
    vi.mocked(loadSettings).mockReturnValue(
      new Promise((done) => {
        resolve = done
      })
    )
    const { unmount } = renderWithProviders(<Preferences />)
    unmount()
    await act(async () => resolve({ theme: 'dark' }))
    expect(saveSettings).not.toHaveBeenCalled()
  })
})
