import { useCallback, useEffect, useRef, useState } from 'react'

// Last-good values keyed by poll name, so tab switches render instantly and
// refresh in the background (stale-while-revalidate).
const cache = new Map<string, unknown>()

export interface PollResult<T> {
  data: T | null
  error: string | null
  refreshing: boolean
  refresh: () => void
  /**
   * Publish a value the caller already has — e.g. the authoritative state a
   * mutation endpoint returned — instead of spending a round trip re-fetching
   * what the server just told us.
   */
  mutate: (value: T) => void
}

/**
 * Self-rescheduling poller built for a low-power server:
 * - requests never overlap (next poll scheduled after the previous completes)
 * - fully idle while the tab is hidden
 * - serves cached data instantly on remount / tab switch
 *
 * `intervalMs` is read at schedule time rather than being an effect dependency,
 * so a caller can vary the cadence (e.g. slow down on a background tab) without
 * tearing down the loop and firing an immediate extra request.
 */
export function usePoll<T>(
  key: string,
  fn: () => Promise<T>,
  intervalMs: number,
  enabled = true,
): PollResult<T> {
  const [data, setData] = useState<T | null>(() => (cache.has(key) ? (cache.get(key) as T) : null))
  const [error, setError] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)
  const fnRef = useRef(fn)
  const intervalRef = useRef(intervalMs)
  const busy = useRef(false)

  useEffect(() => {
    fnRef.current = fn
    intervalRef.current = intervalMs
  })

  const tick = useCallback(
    async (manual = false) => {
      if (busy.current) return
      busy.current = true
      // Only surface a spinner for an explicit refresh — flagging every
      // background tick costs an extra render of the whole subtree.
      if (manual) setRefreshing(true)
      try {
        const value = await fnRef.current()
        cache.set(key, value)
        setData(value)
        setError(null)
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        busy.current = false
        if (manual) setRefreshing(false)
      }
    },
    [key],
  )

  const refresh = useCallback(() => {
    void tick(true)
  }, [tick])

  const mutate = useCallback(
    (value: T) => {
      cache.set(key, value)
      setData(value)
      setError(null)
    },
    [key],
  )

  useEffect(() => {
    if (!enabled) return
    let stopped = false
    let timer: ReturnType<typeof setTimeout> | undefined

    const loop = async () => {
      if (stopped || document.hidden) return // the visibility handler restarts us
      await tick()
      if (!stopped && !document.hidden) timer = setTimeout(loop, intervalRef.current)
    }

    void loop()

    const onVisibility = () => {
      if (!document.hidden && !stopped) {
        clearTimeout(timer)
        void loop()
      }
    }
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      stopped = true
      clearTimeout(timer)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [key, enabled, tick])

  return { data, error, refreshing, refresh, mutate }
}
