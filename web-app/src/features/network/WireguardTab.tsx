import { useCallback, useEffect, useState } from 'react'
import { api } from '../../data/api'
import type { WireguardState } from '../../types'
import { IAlert } from '../../icons'
import { Button, Field, Input, Toggle } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, Spinner } from '../../ui/primitives'
import WireguardProfiles from './WireguardProfiles'

/**
 * The fields the agent will actually write.
 *
 * Deliberately the same list, in the same order, as `WRITABLE` in the agent's
 * tunnel module. Anything else is either read-only vendor state or a secret,
 * and the agent rejects it rather than silently ignoring it — so showing an
 * input for it would be offering something that cannot work.
 */
const FIELDS: { key: string; label: string; hint?: string; placeholder?: string }[] = [
  { key: 'tunnel_ip', label: 'Router address', hint: 'This end of the tunnel, e.g. 10.0.0.2/24', placeholder: '10.0.0.2/24' },
  { key: 'listen_port', label: 'Listen port', placeholder: '51820' },
  { key: 'peer_public_key', label: 'Peer public key', hint: "The server's public key, base64" },
  { key: 'peer_endip', label: 'Peer endpoint', hint: 'Host or IP of the server', placeholder: 'vpn.example.com' },
  { key: 'peer_listen_port', label: 'Peer port', placeholder: '51820' },
  { key: 'peer_tunnel_ip', label: 'Peer tunnel address', placeholder: '10.0.0.1/32' },
  { key: 'peer_remote_ip', label: 'Allowed network', hint: 'Traffic routed into the tunnel', placeholder: '0.0.0.0' },
  { key: 'peer_remote_mask', label: 'Allowed netmask', placeholder: '0.0.0.0' },
]

/** Everything connect() refuses to run without, so the button can say why. */
const REQUIRED_TO_CONNECT = ['peer_public_key', 'tunnel_ip', 'peer_endip']

export default function WireguardTab() {
  const [state, setState] = useState<WireguardState | null>(null)
  const [draft, setDraft] = useState<Record<string, string>>({})
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const next = await api.wireguard()
      setState(next)
      setDraft(next.settings ?? {})
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not read the tunnel settings')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  async function save() {
    setBusy(true)
    try {
      // Only what changed. Sending the whole form back would rewrite fields the
      // user never touched, and the masked ones would be written as asterisks.
      const changed: Record<string, string> = {}
      for (const { key } of FIELDS) {
        const value = draft[key] ?? ''
        if (value !== (state?.settings?.[key] ?? '')) changed[key] = value
      }
      if (Object.keys(changed).length === 0) {
        toast('Nothing changed')
        return
      }
      await api.wireguardSave(changed)
      toast('Settings saved')
      await load()
    } catch (e) {
      toastError(e, 'Could not save')
    } finally {
      setBusy(false)
    }
  }

  async function keygen() {
    const ok = await confirm({
      title: state?.configured ? 'Replace the existing key?' : 'Generate a key pair?',
      body: state?.configured
        ? 'The current private key is overwritten and cannot be recovered. Every peer configured ' +
          'with the old public key will stop accepting this router until you give them the new one.'
        : 'The private key stays on the router. Only the public key is shown, which is the half ' +
          'you give to the peer.',
      confirmLabel: 'Generate',
      danger: state?.configured,
    })
    if (!ok) return

    setBusy(true)
    try {
      const { public_key } = await api.wireguardKeygen()
      toast('Key pair generated')
      setPublicKey(public_key)
      await load()
    } catch (e) {
      toastError(e, 'Could not generate a key pair')
    } finally {
      setBusy(false)
    }
  }

  const [publicKey, setPublicKey] = useState<string | null>(null)

  async function toggleConnection(next: boolean) {
    setBusy(true)
    try {
      if (next) await api.wireguardConnect()
      else await api.wireguardDisconnect()
      toast(next ? 'Tunnel starting' : 'Tunnel stopped')
      await load()
    } catch (e) {
      toastError(e, next ? 'Could not start the tunnel' : 'Could not stop the tunnel')
    } finally {
      setBusy(false)
    }
  }

  if (loading) {
    return (
      <Card title="WireGuard">
        <div className="flex items-center gap-2 py-1 text-[13px] text-ink3">
          <Spinner size={14} />
          Reading tunnel settings…
        </div>
      </Card>
    )
  }

  if (error) {
    return (
      <Card title="WireGuard">
        <p className="text-[13px] text-warn">{error}</p>
      </Card>
    )
  }

  // The kernel has WireGuard, but the userspace `wg` tool is what generates
  // keys and what the vendor scripts shell out to. Without it nothing here can
  // work, and saying so beats every button failing individually.
  if (!state?.available) {
    return (
      <Card title="WireGuard">
        <div className="flex items-start gap-2 text-[13px] text-ink2">
          <IAlert size={16} className="mt-0.5 shrink-0 text-warn" />
          <p>
            The <code className="font-mono text-[12px]">wg</code> tool is not installed on this
            router, so keys cannot be generated and the vendor tunnel scripts have nothing to call.
            Run <code className="font-mono text-[12px]">scripts/zharden.sh</code>, which installs it
            to <code className="font-mono text-[12px]">/data/bin</code>.
          </p>
        </div>
      </Card>
    )
  }

  const connected = state.connect_status === 'connected' || state.connect_status === '1'
  const missing = REQUIRED_TO_CONNECT.filter((k) => !(draft[k] ?? '').trim())
  const canConnect = state.configured && missing.length === 0

  return (
    <div className="space-y-4">
      <WireguardProfiles onActivated={load} />

      <Card
        title="Tunnel"
        action={
          <Chip tone={connected ? 'ok' : 'default'}>
            {connected ? 'Connected' : state.connect_status || 'Not connected'}
          </Chip>
        }
      >
        <div className="space-y-3">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[13px] font-semibold text-ink">
                {state.configured ? 'Key pair present' : 'No key pair yet'}
              </div>
              <p className="mt-0.5 text-[12px] text-ink3">
                {state.configured
                  ? 'The private key is stored on the router and never sent to this page.'
                  : 'Generate one before connecting. The private half stays on the router.'}
              </p>
            </div>
            <Button size="sm" variant="outline" onClick={keygen} disabled={busy}>
              {state.configured ? 'Regenerate' : 'Generate'}
            </Button>
          </div>

          {publicKey && (
            <div className="rounded-lg border border-line/8 bg-surface2 px-3 py-2">
              <div className="text-[12px] font-semibold text-ink">Public key</div>
              <p className="mt-1 break-all font-mono text-[11px] text-ink2">{publicKey}</p>
              <p className="mt-1 text-[11px] text-ink3">
                Give this to the peer. Shown once here, but it can be read again from the peer's
                own configuration.
              </p>
            </div>
          )}

          <div className="flex items-center justify-between gap-3 border-t border-line/8 pt-3">
            <div className="min-w-0">
              <div className="text-[13px] font-semibold text-ink">Connection</div>
              <p className="mt-0.5 text-[12px] text-ink3">
                {canConnect
                  ? 'Brings the tunnel up using the vendor scripts.'
                  : missing.length > 0
                    ? `Still needed: ${missing.join(', ')}`
                    : 'Generate a key pair first.'}
              </p>
            </div>
            <Toggle
              checked={connected}
              onChange={toggleConnection}
              disabled={busy || (!connected && !canConnect)}
              label="Tunnel"
            />
          </div>
        </div>
      </Card>

      <Card
        title="Peer and addressing"
        action={
          <Button size="sm" variant="primary" onClick={save} loading={busy}>
            Save
          </Button>
        }
      >
        <div className="grid gap-3 sm:grid-cols-2">
          {FIELDS.map(({ key, label, hint, placeholder }) => (
            <Field key={key} label={label} hint={hint}>
              <Input
                value={draft[key] ?? ''}
                onChange={(e) => setDraft((d) => ({ ...d, [key]: e.target.value }))}
                placeholder={placeholder}
                disabled={busy}
              />
            </Field>
          ))}
        </div>
      </Card>
    </div>
  )
}
