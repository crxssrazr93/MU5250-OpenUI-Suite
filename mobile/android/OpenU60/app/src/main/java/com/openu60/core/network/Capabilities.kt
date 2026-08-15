package com.openu60.core.network

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import javax.inject.Inject
import javax.inject.Singleton

/**
 * What the agent on the other end actually serves.
 *
 * This app is built against a superset of any one agent build. This fork
 * dropped several features the upstream app still has screens for (speed test,
 * telemetry blocker, scheduler, SMS forwarding, DoH, AT writes), and added
 * eUICC management that upstream has no idea about. Without asking, the only
 * way to discover a missing route is to call it and take a 404, which reaches
 * the user as an error rather than as "this build does not do that".
 *
 * `unsupported` carries the agent's own reason for each omission, which is
 * worth showing: "removed: unrestricted AT access can brick the modem" is a
 * better answer than a greyed-out row.
 */
data class Capabilities(
    val agentVersion: String,
    val apiVersion: Int,
    val supported: Map<String, Boolean>,
    val unsupported: Map<String, String>,
) {
    fun has(feature: String): Boolean = supported[feature] == true

    fun reasonMissing(feature: String): String? = unsupported[feature]

}

@Singleton
class CapabilityProvider @Inject constructor(
    private val agentClient: AgentClient,
) {
    private val mutex = Mutex()
    private var cached: Capabilities? = null

    /** True while undiscovered, so the UI does not flicker features away. */
    private var discovered = false

    suspend fun get(): Capabilities? = mutex.withLock {
        cached?.let { return it }
        if (discovered) return null
        val fetched = runCatching { fetch() }.getOrNull()
        discovered = true
        cached = fetched
        fetched
    }

    /**
     * Whether `feature` is available.
     *
     * Returns true when discovery failed: an agent old enough to lack
     * `/api/capabilities` still serves the rest of the API, so assuming a
     * feature works and letting the individual call fail is less wrong than
     * hiding the whole app.
     */
    suspend fun has(feature: String): Boolean = get()?.has(feature) ?: true

    suspend fun reasonMissing(feature: String): String? = get()?.reasonMissing(feature)

    /** Drop the cache. The answer is fixed per agent process, so only restarts matter. */
    suspend fun reset() = mutex.withLock {
        cached = null
        discovered = false
    }

    private suspend fun fetch(): Capabilities {
        val data = agentClient.getJSON("/api/capabilities")

        @Suppress("UNCHECKED_CAST")
        val supported = (data["supported"] as? Map<String, Any?>).orEmpty()
            .mapValues { (_, v) -> v == true }

        @Suppress("UNCHECKED_CAST")
        val unsupported = (data["unsupported"] as? Map<String, Any?>).orEmpty()
            .mapValues { (_, v) -> v?.toString().orEmpty() }

        return Capabilities(
            agentVersion = data["agent_version"]?.toString() ?: "unknown",
            apiVersion = (data["api_version"] as? Number)?.toInt() ?: 0,
            supported = supported,
            unsupported = unsupported,
        )
    }
}

/** Feature keys the agent reports. Kept here so screens do not spell them by hand. */
object Feature {
    const val EUICC_READ = "euicc_read"
    const val EUICC_WRITE = "euicc_write"
    const val EUICC_HTTP_RELAY = "euicc_http_relay"
    const val SMS = "sms"
    const val WIFI = "wifi"
    const val APN = "apn"
    const val CELL_LOCK = "cell_lock"
    const val BAND_LOCK = "band_lock"
    const val ROUTER_DNS = "router_dns"
    const val ROUTER_LAN = "router_lan"
    const val USB = "usb"
    const val TTL = "ttl"
    const val CHARGE_CONTROL = "charge_control"
    const val SIGNAL_LOGGER = "signal_logger"
    const val CONNECTION_LOGGER = "connection_logger"
    const val AT_CONSOLE_READONLY = "at_console_readonly"
    const val SPEEDTEST = "speedtest"
    const val SCHEDULER = "scheduler"
    const val SMS_FORWARD = "sms_forward"
    const val TELEMETRY = "telemetry"
    const val DOH_PROXY = "doh_proxy"
    const val TAILSCALE = "tailscale"
    const val AT_CONSOLE_WRITE = "at_console_write"
}
