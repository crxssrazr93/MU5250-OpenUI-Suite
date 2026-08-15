import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../../data/api'
import type { WireguardProfile } from '../../types'
import { IPlus, IX } from '../../icons'
import { Button, Field, Input } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, Spinner } from '../../ui/primitives'

/**
 * Saved tunnels.
 *
 * The firmware has room for exactly one WireGuard config, so switching
 * providers used to mean overwriting the only copy — including a private key
 * that a commercial provider will not issue twice. Profiles are stored on the
 * router instead and copied into the vendor config when activated.
 *
 * Importing takes the provider's `.conf` verbatim. Retyping eight fields from a
 * file you already have is how a wrong character ends up in a key, and the
 * symptom of that is a tunnel that connects and never passes traffic.
 */
export default function WireguardProfiles({ onActivated }: { onActivated: () => void }) {
  const [profiles, setProfiles] = useState<WireguardProfile[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [conf, setConf] = useState('')
  const fileInput = useRef<HTMLInputElement>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setProfiles((await api.wireguardProfiles()).profiles ?? [])
    } catch {
      setProfiles([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  async function pickFile(file: File | undefined) {
    if (!file) return
    const text = await file.text()
    setConf(text)
    // The filename is nearly always more meaningful than anything a user would
    // type here, so offer it and let them override.
    if (!name.trim()) setName(file.name.replace(/\.conf$/i, ''))
  }

  async function save() {
    if (!conf.trim()) {
      toastError(new Error('Paste a config or choose a .conf file'), 'Nothing to import')
      return
    }
    setBusy(true)
    try {
      await api.wireguardProfileImport(name.trim() || 'Untitled tunnel', conf)
      toast('Profile saved')
      setAdding(false)
      setName('')
      setConf('')
      await load()
    } catch (e) {
      toastError(e, 'Could not import that config')
    } finally {
      setBusy(false)
    }
  }

  async function activate(profile: WireguardProfile) {
    setBusy(true)
    try {
      const result = await api.wireguardProfileActivate(profile.id)
      toast(
        result.endpoint
          ? `${result.name} is ready — endpoint ${result.endpoint}`
          : `${result.name} is ready`,
      )
      await load()
      // The settings panel below shows the vendor config, which this replaced.
      onActivated()
    } catch (e) {
      toastError(e, 'Could not activate that profile')
    } finally {
      setBusy(false)
    }
  }

  async function remove(profile: WireguardProfile) {
    const ok = await confirm({
      title: `Delete ${profile.name}?`,
      body:
        'The private key goes with it. Most providers issue a key once, so unless you still have ' +
        'the original .conf file this tunnel cannot be recreated.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!ok) return

    setBusy(true)
    try {
      await api.wireguardProfileDelete(profile.id)
      toast('Profile deleted')
      await load()
    } catch (e) {
      toastError(e, 'Could not delete that profile')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card
      title="Saved tunnels"
      action={
        <Button size="sm" variant={adding ? 'ghost' : 'outline'} onClick={() => setAdding((v) => !v)}>
          {adding ? <IX size={14} /> : <IPlus size={14} />}
          {adding ? 'Cancel' : 'Import'}
        </Button>
      }
    >
      <div className="space-y-3">
        {adding && (
          <div className="space-y-2.5 rounded-lg border border-line/8 bg-surface2 p-3">
            <Field label="Name">
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Provider, Mumbai"
                disabled={busy}
              />
            </Field>
            <Field
              label="Configuration"
              hint="Paste the provider's .conf exactly as given, or choose the file."
            >
              <textarea
                value={conf}
                onChange={(e) => setConf(e.target.value)}
                rows={7}
                spellCheck={false}
                disabled={busy}
                placeholder={'[Interface]\nPrivateKey = …\nAddress = …\n\n[Peer]\nPublicKey = …\nEndpoint = host:port\nAllowedIPs = 0.0.0.0/0'}
                className="w-full rounded-lg border border-line/10 bg-surface px-3 py-2 font-mono text-[12px] text-ink placeholder:text-ink3 focus:border-section/50 focus:outline-none"
              />
            </Field>
            <div className="flex items-center gap-2">
              <Button size="sm" variant="primary" onClick={save} loading={busy}>
                Save profile
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => fileInput.current?.click()}
                disabled={busy}
              >
                Choose file…
              </Button>
              <input
                ref={fileInput}
                type="file"
                accept=".conf,text/plain"
                className="hidden"
                onChange={(e) => pickFile(e.target.files?.[0])}
              />
            </div>
            <p className="text-[11px] text-ink3">
              The endpoint hostname is stored and looked up each time you activate, so a provider
              moving servers does not silently break the tunnel. DNS, MTU and keepalive settings in
              the file are ignored — this firmware has nowhere to put them.
            </p>
          </div>
        )}

        {loading && (
          <div className="flex items-center gap-2 text-[13px] text-ink3">
            <Spinner size={14} />
            Loading…
          </div>
        )}

        {!loading && profiles.length === 0 && !adding && (
          <p className="text-[13px] text-ink3">
            No saved tunnels. Import a provider's .conf to keep more than one.
          </p>
        )}

        {profiles.length > 0 && (
          <div className="space-y-1.5">
            {profiles.map((p) => (
              <div
                key={p.id}
                className="flex items-center gap-2.5 rounded-lg border border-line/8 bg-surface2 px-3 py-2"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[13px] font-semibold text-ink">{p.name}</span>
                    {p.active && <Chip tone="ok">Active</Chip>}
                  </div>
                  <div className="tnum mt-0.5 truncate text-[11px] text-ink3">
                    {p.settings.peer_host ?? p.settings.peer_endip}:{p.settings.peer_listen_port} ·{' '}
                    {p.settings.tunnel_ip}
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => activate(p)}
                  disabled={busy || p.active}
                >
                  {p.active ? 'In use' : 'Use'}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => remove(p)} disabled={busy}>
                  <IX size={14} />
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>
    </Card>
  )
}
