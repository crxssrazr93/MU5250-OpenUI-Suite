import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../../data/api'
import type { OperatorScan, ScannedOperator } from '../../types'
import { Button } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, Spinner } from '../../ui/primitives'

/** How often to re-ask while a scan is running. */
const POLL_MS = 3000

const STATUS_TONE = {
  current: 'ok',
  available: 'default',
  forbidden: 'danger',
  unknown: 'default',
} as const

export default function OperatorTab() {
  const [scan, setScan] = useState<OperatorScan | null>(null)
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(true)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const refresh = useCallback(async () => {
    try {
      const next = await api.operatorScan()
      setScan(next)
      return next
    } catch {
      // A modem that will not answer is reported by the surrounding page; this
      // panel going quiet is better than an error that replaces prior results.
      return null
    } finally {
      setLoading(false)
    }
  }, [])

  // Polls only while a scan is actually running. Left on a settled result the
  // page makes no requests at all.
  useEffect(() => {
    let live = true
    const tick = async () => {
      const next = await refresh()
      if (live && next?.scanning) timer.current = setTimeout(tick, POLL_MS)
    }
    tick()
    return () => {
      live = false
      if (timer.current) clearTimeout(timer.current)
    }
  }, [refresh])

  async function startScan() {
    const ok = await confirm({
      title: 'Scan for networks?',
      body:
        'The modem sweeps every band looking for operators, which takes about 40 seconds. ' +
        'Mobile data is interrupted for the whole scan.',
      confirmLabel: 'Scan',
    })
    if (!ok) return

    setBusy(true)
    try {
      await api.operatorScanStart()
      toast('Scanning…')
      const next = await refresh()
      if (next?.scanning) timer.current = setTimeout(async function tick() {
        const later = await refresh()
        if (later?.scanning) timer.current = setTimeout(tick, POLL_MS)
      }, POLL_MS)
    } catch (e) {
      toastError(e, 'Could not start a scan')
    } finally {
      setBusy(false)
    }
  }

  async function select(operator: ScannedOperator) {
    const ok = await confirm({
      title: `Register on ${operator.name}?`,
      body:
        operator.status === 'forbidden'
          ? 'This network reports itself as forbidden for your SIM, so registration will very ' +
            'likely be refused and the router may be left without service until you switch back ' +
            'to automatic.'
          : 'The modem stops choosing a network for itself and stays on this one, even when the ' +
            'signal is poor. Switch back to automatic to undo it.',
      confirmLabel: 'Register',
      danger: operator.status === 'forbidden',
    })
    if (!ok) return

    setBusy(true)
    try {
      await api.operatorSelect({ select: operator.select })
      toast(`Registering on ${operator.name}…`)
    } catch (e) {
      toastError(e, 'Could not select that network')
    } finally {
      setBusy(false)
    }
  }

  async function automatic() {
    setBusy(true)
    try {
      await api.operatorSelect({ auto: true })
      toast('Back to automatic selection')
    } catch (e) {
      toastError(e, 'Could not switch to automatic')
    } finally {
      setBusy(false)
    }
  }

  const operators = scan?.operators ?? []

  return (
    <Card
      title="Operator selection"
      action={
        <div className="flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={automatic} disabled={busy || scan?.scanning}>
            Automatic
          </Button>
          <Button
            size="sm"
            variant="primary"
            onClick={startScan}
            loading={busy}
            disabled={scan?.scanning}
          >
            Scan
          </Button>
        </div>
      }
    >
      <div className="space-y-3">
        <p className="text-[12px] text-ink2">
          Normally the modem picks a network itself. Scanning shows which are actually reachable,
          and one can be chosen by hand — useful when the automatic choice is a weaker network, or
          to confirm a SIM is barred somewhere.
        </p>

        {loading && (
          <div className="flex items-center gap-2 text-[13px] text-ink3">
            <Spinner size={14} />
            Checking…
          </div>
        )}

        {scan?.scanning && (
          <div className="flex items-center gap-2 rounded-lg border border-line/8 bg-surface2 px-3 py-2.5 text-[13px] text-ink2">
            <Spinner size={14} />
            Sweeping the bands. This takes about 40 seconds and mobile data is down until it
            finishes.
          </div>
        )}

        {!loading && !scan?.scanning && scan?.failed && (
          <div className="rounded-lg border border-line/8 bg-surface2 px-3 py-2.5 text-[13px] text-ink2">
            <span className="font-semibold text-warn">The scan failed.</span> The modem reports
            this when it cannot sweep — most often because it has no usable service to begin with,
            so check the SIM is registered before trying again.
          </div>
        )}

        {!loading && !scan?.scanning && !scan?.failed && operators.length === 0 && (
          <p className="text-[13px] text-ink3">
            No results yet. Run a scan to see which networks are in range.
          </p>
        )}

        {operators.length > 0 && (
          <div className="space-y-1.5">
            {operators.map((op) => (
              <div
                key={`${op.plmn}-${op.rat}`}
                className="flex items-center gap-2.5 rounded-lg border border-line/8 bg-surface2 px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[13px] font-semibold text-ink">{op.name}</span>
                    <Chip tone={STATUS_TONE[op.status]}>{op.status}</Chip>
                  </div>
                  <div className="tnum mt-0.5 text-[11px] text-ink3">
                    {op.mcc}-{op.mnc} · {op.rat}
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => select(op)}
                  disabled={busy || op.status === 'current'}
                >
                  {op.status === 'current' ? 'In use' : 'Use'}
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>
    </Card>
  )
}
