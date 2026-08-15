/**
 * lpac's step names, shown as-is.
 *
 * These are the ES9+/ES10 function names — `es9p_authenticate_client` and so
 * on. They are not friendly, but when a download fails partway the step it
 * stopped at is the single most useful thing to have, and translating them
 * would only obscure which leg of the RSP conversation broke.
 */
export default function ProgressLog({ steps }: { steps: string[] }) {
  if (steps.length === 0) return null

  return (
    <details className="rounded-lg border border-line/8 bg-surface2 px-3 py-2">
      <summary className="cursor-pointer text-[12px] font-semibold text-ink2">
        Protocol steps ({steps.length})
      </summary>
      <ol className="mt-2 space-y-0.5">
        {steps.map((step, i) => (
          <li key={`${step}-${i}`} className="font-mono text-[11px] text-ink3">
            {step}
          </li>
        ))}
      </ol>
    </details>
  )
}
