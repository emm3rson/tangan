import { useState } from 'react'
import { ArrowLeft, GearIcon, MoonIcon, SunIcon } from '@/components/ui/icons'
import { cx } from '@/components/ui'
import { toolById } from '@/tools/registry'
import type { ToolId } from '@/tools/types'
import { Launcher } from './Launcher'
import { Settings } from './Settings'
import { useSettings } from './providers/SettingsProvider'

type Route =
  | { view: 'launcher' }
  | { view: 'tool'; id: ToolId }
  | { view: 'settings' }

export function App() {
  const [route, setRoute] = useState<Route>({ view: 'launcher' })
  const { isDark, setTheme } = useSettings()

  const tool = route.view === 'tool' ? toolById(route.id) : null
  const Tool = tool?.component

  return (
    <div className="min-h-screen bg-background text-foreground flex flex-col">
      <header className="sticky top-0 z-10 flex h-13 items-center gap-3 border-b border-border bg-background/85 px-4 backdrop-blur-sm">
        {route.view === 'launcher' ? (
          <div className="flex items-center gap-2 pl-1">
            <span className="text-[13.5px] font-semibold tracking-tight">
              Tangan
            </span>
          </div>
        ) : (
          <>
            <button
              onClick={() => setRoute({ view: 'launcher' })}
              className="inline-flex items-center gap-1.5 rounded-[var(--radius-sm)] px-2 py-1.5 -ml-1 text-[13px] text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
            >
              <ArrowLeft size={16} />
              Tangan
            </button>
            <span className="text-border-strong">/</span>
            <span className="text-[13.5px] font-medium">
              {route.view === 'settings' ? 'Settings' : tool?.name}
            </span>
          </>
        )}

        <div className="ml-auto flex items-center gap-2">
          <button
            onClick={() => setTheme(isDark ? 'light' : 'dark')}
            aria-label="Toggle theme"
            className="grid h-8 w-8 place-items-center rounded-[var(--radius-sm)] text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
          >
            {isDark ? <SunIcon size={17} /> : <MoonIcon size={17} />}
          </button>
          <button
            onClick={() => setRoute({ view: 'settings' })}
            aria-label="Settings"
            className={cx(
              'grid h-8 w-8 place-items-center rounded-[var(--radius-sm)] transition-colors',
              route.view === 'settings'
                ? 'bg-muted text-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted'
            )}
          >
            <GearIcon size={16} />
          </button>
        </div>
      </header>

      <main className="flex-1">
        {route.view === 'launcher' && (
          <Launcher onOpen={(id) => setRoute({ view: 'tool', id })} />
        )}
        {route.view === 'tool' && Tool && (
          <div className="mx-auto max-w-[1120px] px-6 py-8">
            <h1 className="mb-6 text-[19px] font-semibold tracking-[-.015em]">
              {tool.name}
            </h1>
            <Tool />
          </div>
        )}
        {route.view === 'settings' && (
          <div className="mx-auto max-w-[1120px] px-6 py-10">
            <Settings />
          </div>
        )}
      </main>
    </div>
  )
}
