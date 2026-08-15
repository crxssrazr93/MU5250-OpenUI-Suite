// Where the agent is, and how to reach it.
//
// Served from the router, both answers are obvious: the agent is on the host
// that served the page, and the browser's own fetch will do. The desktop build
// has neither. Its page comes from `tauri://localhost`, so there is no router
// hostname to borrow, and that origin is not a LAN address, so the agent's CORS
// check rejects it — correctly; loosening the check to admit a desktop app
// would admit every other page too.
//
// So under Tauri the address is asked for once and remembered, and requests go
// out from Rust instead of from the webview. A request made outside a browser
// has no origin to police, which sidesteps CORS rather than weakening it.

const STORAGE_KEY = 'zte_agent_host'

/** True when running inside the Tauri shell rather than a browser tab. */
export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

type TauriFetch = typeof fetch

let desktopFetch: TauriFetch | null = null
let host: string | null = null

if (!isDesktop && typeof window !== 'undefined') {
  host = window.location.hostname
}

/**
 * Load the Tauri HTTP plugin.
 *
 * A plain dynamic import, so Vite splits it into its own chunk that is only
 * fetched when running under Tauri. Deliberately not an ignored bare specifier:
 * nothing would resolve that at runtime in either target. The chunk costs the
 * router-served build a file it never requests.
 */
export async function initDesktopTransport() {
  if (!isDesktop || desktopFetch) return
  const plugin = await import('@tauri-apps/plugin-http')
  desktopFetch = plugin.fetch as TauriFetch
  // Re-checked on the way in, not just on the way out: what is in storage was
  // last written by an older build, or by anything else with access to it.
  const stored = localStorage.getItem(STORAGE_KEY)
  host = stored && isPrivateAddress(stored) ? stored : null
}

/** The agent's base URL, or null on desktop before an address is chosen. */
export function apiBase(): string | null {
  return host ? `http://${host}:9090` : null
}

export function agentHost(): string | null {
  return host
}

/**
 * Whether an address is one the agent could legitimately be on.
 *
 * The same rule the agent applies to origins: loopback, or RFC1918. Enforced
 * here because the desktop scope cannot express it. Tauri filters the HTTP
 * plugin with `urlpattern`, whose Rust implementation will not match a wildcard
 * across a dot — `192.168.*.*` matches nothing, and `192.168.*` does not match
 * `192.168.0.1` either. Only an exact host or a bare `*` works, and the host is
 * not known until the user types it. So the scope is written as coarsely as it
 * can be expressed (plain HTTP, the agent's port) and the real restriction is
 * applied to the address before anything is ever requested from it.
 *
 * Addresses only, not names: a hostname resolves wherever DNS says, which would
 * put the check back where it cannot be made.
 */
export function isPrivateAddress(value: string): boolean {
  const host = value.trim()
  if (host === 'localhost' || host === '127.0.0.1' || host === '::1') return true
  const parts = host.split('.')
  if (parts.length !== 4) return false
  const octets = parts.map((p) => (/^\d{1,3}$/.test(p) ? Number(p) : NaN))
  if (octets.some((o) => Number.isNaN(o) || o > 255)) return false
  const [a, b] = octets
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || a === 127
}

/**
 * Point the app at a router. Desktop only — in a browser the host is whatever
 * served the page, and letting it be reassigned would only produce requests the
 * agent's CORS check then refuses.
 *
 * Returns false, rather than throwing, for an address outside the private
 * ranges: it is a thing the user typed, so it is answered in the form.
 */
export function setAgentHost(next: string): boolean {
  if (!isDesktop) return false
  const candidate = next.trim()
  if (!isPrivateAddress(candidate)) return false
  host = candidate
  localStorage.setItem(STORAGE_KEY, host)
  return true
}

export function clearAgentHost() {
  if (!isDesktop) return
  host = null
  localStorage.removeItem(STORAGE_KEY)
}

/** The browser's fetch in a tab, the Rust-side one under Tauri. */
export function transport(): TauriFetch {
  return desktopFetch ?? fetch
}
