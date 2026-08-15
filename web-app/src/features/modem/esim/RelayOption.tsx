import { useEffect, useState } from 'react'
import { api } from '../../../data/api'
import { Toggle } from '../../../ui/controls'
import { Spinner } from '../../../ui/primitives'

/**
 * The relay switch.
 *
 * A router being set up for the first time has no WAN — the cellular link it
 * would use is the one the profile provides. The relay breaks that cycle by
 * having something else on the LAN perform the operator requests.
 *
 * It sits at the top of the tab rather than inside the add-profile panel,
 * because it is no longer only about downloading. Enabling, disabling and
 * deleting each report to the operator too, and the agent sends those inside
 * the operation. Hidden behind "Add profile", the switch was unreachable for
 * exactly the operations that most need it — a delete whose notification never
 * arrives leaves the activation code spent at the operator forever.
 */
export default function RelayOption({
  enabled,
  onChange,
  disabled,
}: {
  enabled: boolean
  onChange: (next: boolean) => void
  disabled: boolean
}) {
  const [status, setStatus] = useState({ active: false, waiting: false, connected: false })
  // Derived, not cleared in the effect: turning the switch off is a rendering
  // decision, and writing it back would just cascade another render.
  const active = enabled && status.active
  const waiting = enabled && status.waiting
  const connected = enabled && status.connected

  // Polled while the switch is on rather than only during a download, because
  // the switch now governs enable, disable and delete as well and those are
  // driven from elsewhere on the page. Off, it polls nothing.
  useEffect(() => {
    if (!enabled) return
    let live = true
    const tick = () => {
      api
        .euiccRelayStatus()
        .then(
          (s) =>
            live &&
            setStatus({
              active: s.active,
              waiting: s.waiting_for_client,
              connected: s.client_connected,
            }),
        )
        .catch(() => {})
    }
    tick()
    const timer = setInterval(tick, 3000)
    return () => {
      live = false
      clearInterval(timer)
    }
  }, [enabled])

  return (
    <div className="rounded-lg border border-line/8 bg-surface2 px-3 py-2.5">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[13px] font-semibold text-ink">Router has no internet</div>
          <p className="mt-0.5 text-[12px] text-ink3">
            Have another device carry the operator traffic. Needed for downloading a profile, and
            for reporting an enable, disable or delete back to the operator. Run the relay client
            on a phone or laptop that can reach both this router and the internet.
          </p>
        </div>
        <Toggle checked={enabled} onChange={onChange} disabled={disabled} label="Use a relay" />
      </div>

      {enabled && !active && (
        <>
          {/* Said up front, because the alternative is finding out only when an
              operation is refused. */}
          <div className="mt-2 flex items-center gap-1.5 text-[12px]">
            <span
              className={`h-1.5 w-1.5 rounded-full ${connected ? 'bg-ok' : 'bg-warn'}`}
              aria-hidden
            />
            <span className={connected ? 'text-ok' : 'text-warn'}>
              {connected ? 'Relay client connected' : 'No relay client connected'}
            </span>
          </div>
          {!connected && (
            <p className="mt-1.5 font-mono text-[11px] text-ink3">
              python3 scripts/relay-client.py --agent {location.origin} --password …
            </p>
          )}
        </>
      )}
      {active && (
        <div className="mt-2 flex items-center gap-2 text-[12px] text-ink2">
          <Spinner size={12} />
          {active && waiting ? 'Waiting for the relay client to answer…' : 'Working on the card…'}
        </div>
      )}
    </div>
  )
}
