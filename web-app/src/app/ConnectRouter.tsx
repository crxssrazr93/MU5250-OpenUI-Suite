import { useState } from 'react'
import { setAgentHost } from '../data/host'
import { ISignal } from '../icons'
import { Button } from '../ui/controls'

/**
 * Ask which router to talk to.
 *
 * Desktop only. Served from the router the question does not arise, but a
 * desktop window has no address to inherit, so it is asked once and kept.
 *
 * The address is only checked for shape here. Whether anything is listening is
 * settled by the sign-in that follows, which has to reach the agent anyway —
 * probing first would mean two ways to fail at the same thing.
 */
export default function ConnectRouter({ onSet }: { onSet: () => void }) {
  const [value, setValue] = useState('192.168.0.1')
  const [err, setErr] = useState('')

  function submit(e: React.FormEvent) {
    e.preventDefault()
    const host = value.trim()
    // A host, not a URL: the scheme and the agent's port are added for it, so a
    // pasted "http://…:9090" would otherwise become a URL with two of each.
    if (!/^[a-zA-Z0-9.:-]+$/.test(host)) {
      setErr('Enter an address like 192.168.0.1, without http:// or a port')
      return
    }
    if (!setAgentHost(host)) {
      setErr('Use the router’s address on your own network, such as 192.168.0.1')
      return
    }
    onSet()
  }

  return (
    <div className="flex min-h-full items-center justify-center bg-bg p-6">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex flex-col items-center text-center">
          <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-accent text-white">
            <ISignal size={24} />
          </div>
          <h1 className="text-lg font-bold text-ink">ZTE U60 Pro</h1>
          <p className="mt-0.5 text-[13px] text-ink2">Which router should this connect to?</p>
        </div>

        <form onSubmit={submit} className="space-y-4 rounded-xl border border-line/8 bg-surface p-5">
          <input
            value={value}
            onChange={(e) => {
              setValue(e.target.value)
              setErr('')
            }}
            className="h-11 w-full rounded-lg border border-line/12 bg-surface2/50 px-3.5 text-sm text-ink outline-none transition-colors placeholder:text-ink3 focus:border-accent/60"
            placeholder="192.168.0.1"
            autoFocus
            spellCheck={false}
            autoCapitalize="none"
            aria-label="Router address"
          />

          <p className="text-[12px] text-ink3">
            The router's LAN address. The agent's port is added automatically.
          </p>

          {err && <p className="text-xs font-medium text-danger">{err}</p>}

          <Button type="submit" variant="primary" disabled={!value.trim()} className="w-full !h-11">
            Continue
          </Button>
        </form>
      </div>
    </div>
  )
}
