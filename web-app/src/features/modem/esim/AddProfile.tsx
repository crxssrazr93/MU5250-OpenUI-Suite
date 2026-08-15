import { useRef, useState } from 'react'
import { api } from '../../../data/api'
import type { EuiccDownloadRequest, EuiccOperation } from '../../../types'
import { IPlus, IX } from '../../../icons'
import { Button, Field, Input, Segmented } from '../../../ui/controls'
import { toastError } from '../../../ui/feedback'
import { Card, Chip } from '../../../ui/primitives'
import { decodeQrImage, parseActivationCode } from './activationCode'
import ProgressLog from './ProgressLog'

type Mode = 'code' | 'manual'

const MODES: { value: Mode; label: string }[] = [
  { value: 'code', label: 'Activation code' },
  { value: 'manual', label: 'Enter manually' },
]

/**
 * Download a profile, the three ways a user actually has one.
 *
 * EasyLPAC's split is the right one and this follows it: an activation code
 * (pasted, or read out of a QR image), or the SM-DP+ address and matching ID
 * typed in separately for the operators who mail those out as text.
 */
export default function AddProfile({
  onDownloaded,
  relay,
}: {
  onDownloaded: () => void
  /** Owned by the tab: every operation here reports to the operator. */
  relay: boolean
}) {
  const [open, setOpen] = useState(false)
  const [mode, setMode] = useState<Mode>('code')

  const [code, setCode] = useState('')
  const [smdp, setSmdp] = useState('')
  const [matchingId, setMatchingId] = useState('')
  const [confirmationCode, setConfirmationCode] = useState('')
  const [imei, setImei] = useState('')

  const [scanning, setScanning] = useState(false)
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState<string[]>([])
  const [error, setError] = useState<string | null>(null)
  const fileInput = useRef<HTMLInputElement>(null)

  const parsed = mode === 'code' ? parseActivationCode(code) : null
  const codeLooksWrong = mode === 'code' && code.trim().length > 0 && parsed == null
  const needsConfirmation = parsed?.confirmationCodeRequired ?? false
  const ready =
    mode === 'code'
      ? parsed != null && (!needsConfirmation || confirmationCode.trim().length > 0)
      : smdp.trim().length > 0 && matchingId.trim().length > 0

  function reset() {
    setCode('')
    setSmdp('')
    setMatchingId('')
    setConfirmationCode('')
    setImei('')
    setProgress([])
    setError(null)
  }

  async function scan(file: File) {
    setScanning(true)
    setError(null)
    try {
      const text = await decodeQrImage(file)
      setCode(text)
      if (parseActivationCode(text) == null) {
        setError('That QR code decoded, but it is not an eSIM activation code.')
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setScanning(false)
      // Let the same file be picked again after a failed scan.
      if (fileInput.current) fileInput.current.value = ''
    }
  }

  async function download() {
    const request: EuiccDownloadRequest = { relay }
    if (mode === 'code') {
      // Send the parsed parts rather than the raw string: the user may have
      // pasted a universal link, which lpac would not understand.
      request.smdp = parsed!.smdp
      request.matching_id = parsed!.matchingId
    } else {
      request.smdp = smdp.trim()
      request.matching_id = matchingId.trim()
    }
    if (confirmationCode.trim()) request.confirmation_code = confirmationCode.trim()
    if (imei.trim()) request.imei = imei.trim()

    setBusy(true)
    setError(null)
    setProgress([])
    try {
      const result: EuiccOperation = await api.euiccDownload(request)
      setProgress(result.progress ?? [])
      reset()
      setOpen(false)
      onDownloaded()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      toastError(e, 'Download failed')
    } finally {
      setBusy(false)
    }
  }

  if (!open) {
    return (
      <Card title="Add a profile">
        <div className="flex items-center justify-between gap-3">
          <p className="text-[13px] text-ink3">
            Scan a QR code, paste an activation code, or type the details from your operator.
          </p>
          <Button variant="primary" size="sm" onClick={() => setOpen(true)}>
            <IPlus size={14} />
            Add
          </Button>
        </div>
      </Card>
    )
  }

  return (
    <Card
      title="Add a profile"
      action={
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onClick={() => {
            reset()
            setOpen(false)
          }}
        >
          <IX size={14} />
        </Button>
      }
    >
      <div className="space-y-4">
        <Segmented options={MODES} value={mode} onChange={setMode} disabled={busy} />

        {mode === 'code' ? (
          <div className="space-y-3">
            <Field
              label="Activation code"
              hint="LPA:1$smdp.example.com$MATCHING-ID — a QR link from your operator works too."
            >
              <Input
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="LPA:1$rsp.example.com$ABC-123"
                spellCheck={false}
                autoComplete="off"
                disabled={busy}
                className="font-mono"
              />
            </Field>

            <div className="flex items-center gap-2">
              <input
                ref={fileInput}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0]
                  if (file) scan(file)
                }}
              />
              <Button
                size="sm"
                loading={scanning}
                disabled={busy}
                onClick={() => fileInput.current?.click()}
              >
                Scan QR image
              </Button>
              <span className="text-[11px] text-ink3">
                Decoded in your browser. The image is never uploaded.
              </span>
            </div>

            {parsed && (
              <div className="rounded-lg border border-line/8 bg-surface2 px-3 py-2.5 text-[12px]">
                <div className="mb-1.5 flex items-center gap-1.5">
                  <Chip tone="ok">Valid code</Chip>
                  {parsed.confirmationCodeRequired && (
                    <Chip tone="warn">Confirmation code required</Chip>
                  )}
                </div>
                <div className="text-ink3">
                  SM-DP+ <span className="font-mono text-ink2">{parsed.smdp}</span>
                </div>
                <div className="text-ink3">
                  Matching ID <span className="font-mono text-ink2">{parsed.matchingId}</span>
                </div>
              </div>
            )}
            {codeLooksWrong && (
              <p className="text-[12px] text-warn">
                That does not look like an activation code. It should start with
                <span className="font-mono"> LPA:1$</span>.
              </p>
            )}
          </div>
        ) : (
          <div className="space-y-3">
            <Field label="SM-DP+ address" hint="Hostname only, without https://">
              <Input
                value={smdp}
                onChange={(e) => setSmdp(e.target.value)}
                placeholder="rsp.example.com"
                spellCheck={false}
                autoComplete="off"
                disabled={busy}
                className="font-mono"
              />
            </Field>
            <Field label="Matching ID">
              <Input
                value={matchingId}
                onChange={(e) => setMatchingId(e.target.value)}
                placeholder="ABC-123-DEF"
                spellCheck={false}
                autoComplete="off"
                disabled={busy}
                className="font-mono"
              />
            </Field>
          </div>
        )}

        {(mode === 'manual' || needsConfirmation) && (
          <Field
            label="Confirmation code"
            hint={
              needsConfirmation
                ? 'This profile requires one. Your operator sends it separately.'
                : 'Optional — only if your operator issued one.'
            }
          >
            <Input
              type="password"
              value={confirmationCode}
              onChange={(e) => setConfirmationCode(e.target.value)}
              autoComplete="off"
              disabled={busy}
            />
          </Field>
        )}

        {mode === 'manual' && (
          <Field label="IMEI" hint="Optional. Some operators bind the profile to a device.">
            <Input
              value={imei}
              onChange={(e) => setImei(e.target.value)}
              placeholder="Leave blank to use the router's"
              inputMode="numeric"
              autoComplete="off"
              disabled={busy}
              className="font-mono"
            />
          </Field>
        )}

        
        {error && <p className="text-[12px] text-danger">{error}</p>}
        {progress.length > 0 && <ProgressLog steps={progress} />}

        <div className="flex items-center gap-2">
          <Button variant="primary" onClick={download} loading={busy} disabled={!ready}>
            Download profile
          </Button>
          {busy && (
            <span className="text-[12px] text-ink3">
              This talks to your operator and can take a minute. Don't close the page.
            </span>
          )}
        </div>
      </div>
    </Card>
  )
}
