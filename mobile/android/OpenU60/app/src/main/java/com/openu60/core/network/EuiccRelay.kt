package com.openu60.core.network

import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.security.cert.CertificateFactory
import java.security.KeyStore
import java.util.concurrent.TimeUnit
import javax.inject.Inject
import javax.inject.Singleton
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager
import kotlin.coroutines.coroutineContext

/**
 * Carry the router's eSIM download traffic on its behalf.
 *
 * A router being provisioned for the first time has no WAN: the cellular link
 * it would use is the one the profile provides. That is a real cycle, and it
 * is the normal state of a device being set up.
 *
 * A phone breaks it. Joined to the router's Wi-Fi, it keeps mobile data active
 * precisely because that Wi-Fi has no internet, so it can reach both sides. The
 * agent parks each HTTPS request lpac makes; this class collects it, performs
 * it, and posts the response back.
 *
 * ```
 *   agent --park--> GET  /api/euicc/relay/pending   (long poll)
 *                          |  phone does the HTTPS itself
 *   agent <--------- POST /api/euicc/relay/response
 * ```
 *
 * This is the job the dashboard in a browser cannot do: those requests are
 * cross-origin and SM-DP+ servers do not answer CORS preflights, so `fetch` is
 * refused before it is sent. A native client has no such restriction, which is
 * the main reason this app exists rather than just bookmarking the dashboard.
 */
@Singleton
class EuiccRelay @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) {
    enum class State { Idle, Waiting, Carrying }

    private val _state = MutableStateFlow(State.Idle)
    val state: StateFlow<State> = _state.asStateFlow()

    private val _lastError = MutableStateFlow<String?>(null)
    val lastError: StateFlow<String?> = _lastError.asStateFlow()

    /**
     * ES9+ TLS is not anchored on the public web PKI.
     *
     * Some SM-DP+ servers present a WebPKI certificate; others present one
     * issued by a GSMA Certificate Issuer, which no Android trust store
     * carries. Both must work, so the GSMA roots ship in `res/raw` and are
     * added *alongside* the system anchors rather than replacing them.
     */
    private val client: OkHttpClient by lazy { buildClient() }

    private var extraAnchors: List<ByteArray> = emptyList()

    /** Supply GSMA CI roots (DER or PEM bytes) before the first request. */
    fun useTrustAnchors(anchors: List<ByteArray>) {
        extraAnchors = anchors
    }

    /**
     * Poll until cancelled, carrying whatever the agent parks.
     *
     * Only one request is ever outstanding, because all card access — and so
     * every lpac run — is serialized by a mutex on the agent.
     */
    suspend fun run() = withContext(Dispatchers.IO) {
        _lastError.value = null
        _state.value = State.Waiting
        try {
            while (coroutineContext.isActive) {
                // Deliberately not `continue` from inside runCatching: that
                // needs Kotlin 2.2, and the result is clearer read as a
                // nullable poll anyway.
                val pending = try {
                    agentClient.getSlowJSON("/api/euicc/relay/pending?wait=25")
                } catch (e: CancellationException) {
                    throw e
                } catch (e: AgentError.Unauthorized) {
                    // The agent restarting invalidates the token, and this
                    // relay is meant to sit running across exactly that. Left
                    // unhandled it polls forever with a dead token while every
                    // operation on the router quietly fails for want of a
                    // carrier.
                    Log.w(TAG, "session expired; signing in again")
                    if (!authManager.reauthenticate()) delay(RETRY_DELAY_MS)
                    null
                } catch (e: Exception) {
                    // The agent going quiet mid-download is normal — it is busy
                    // on the card. Keep polling rather than tearing the relay
                    // down. Backoff so a router that is down does not turn this
                    // into a hot loop on someone's battery.
                    Log.w(TAG, "poll failed: ${e.message}")
                    delay(RETRY_DELAY_MS)
                    null
                }

                @Suppress("UNCHECKED_CAST")
                val request = pending?.get("request") as? Map<String, Any?>
                if (request != null) {
                    _state.value = State.Carrying
                    carry(request)
                    _state.value = State.Waiting
                }
            }
        } finally {
            _state.value = State.Idle
        }
    }

    private suspend fun carry(request: Map<String, Any?>) {
        val id = (request["id"] as? Number)?.toLong() ?: return
        val url = request["url"]?.toString() ?: return
        val headers = (request["headers"] as? List<*>).orEmpty().map { it.toString() }
        val bodyHex = request["body_hex"]?.toString().orEmpty()

        val (status, responseHex) = perform(url, headers, bodyHex)

        runCatching {
            agentClient.postJSON(
                "/api/euicc/relay/response",
                mapOf("id" to id, "status" to status, "body_hex" to responseHex),
            )
        }.onFailure { Log.w(TAG, "handing back response failed: ${it.message}") }
    }

    private fun perform(url: String, headers: List<String>, bodyHex: String): Pair<Int, String> {
        val body = decodeHex(bodyHex)
        val builder = Request.Builder().url(url)
        for (header in headers) {
            val name = header.substringBefore(':').trim()
            val value = header.substringAfter(':', "").trim()
            if (name.isNotEmpty() && value.isNotEmpty()) builder.header(name, value)
        }
        if (body.isEmpty()) builder.get() else builder.post(body.toRequestBody())

        return try {
            client.newCall(builder.build()).execute().use { response ->
                // An HTTP error is a real answer and must be relayed as-is: an
                // SM-DP+ signals protocol failures with 4xx/5xx bodies that lpac
                // parses. Swallowing them would turn a clear operator error into
                // an opaque timeout.
                response.code to encodeHex(response.body?.bytes() ?: ByteArray(0))
            }
        } catch (e: Exception) {
            Log.w(TAG, "relayed request failed: ${e.message}")
            _lastError.value = e.message
            // Status 0 tells the agent the request never completed, which is
            // different from the server answering with an error.
            0 to ""
        }
    }

    private fun buildClient(): OkHttpClient {
        val builder = OkHttpClient.Builder()
            .connectTimeout(20, TimeUnit.SECONDS)
            .readTimeout(60, TimeUnit.SECONDS)
            .writeTimeout(60, TimeUnit.SECONDS)

        if (extraAnchors.isNotEmpty()) {
            runCatching { trustSystemPlus(extraAnchors) }
                .onSuccess { (context, manager) ->
                    builder.sslSocketFactory(context.socketFactory, manager)
                }
                .onFailure { Log.w(TAG, "GSMA anchors not loaded: ${it.message}") }
        }
        return builder.build()
    }

    /** System anchors plus the supplied certificates, not instead of them. */
    private fun trustSystemPlus(anchors: List<ByteArray>): Pair<SSLContext, X509TrustManager> {
        val factory = CertificateFactory.getInstance("X.509")
        val store = KeyStore.getInstance(KeyStore.getDefaultType()).apply { load(null, null) }

        // Seed with the platform's own anchors so ordinary WebPKI SM-DP+ hosts
        // keep working.
        val system = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
            .apply { init(null as KeyStore?) }
        val systemManager = system.trustManagers.filterIsInstance<X509TrustManager>().first()
        systemManager.acceptedIssuers.forEachIndexed { i, cert ->
            store.setCertificateEntry("system-$i", cert)
        }
        anchors.forEachIndexed { i, bytes ->
            store.setCertificateEntry("gsma-$i", factory.generateCertificate(bytes.inputStream()))
        }

        val merged = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
            .apply { init(store) }
        val manager = merged.trustManagers.filterIsInstance<X509TrustManager>().first()
        val context = SSLContext.getInstance("TLS").apply {
            init(null, arrayOf(manager), null)
        }
        return context to manager
    }

    private fun decodeHex(hex: String): ByteArray {
        if (hex.isEmpty() || hex.length % 2 != 0) return ByteArray(0)
        return runCatching {
            ByteArray(hex.length / 2) { hex.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
        }.getOrDefault(ByteArray(0))
    }

    private fun encodeHex(bytes: ByteArray): String =
        bytes.joinToString("") { "%02X".format(it) }

    private companion object {
        const val TAG = "EuiccRelay"

        /** Pause before retrying a failed poll, so a down router is not a hot loop. */
        const val RETRY_DELAY_MS = 3_000L
    }
}
