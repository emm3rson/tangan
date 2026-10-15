import { beforeEach, describe, expect, it } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { mockTauri, renderWithProviders, resetMockTauri } from '@/test/setup'
import { App } from './App'

beforeEach(() => {
  resetMockTauri()
  localStorage.clear()
})

describe('App', () => {
  it('opens Trace to SVG as a separate utility from Convert Images', async () => {
    renderWithProviders(<App />)

    expect(screen.getByRole('button', { name: /Convert Images/ })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: /Trace to SVG/ }))

    expect(screen.getByText('Drop images here')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Trace to SVG' })).toBeInTheDocument()
  })

  it('keeps theme and settings controls in the navigation bar without a persistent version', async () => {
    mockTauri.getVersion.mockResolvedValue('0.4.0')

    renderWithProviders(<App />)

    expect(screen.getByLabelText('Toggle theme')).toBeInTheDocument()
    expect(screen.getByLabelText('Settings')).toBeInTheDocument()
    expect(screen.queryByTitle('Installed version')).not.toBeInTheDocument()
  })

  it('surfaces the installed app version inside Settings About section', async () => {
    mockTauri.getVersion.mockResolvedValue('0.4.0')

    renderWithProviders(<App />)

    await userEvent.click(screen.getByLabelText('Settings'))

    expect(await screen.findByTitle('Installed version')).toHaveTextContent(
      'v0.4.0'
    )
    expect(screen.getByText('About')).toBeInTheDocument()
  })
})
