// Capability discovery.
//
// The dashboard is built against a superset of what any one agent build serves
// — this fork dropped several upstream features, and the eUICC surface is new.
// Without asking, the only way to find out a route is missing is to call it and
// take a 404, which surfaces to the user as an error rather than as "absent".
//
// The answer is fixed for the lifetime of an agent process, so it is fetched
// once and shared. A failed fetch is treated as "assume supported": an older
// agent predating this endpoint still serves the rest of the API, and hiding
// everything would be worse than letting an individual call fail.

import { useEffect, useState } from 'react'
import { api } from './api'
import type { Capabilities } from '../types'

let cached: Capabilities | null = null
let inFlight: Promise<Capabilities | null> | null = null

export function loadCapabilities(): Promise<Capabilities | null> {
  if (cached) return Promise.resolve(cached)
  if (!inFlight) {
    inFlight = api
      .capabilities()
      .then((caps) => {
        cached = caps
        return caps
      })
      .catch(() => null)
      .finally(() => {
        inFlight = null
      })
  }
  return inFlight
}

/** Clears the cache — call after the agent restarts. */
export function resetCapabilities() {
  cached = null
}

/**
 * Whether the agent serves `feature`.
 *
 * Returns `true` while loading and if discovery failed, so the UI does not
 * flicker features out or hide them against an agent that simply predates
 * `/api/capabilities`.
 */
export function useCapability(feature: string): boolean {
  const [supported, setSupported] = useState(true)

  useEffect(() => {
    let active = true
    loadCapabilities().then((caps) => {
      if (!active || !caps) return
      setSupported(caps.supported[feature] === true)
    })
    return () => {
      active = false
    }
  }, [feature])

  return supported
}
