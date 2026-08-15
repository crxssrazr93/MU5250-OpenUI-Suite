import { useCallback, useEffect, useState } from 'react'
import { api } from '../../data/api'
import { useCapability } from '../../data/capabilities'
import type {
  EuiccChipInfo,
  EuiccOperation,
  EuiccProfile,
  EuiccProfiles,
  EuiccStatus,
} from '../../types'
import { IAlert, IPencil, IRestart, ISim, ITrash, IX } from '../../icons'
import { Button, Input } from '../../ui/controls'
import { confirm, toast, toastError } from '../../ui/feedback'
import { Card, Chip, Empty, Row, Spinner } from '../../ui/primitives'
import type { ChipTone } from '../../ui/primitives'
import AddProfile from './esim/AddProfile'
import RelayOption from './esim/RelayOption'
import Notifications from './esim/Notifications'

/**
 * eSIM profile management.
 *
 * EID and ICCID are masked by default and only fetched in full on an explicit
 * action: they tie to a subscriber, and this page gets screenshotted into
 * issue reports.
 *
 * Every write goes through lpac on the router. If lpac is not installed the
 * agent says so via `/api/capabilities` and the whole management surface is
 * hidden rather than shown and then failing.
 */
export default function EsimTab() {
  const canWrite = useCapability('euicc_write')

  const [status, setStatus] = useState<EuiccStatus | null>(null)
  const [profiles, setProfiles] = useState<EuiccProfiles | null>(null)
  const [eid, setEid] = useState<string | null>(null)
  const [chip, setChip] = useState<EuiccChipInfo | null>(null)
  const [revealed, setRevealed] = useState(false)
  // Owned here rather than inside the add-profile panel: every operation on
  // this tab now reports to the operator, so they all need the same answer to
  // "can this router reach the internet, and if not, who is carrying for it".
  const [relay, setRelay] = useState(false)
  const [writeCount, setWriteCount] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [rebootNotice, setRebootNotice] = useState<string | null>(null)

  const load = useCallback(async (full: boolean) => {
    setLoading(true)
    setError(null)
    try {
      const s = await api.euiccStatus()
      setStatus(s)
      if (!s.euicc_available) {
        setProfiles(null)
        setEid(null)
        return
      }
      // Card access is serialized agent-side behind one mutex, so these are
      // sequential by necessity rather than for convenience.
      const e = await api.euiccEid(full)
      setEid(e.eid)
      setProfiles(await api.euiccProfiles(full))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load(false)
  }, [load])

  async function toggleReveal() {
    const next = !revealed
    setRevealed(next)
    try {
      await load(next)
    } catch (e) {
      setRevealed(!next)
      toastError(e, 'Failed to reload identifiers')
    }
  }

  async function loadChip() {
    try {
      const result = await api.euiccChip()
      setChip((result.result ?? null) as EuiccChipInfo | null)
    } catch (e) {
      toastError(e, 'Could not read chip info')
    }
  }

  /**
   * Get a usable ICCID for a profile the page may only be holding masked.
   *
   * Masking is about what is on screen, so it must not decide what the user is
   * allowed to do. The ISD-P AID is never masked and is unique per profile, so
   * it identifies the same row in the unmasked list.
   */
  async function resolveIccid(profile: EuiccProfile): Promise<string> {
    if (profile.iccid && !profile.iccid.includes('*')) return profile.iccid
    const full = await api.euiccProfiles(true)
    const match = full.profiles.find((p) => p.isdp_aid === profile.isdp_aid)
    if (!match?.iccid) throw new Error('Could not identify that profile on the card.')
    return match.iccid
  }

  /** Fold a write's result back into the page: refresh, and surface a notice. */
  async function afterWrite(result: EuiccOperation) {
    if (result.reboot_required && result.notice) setRebootNotice(result.notice)
    // Only ones the agent could not deliver survive an operation, so re-read
    // the queue to show whatever is genuinely stuck.
    setWriteCount((n) => n + 1)
    await load(revealed)
  }

  if (loading && status == null) {
    return (
      <Card title="eSIM">
        <div className="flex items-center gap-2 py-2 text-[13px] text-ink3">
          <Spinner size={14} />
          Reading card…
        </div>
      </Card>
    )
  }

  if (error != null) {
    return (
      <Card title="eSIM">
        <div className="space-y-3">
          <p className="text-[13px] text-danger">{error}</p>
          <Button size="sm" onClick={() => load(revealed)}>
            Retry
          </Button>
        </div>
      </Card>
    )
  }

  if (status != null && !status.euicc_available) {
    return (
      <Card title="eSIM">
        <Empty
          icon={<ISim size={20} />}
          title={status.card_present ? 'No eUICC on this card' : 'No card detected'}
          body={status.detail}
        />
      </Card>
    )
  }

  const enabledCount = profiles?.profiles.filter((p) => p.enabled).length ?? 0

  return (
    <div className="space-y-4">
      {rebootNotice && (
        <RebootBanner notice={rebootNotice} onDismiss={() => setRebootNotice(null)} />
      )}

      <Card
        title="eUICC"
        action={
          <Button size="sm" onClick={toggleReveal} loading={loading}>
            {revealed ? 'Hide identifiers' : 'Show full identifiers'}
          </Button>
        }
      >
        <div className="space-y-1">
          <Row label="Status" value={<Chip tone="ok">Detected</Chip>} />
          <Row label="EID" value={eid ?? '—'} mono wrap />
          <Row label="Profiles" value={profiles?.count ?? 0} />
          {chip?.EUICCInfo2 && (
            <>
              <Row label="Firmware" value={chip.EUICCInfo2.euiccFirmwareVer ?? '—'} />
              <Row label="SGP.22 version" value={chip.EUICCInfo2.profileVersion ?? '—'} />
              <Row label="Free space" value={formatFreeSpace(chip.EUICCInfo2.extCardResource)} />
            </>
          )}
        </div>
        {canWrite && chip == null && (
          <Button size="sm" variant="ghost" className="mt-2 -ml-3" onClick={loadChip}>
            Read chip details
          </Button>
        )}
      </Card>

      {canWrite && (
        <>
          <RelayOption enabled={relay} onChange={setRelay} disabled={false} />
          <AddProfile onDownloaded={() => load(revealed)} relay={relay} />
        </>
      )}

      <Card title="Profiles">
        {profiles == null || profiles.profiles.length === 0 ? (
          <Empty
            icon={<ISim size={20} />}
            title="No profiles installed"
            body="This eUICC is commissioned but carries no profiles."
          />
        ) : (
          <div className="space-y-3">
            {profiles.profiles.map((p, i) => (
              <ProfileRow
                key={p.iccid ?? p.isdp_aid ?? i}
                profile={p}
                canWrite={canWrite}
                isOnlyEnabled={p.enabled && enabledCount === 1}
                resolveIccid={resolveIccid}
                relay={relay}
                onChanged={afterWrite}
              />
            ))}
          </div>
        )}
      </Card>

      {canWrite && <Notifications relayAvailable relay={relay} reloadKey={writeCount} />}

      {!canWrite && (
        <p className="px-1 text-[12px] text-ink3">
          This agent has no lpac installed, so profiles can be read but not changed.
        </p>
      )}
    </div>
  )
}

/**
 * The switch happened on the card but the modem has not seen it.
 *
 * This modem rejects ES10c EnableProfile's refresh flag — the firmware exposes
 * no STK/CAT path for a REFRESH to travel over — so the profile list is already
 * correct while the network side still shows the old subscriber. Saying nothing
 * would make a successful operation look like a failed one.
 */
function RebootBanner({ notice, onDismiss }: { notice: string; onDismiss: () => void }) {
  const [rebooting, setRebooting] = useState(false)

  async function reboot() {
    const ok = await confirm({
      title: 'Reboot the router?',
      body: 'Every connected device loses its connection for about a minute.',
      confirmLabel: 'Reboot',
      danger: true,
    })
    if (!ok) return
    setRebooting(true)
    try {
      await api.reboot()
      toast('Rebooting…')
    } catch (e) {
      toastError(e, 'Reboot failed')
      setRebooting(false)
    }
  }

  return (
    <div className="flex items-start gap-2.5 rounded-lg border border-warn/30 bg-warn/10 px-3 py-2.5">
      <IAlert size={16} className="mt-0.5 shrink-0 text-warn" />
      <div className="min-w-0 flex-1">
        <p className="text-[13px] text-ink">{notice}</p>
        <div className="mt-2 flex items-center gap-2">
          <Button size="sm" variant="primary" onClick={reboot} loading={rebooting}>
            <IRestart size={13} />
            Reboot now
          </Button>
          <Button size="sm" variant="ghost" onClick={onDismiss} disabled={rebooting}>
            Later
          </Button>
        </div>
      </div>
    </div>
  )
}

const CLASS_TONES: Record<string, ChipTone> = {
  operational: 'accent',
  provisioning: 'warn',
  test: 'warn',
}

function ProfileRow({
  profile,
  canWrite,
  isOnlyEnabled,
  resolveIccid,
  relay,
  onChanged,
}: {
  profile: EuiccProfile
  canWrite: boolean
  isOnlyEnabled: boolean
  resolveIccid: (profile: EuiccProfile) => Promise<string>
  /** Whether a LAN client is carrying this router's operator traffic. */
  relay: boolean
  onChanged: (result: EuiccOperation) => Promise<void>
}) {
  const title =
    profile.nickname || profile.name || profile.service_provider || 'Unnamed profile'
  const [busy, setBusy] = useState(false)
  const [renaming, setRenaming] = useState(false)
  const [nickname, setNickname] = useState(profile.nickname ?? '')

  // Every operation is addressed by ICCID. A masked one can be resolved, but a
  // profile carrying neither an ICCID nor an ISD-P AID — which the card should
  // never produce — cannot be acted on safely.
  const actionable = canWrite && (profile.iccid != null || profile.isdp_aid != null)

  async function run(label: string, work: (iccid: string) => Promise<EuiccOperation>) {
    setBusy(true)
    try {
      await onChanged(await work(await resolveIccid(profile)))
      toast(label)
    } catch (e) {
      toastError(e, `${label} failed`)
    } finally {
      setBusy(false)
    }
  }

  async function enable() {
    await run('Profile enabled', (iccid) => api.euiccEnable(iccid, false, relay))
  }

  async function disable() {
    const ok = await confirm({
      title: `Disable ${title}?`,
      body: isOnlyEnabled
        ? 'This is the only enabled profile. The router will have no mobile service until ' +
          'you enable another one.'
        : 'The profile stays on the card and can be enabled again.',
      confirmLabel: 'Disable',
      danger: isOnlyEnabled,
    })
    if (!ok) return
    await run('Profile disabled', (iccid) => api.euiccDisable(iccid, isOnlyEnabled, relay))
  }

  async function remove() {
    const ok = await confirm({
      title: `Delete ${title}?`,
      body:
        'This erases the profile from the card. The operator is told, which is what lets some ' +
        'of them release the activation code for reuse — but many issue single-use codes, so ' +
        'treat it as permanent.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!ok) return
    await run('Profile deleted', (iccid) => api.euiccDelete(iccid, relay))
  }

  async function saveNickname() {
    setRenaming(false)
    if (nickname.trim() === (profile.nickname ?? '')) return
    await run('Renamed', (iccid) => api.euiccNickname(iccid, nickname.trim()))
  }

  return (
    <div className="rounded-lg border border-line/8 bg-surface2 px-3 py-2.5">
      <div className="flex items-center justify-between gap-2">
        {renaming ? (
          <Input
            autoFocus
            value={nickname}
            onChange={(e) => setNickname(e.target.value)}
            onBlur={saveNickname}
            onKeyDown={(e) => {
              if (e.key === 'Enter') saveNickname()
              if (e.key === 'Escape') {
                setNickname(profile.nickname ?? '')
                setRenaming(false)
              }
            }}
            placeholder="Nickname — leave blank to clear"
            maxLength={64}
            className="h-8"
          />
        ) : (
          <span className="min-w-0 truncate text-[13px] font-semibold text-ink">{title}</span>
        )}
        <div className="flex shrink-0 items-center gap-1.5">
          <Chip tone={profile.enabled ? 'ok' : 'default'}>{profile.state}</Chip>
          <Chip tone={CLASS_TONES[profile.class] ?? 'default'}>{profile.class}</Chip>
        </div>
      </div>

      <div className="mt-2 space-y-0.5">
        {profile.service_provider != null && (
          <Row label="Provider" value={profile.service_provider} />
        )}
        {profile.name != null && profile.name !== title && (
          <Row label="Name" value={profile.name} />
        )}
        <Row label="ICCID" value={profile.iccid ?? '—'} mono wrap />
        {profile.isdp_aid != null && <Row label="ISD-P AID" value={profile.isdp_aid} mono wrap />}
      </div>

      {actionable && (
        <div className="mt-2.5 flex flex-wrap items-center gap-1.5 border-t border-line/8 pt-2.5">
          {profile.enabled ? (
            <Button size="sm" onClick={disable} loading={busy}>
              Disable
            </Button>
          ) : (
            <Button size="sm" variant="primary" onClick={enable} loading={busy}>
              Enable
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || renaming}
            onClick={() => setRenaming(true)}
          >
            <IPencil size={13} />
            Rename
          </Button>
          {renaming && (
            <Button size="sm" variant="ghost" onClick={() => setRenaming(false)}>
              <IX size={13} />
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            className="text-danger"
            disabled={busy || profile.enabled}
            title={profile.enabled ? 'Disable the profile before deleting it' : undefined}
            onClick={remove}
          >
            <ITrash size={13} />
            Delete
          </Button>
        </div>
      )}
    </div>
  )
}

/**
 * Free non-volatile memory is what decides whether another profile will fit.
 * The other `extCardResource` counters are not actionable, so they are not
 * shown.
 */
function formatFreeSpace(resource?: { freeNonVolatileMemory?: number }): string {
  const bytes = resource?.freeNonVolatileMemory
  if (typeof bytes !== 'number') return '—'
  return bytes >= 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(2)} MB free`
    : `${Math.round(bytes / 1024)} KB free`
}
