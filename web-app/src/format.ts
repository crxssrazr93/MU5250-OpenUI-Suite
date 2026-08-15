// Formatting helpers and signal-quality thresholds.

export function formatBytes(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`
  return `${bytes} B`
}

export function formatSpeed(bps: number): string {
  const mbps = (bps * 8) / 1_000_000
  if (mbps >= 1) return `${mbps.toFixed(1)} Mbps`
  const kbps = (bps * 8) / 1000
  return `${kbps.toFixed(0)} Kbps`
}

export function parseBandwidthMHz(bandwidth?: string): number {
  if (!bandwidth || bandwidth === '\u2014') return 0
  const match = bandwidth.match(/\d+(?:\.\d+)?/)
  return match ? parseFloat(match[0]) : 0
}

export function sumBandwidthMHz(carriers: { bandwidth?: string }[]): number {
  return carriers.reduce((sum, c) => sum + parseBandwidthMHz(c.bandwidth), 0)
}

export function formatBandwidthMHz(mhz: number): string {
  if (mhz <= 0) return '\u2014'
  return `${Number.isInteger(mhz) ? mhz.toFixed(0) : mhz.toFixed(1)} MHz`
}

export function formatUptime(secs?: number): string {
  if (!secs) return '\u2014'
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return [d && `${d}d`, (d || h) && `${h}h`, `${m}m`].filter(Boolean).join(' ')
}

export function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return '0s'
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

// ── Signal quality ────────────────────────────────────────────────────────────

export type Quality = 'excellent' | 'good' | 'fair' | 'poor' | 'unknown'

/**
 * RSRP in dBm to a quality band.
 *
 * The firmware reports `0` when it has no measurement — no service, no camped
 * cell — and 0 is greater than every threshold below, so it used to come out
 * as "Excellent" next to a 0/5 bars indicator. RSRP is only ever negative in
 * practice: 3GPP defines the reportable range as -140 to -44 dBm. So anything
 * at or above zero is an absent reading, not a strong one, and anything below
 * the floor is equally meaningless.
 */
export function rsrpQuality(rsrp?: number): Quality {
  if (rsrp == null) return 'unknown'
  if (rsrp >= 0 || rsrp < -140) return 'unknown'
  if (rsrp > -80) return 'excellent'
  if (rsrp > -90) return 'good'
  if (rsrp > -100) return 'fair'
  return 'poor'
}

export function qualityLabel(q: Quality): string {
  switch (q) {
    case 'excellent':
      return 'Excellent'
    case 'good':
      return 'Good'
    case 'fair':
      return 'Fair'
    case 'poor':
      return 'Weak'
    default:
      return '\u2014'
  }
}

/** Tailwind text color class for a quality level. */
export function qualityText(q: Quality): string {
  switch (q) {
    case 'excellent':
    case 'good':
      return 'text-ok'
    case 'fair':
      return 'text-warn'
    case 'poor':
      return 'text-danger'
    default:
      return 'text-ink3'
  }
}

/** Tailwind bg class for status dots / bars. */
export function qualityBg(q: Quality): string {
  switch (q) {
    case 'excellent':
    case 'good':
      return 'bg-ok'
    case 'fair':
      return 'bg-warn'
    case 'poor':
      return 'bg-danger'
    default:
      return 'bg-ink3'
  }
}

export function rsrpColorClass(rsrp?: number): string {
  return qualityText(rsrpQuality(rsrp))
}

export function rsrqColorClass(v?: number): string {
  if (v == null) return 'text-ink3'
  if (v > -10) return 'text-ok'
  if (v > -15) return 'text-warn'
  return 'text-danger'
}

export function sinrColorClass(v?: number): string {
  if (v == null) return 'text-ink3'
  if (v > 15) return 'text-ok'
  if (v > 5) return 'text-warn'
  return 'text-danger'
}

export function tempColorClass(c?: number): string {
  if (c == null) return 'text-ink3'
  if (c > 80) return 'text-danger'
  if (c > 60) return 'text-warn'
  return 'text-ok'
}

export function modemMode(type?: string): string {
  const raw = (type ?? '').toUpperCase()
  if (!raw) return '\u2014'
  if (raw.includes('ENDC') || raw.includes('NSA')) return 'ENDC'
  if (raw.includes('SA')) return 'SA'
  if (raw.includes('LTE') || raw === '4G') return 'LTE'
  if (raw.includes('NR') || raw.includes('5G')) return '5G'
  return raw
}
