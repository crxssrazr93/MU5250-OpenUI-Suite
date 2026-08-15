import { useCallback, useSyncExternalStore } from 'react'

/**
 * Where you are, kept in the URL.
 *
 * Navigation used to be plain component state, so a refresh — or the browser
 * restoring the page after being backgrounded on a phone — dropped you back on
 * Home, and the back button left the dashboard entirely. The location is the
 * browser's own idea of "where am I", so putting it there fixes reload, back,
 * forward and bookmarking at once, and costs no router dependency.
 *
 * The shape is `#/<group>/<tab>`: segment 0 is the sidebar group, segment 1 is
 * the tab within it.
 */
const PATH_PREFIX = /^#\/?/

function subscribe(onChange: () => void) {
  window.addEventListener('hashchange', onChange)
  return () => window.removeEventListener('hashchange', onChange)
}

function readHash() {
  return window.location.hash
}

function split(hash: string) {
  return hash.replace(PATH_PREFIX, '').split('/').filter(Boolean)
}

/**
 * Read one segment of the hash path, validated against what actually exists.
 *
 * An unknown or missing segment reads as `fallback` rather than erroring, so a
 * hand-edited URL, a stale bookmark, or a tab that is hidden on this agent
 * (eSIM on a unit with no eUICC) degrades to the group's default instead of
 * rendering nothing.
 */
export function useHashSegment<T extends string>(
  depth: number,
  valid: readonly T[],
  fallback: T,
): [T, (next: T) => void] {
  // Server snapshot is unused here but required; the dashboard never SSRs.
  const hash = useSyncExternalStore(subscribe, readHash, () => '')
  const raw = split(hash)[depth]
  const value = valid.includes(raw as T) ? (raw as T) : fallback

  const set = useCallback(
    (next: T) => {
      // Truncate deeper segments: choosing a new group must not carry the old
      // group's tab along, and a tab name is only meaningful under its own group.
      const parts = split(readHash()).slice(0, depth)
      if (parts.length < depth) return
      parts.push(next)
      window.location.hash = `/${parts.join('/')}`
    },
    [depth],
  )

  return [value, set]
}

/**
 * Give the hash a group segment when it has none.
 *
 * Without this, a tab written at depth 1 on an empty hash would land in the
 * group slot. Uses `replaceState` so normalising the URL does not add a history
 * entry the user then has to press back through.
 */
export function normalizeHash(fallbackGroup: string) {
  if (split(readHash()).length === 0) {
    window.history.replaceState(null, '', `#/${fallbackGroup}`)
  }
}
