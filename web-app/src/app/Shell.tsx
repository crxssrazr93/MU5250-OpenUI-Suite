/* eslint-disable react-refresh/only-export-components */
import { Suspense, useEffect, useState, type ReactNode } from 'react'
import { useAlerts } from './HomeContext'
import { agentHost } from '../data/host'
import { IGauge, IGlobe, IHome, ISim, ISignal, IX, IMoon, ISun } from '../icons'
import { Spinner } from '../ui/primitives'

export type Group = 'home' | 'signal' | 'network' | 'modem' | 'system'

/** The set the URL is validated against, so a stale link falls back to Home. */
export const GROUPS = ['home', 'signal', 'network', 'modem', 'system'] as const satisfies readonly Group[]

export const NAV: { id: Group; label: string; icon: (p: { size?: number; className?: string }) => ReactNode }[] = [
  { id: 'home', label: 'Home', icon: (p) => <IHome {...p} /> },
  { id: 'signal', label: 'Signal', icon: (p) => <ISignal {...p} /> },
  { id: 'network', label: 'Network', icon: (p) => <IGlobe {...p} /> },
  { id: 'modem', label: 'Modem', icon: (p) => <ISim {...p} /> },
  { id: 'system', label: 'System', icon: (p) => <IGauge {...p} /> },
]

/**
 * One hue per section.
 *
 * Every section used to be the same blue, so the only thing telling you where
 * you were was a word. Giving each its own colour makes the active tab
 * recognisable peripherally, and lets each section's own accents agree with its
 * nav entry instead of all being generic.
 *
 * Tailwind needs whole class names to survive its scan, so these are written
 * out rather than built by interpolation.
 */
const GROUP_ACCENT: Record<Group, { active: string; rail: string; bar: string }> = {
  home: { active: 'bg-section/12 text-section', rail: 'text-section', bar: 'bg-section' },
  signal: { active: 'bg-violet/12 text-violet', rail: 'text-violet', bar: 'bg-violet' },
  network: { active: 'bg-cyan/12 text-cyan', rail: 'text-cyan', bar: 'bg-cyan' },
  modem: { active: 'bg-teal/12 text-teal', rail: 'text-teal', bar: 'bg-teal' },
  system: { active: 'bg-indigo/12 text-indigo', rail: 'text-indigo', bar: 'bg-indigo' },
}

/**
 * The CSS variable each section publishes.
 *
 * Setting `--section` on the document root retints everything that uses the
 * `section` token — cards, tabs, primary buttons, the background wash — from
 * one place, without threading a colour through every component or building
 * class names Tailwind's scanner cannot see.
 */
const GROUP_HUE: Record<Group, string> = {
  home: 'var(--accent)',
  signal: 'var(--violet)',
  network: 'var(--cyan)',
  modem: 'var(--teal)',
  system: 'var(--indigo)',
}

const GROUP_TITLES: Record<Group, string> = {
  home: 'Home',
  signal: 'Signal',
  network: 'Network',
  modem: 'Modem',
  system: 'System',
}

/** Publish the active section's hue to CSS. */
function useSectionHue(group: Group) {
  useEffect(() => {
    document.documentElement.style.setProperty('--section', GROUP_HUE[group])
  }, [group])
}

// ── Alert banner (fed by the home poll — zero extra requests) ─────────────────

function AlertBanner() {
  const alerts = useAlerts()
  const [dismissed, setDismissed] = useState<Set<string>>(new Set())

  const visible = alerts.filter((a) => !dismissed.has(a.message))
  if (visible.length === 0) return null

  return (
    <div className="mb-4 space-y-1.5">
      {visible.map((a) => (
        <div
          key={a.message}
          className={`flex items-center gap-2.5 rounded-lg border px-3 py-2 text-[13px] font-medium ${
            a.level === 'error'
              ? 'border-danger/25 bg-danger/8 text-danger'
              : 'border-warn/25 bg-warn/8 text-warn'
          }`}
          role="alert"
        >
          <span className="min-w-0 flex-1">{a.message}</span>
          <button
            onClick={() => setDismissed((prev) => new Set(prev).add(a.message))}
            className="shrink-0 opacity-60 transition-opacity hover:opacity-100"
            aria-label="Dismiss"
          >
            <IX size={14} />
          </button>
        </div>
      ))}
    </div>
  )
}

// ── Shell ─────────────────────────────────────────────────────────────────────

export default function Shell({
  group,
  onNavigate,
  theme,
  onToggleTheme,
  children,
}: {
  group: Group
  onNavigate: (g: Group) => void
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  children: ReactNode
}) {
  useSectionHue(group)

  // Lock body scroll while nothing needs it; keeps mobile address bar behavior sane.
  useEffect(() => {
    document.body.style.overflow = ''
  }, [])

  const themeIcon =
    theme === 'dark' ? <ISun size={17} /> : <IMoon size={17} />

  return (
    <div className="flex h-full bg-bg">
      {/* Desktop sidebar */}
      <aside className="hidden w-56 shrink-0 flex-col border-r border-line/8 bg-surface lg:flex">
        <div className="flex items-center gap-2.5 px-5 pb-5 pt-6">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent text-white">
            <ISignal size={17} />
          </div>
          <div className="min-w-0">
            <p className="truncate text-[13px] font-bold text-ink">U60 Pro</p>
            {/* The router being managed, which on desktop is not the host that
                served the page — there it would always read "localhost". */}
            <p className="tnum truncate text-[11px] text-ink3">{agentHost() ?? '—'}</p>
          </div>
        </div>

        <nav className="flex-1 space-y-0.5 px-3">
          {NAV.map((item) => (
            <button
              key={item.id}
              onClick={() => onNavigate(item.id)}
              className={`relative flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-[13px] font-semibold transition-colors ${
                group === item.id
                  ? GROUP_ACCENT[item.id].active
                  : 'text-ink2 hover:bg-surface2 hover:text-ink'
              }`}
            >
              {group === item.id && (
                <span
                  aria-hidden
                  className={`absolute left-0 top-1/2 h-5 w-[3px] -translate-y-1/2 rounded-r ${GROUP_ACCENT[item.id].bar}`}
                />
              )}
              {item.icon({ size: 17 })}
              {item.label}
            </button>
          ))}
        </nav>

        <div className="border-t border-line/8 px-5 py-3">
          <button
            onClick={onToggleTheme}
            className="flex items-center gap-2 text-[12px] font-medium text-ink2 transition-colors hover:text-ink"
          >
            {themeIcon}
            {theme === 'dark' ? 'Light mode' : 'Dark mode'}
          </button>
        </div>
      </aside>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Mobile top bar */}
        <header
          className="flex min-h-12 shrink-0 items-center justify-between border-b border-line/8 bg-surface px-4 lg:hidden"
          style={{ paddingTop: 'env(safe-area-inset-top)' }}
        >
          <span className="text-sm font-bold text-ink">{GROUP_TITLES[group]}</span>
          <button
            onClick={onToggleTheme}
            className="flex h-8 w-8 items-center justify-center rounded-lg text-ink2 transition-colors hover:bg-surface2 hover:text-ink"
            aria-label="Toggle theme"
          >
            {themeIcon}
          </button>
        </header>

        <main className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-shell px-4 pb-24 pt-4 lg:px-6 lg:pb-10 lg:pt-6">
            <AlertBanner />
            <Suspense
              fallback={
                <div className="flex justify-center py-20 text-ink3">
                  <Spinner size={22} />
                </div>
              }
            >
              {children}
            </Suspense>
          </div>
        </main>

        {/* Mobile bottom tabs */}
        <nav
          className="fixed inset-x-0 bottom-0 z-30 border-t border-line/8 bg-surface lg:hidden"
          style={{ paddingBottom: 'env(safe-area-inset-bottom)' }}
        >
          <div className="mx-auto flex max-w-md items-stretch justify-around">
            {NAV.map((item) => {
              const active = group === item.id
              return (
                <button
                  key={item.id}
                  onClick={() => onNavigate(item.id)}
                  className={`flex flex-1 flex-col items-center gap-0.5 pb-1.5 pt-2 text-[10px] font-semibold transition-colors ${
                    active ? GROUP_ACCENT[item.id].rail : 'text-ink3 hover:text-ink2'
                  }`}
                >
                  {item.icon({ size: 20 })}
                  {item.label}
                </button>
              )
            })}
          </div>
        </nav>
      </div>
    </div>
  )
}
