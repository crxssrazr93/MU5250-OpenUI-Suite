import { useCallback, useEffect, useState } from 'react'
import { api } from '../../../data/api'
import type { EuiccNotification } from '../../../types'
import { IAlert } from '../../../icons'
import { Button } from '../../../ui/controls'
import { confirm, toast, toastError } from '../../../ui/feedback'
import { Card, Chip, Spinner } from '../../../ui/primitives'

const OPERATION_LABELS: Record<string, string> = {
  install: 'Installed',
  enable: 'Enabled',
  disable: 'Disabled',
  delete: 'Deleted',
}

/**
 * Profile changes the operator has not been told about.
 *
 * This panel no longer sends anything on its own. The agent now delivers each
 * notification inside the operation that created it, while it still holds the
 * card and the relay, which is the only point at which delivery is reliable:
 * enabling ends in a reboot that would take this page with it, and a download's
 * relay is gone by the time a browser could ask for a separate send.
 *
 * So anything listed here has already failed at least once. That makes it worth
 * showing — and worth an explicit retry rather than a silent one, because the
 * usual reason is a dead SM-DP+ host that will fail again and again.
 */
export default function Notifications({
  relayAvailable,
  relay,
  reloadKey,
}: {
  relayAvailable: boolean
  /** Whether a LAN client is carrying this router's traffic right now. */
  relay: boolean
  /** Bumped by the tab after a write, so the list reflects what just happened. */
  reloadKey: number
}) {
  const [pending, setPending] = useState<EuiccNotification[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const listed = await api.euiccNotifications()
      setPending((Array.isArray(listed.result) ? listed.result : []) as EuiccNotification[])
    } catch {
      // A card that cannot be read is already reported by the page around this.
      setPending([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
  }, [load, reloadKey])

  async function retry() {
    setBusy(true)
    try {
      await api.euiccNotificationsProcess(
        pending.map((n) => n.seqNumber),
        relay,
      )
      toast('Sent to the operator')
    } catch (e) {
      toastError(e, 'Still could not reach the operator')
    } finally {
      setBusy(false)
      await load()
    }
  }

  async function discardAll() {
    const sequences = pending.map((n) => n.seqNumber)
    const ok = await confirm({
      title: `Discard ${sequences.length} notification${sequences.length === 1 ? '' : 's'}?`,
      body:
        'They are removed from the card without reaching the operator, so its records stay out ' +
        'of date. For a delete that means the operator still believes the profile is installed ' +
        'and the activation code stays spent. Reasonable when the address no longer exists, ' +
        'which is the usual reason something ends up here.',
      confirmLabel: 'Discard',
      danger: true,
    })
    if (!ok) return

    setBusy(true)
    try {
      await api.euiccNotificationsRemove(sequences)
      toast('Notifications discarded')
    } catch (e) {
      toastError(e, 'Could not discard notifications')
    } finally {
      setBusy(false)
      await load()
    }
  }

  if (loading) {
    return (
      <Card title="Operator notifications">
        <div className="flex items-center gap-2 py-1 text-[13px] text-ink3">
          <Spinner size={14} />
          Checking…
        </div>
      </Card>
    )
  }

  // Nothing queued is the normal state now that delivery happens inside each
  // operation. Saying "0 pending" would be noise on an already busy page.
  if (pending.length === 0) return null

  return (
    <Card
      title="Undelivered notifications"
      action={
        <Button size="sm" variant="ghost" onClick={retry} disabled={busy}>
          Retry
        </Button>
      }
    >
      <div className="space-y-3">
        <div className="flex items-start gap-2 text-[12px] text-ink2">
          <IAlert size={15} className="mt-0.5 shrink-0 text-warn" />
          <p>
            These changes were made on the card, but the operator could not be told. Almost always
            that means the router had no way to reach it.
            {relayAvailable && !relay
              ? ' Turn on "Router has no internet" above so another device carries the traffic, then retry.'
              : ' Check that the relay client is actually running and signed in, then retry.'}{' '}
            If retrying keeps failing, the operator may have retired that hostname — it is written
            into the profile at download time and cannot be changed, so those can never be
            delivered and discarding is the only way to clear them.
          </p>
        </div>

        <div className="space-y-1.5">
          {pending.map((n) => (
            <div
              key={n.seqNumber}
              className="flex items-center gap-2.5 rounded-lg border border-line/8 bg-surface2 px-3 py-2"
            >
              <Chip tone="warn">
                {OPERATION_LABELS[n.profileManagementOperation] ?? n.profileManagementOperation}
              </Chip>
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-[12px] text-ink2">{n.iccid}</div>
                <div className="truncate text-[11px] text-ink3">{n.notificationAddress}</div>
              </div>
              <span className="shrink-0 text-[11px] text-ink3">#{n.seqNumber}</span>
            </div>
          ))}
        </div>

        <Button size="sm" variant="outline" onClick={discardAll} loading={busy}>
          Discard all
        </Button>
      </div>
    </Card>
  )
}
