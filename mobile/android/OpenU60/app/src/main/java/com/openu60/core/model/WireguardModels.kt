package com.openu60.core.model

/**
 * The vendor tunnel's current state, as the agent reports it.
 *
 * [available] is false when the `wg` userspace tool is missing: the kernel has
 * WireGuard, but keys are generated and the vendor scripts driven through `wg`,
 * so nothing here works without it.
 */
data class WireguardState(
    val available: Boolean,
    val configured: Boolean,
    val connectStatus: String,
    val settings: Map<String, String>,
) {
    val connected: Boolean get() = connectStatus == "connected" || connectStatus == "1"

    companion object {
        val empty = WireguardState(false, false, "", emptyMap())
    }
}

/** One saved tunnel. The firmware holds a single active config; these are the library. */
data class WireguardProfile(
    val id: Int,
    val name: String,
    /** Whether this profile's peer key is the one in the vendor config right now. */
    val active: Boolean,
    val settings: Map<String, String>,
) {
    /** host:port · address, matching the dashboard's one-line summary. */
    val summary: String
        get() {
            val host = settings["peer_host"] ?: settings["peer_endip"] ?: ""
            val port = settings["peer_listen_port"] ?: ""
            val addr = settings["tunnel_ip"] ?: ""
            return listOfNotNull(
                "$host:$port".takeIf { host.isNotBlank() },
                addr.takeIf { it.isNotBlank() },
            ).joinToString(" · ")
        }
}

object WireguardParser {

    private fun settingsOf(raw: Any?): Map<String, String> {
        @Suppress("UNCHECKED_CAST")
        val map = raw as? Map<String, Any?> ?: return emptyMap()
        return map.mapNotNull { (k, v) -> v?.let { k to it.toString() } }.toMap()
    }

    fun parseState(data: Map<String, Any?>) = WireguardState(
        available = data["available"] == true,
        configured = data["configured"] == true,
        connectStatus = data["connect_status"]?.toString().orEmpty(),
        settings = settingsOf(data["settings"]),
    )

    @Suppress("UNCHECKED_CAST")
    fun parseProfiles(data: Map<String, Any?>): List<WireguardProfile> {
        val list = data["profiles"] as? List<*> ?: return emptyList()
        return list.mapNotNull { item ->
            val p = item as? Map<String, Any?> ?: return@mapNotNull null
            val id = (p["id"] as? Number)?.toInt() ?: return@mapNotNull null
            WireguardProfile(
                id = id,
                name = p["name"]?.toString().orEmpty(),
                active = p["active"] == true,
                settings = settingsOf(p["settings"]),
            )
        }
    }

    /** The writable fields, in the same order the dashboard shows them. */
    val fields: List<Field> = listOf(
        Field("tunnel_ip", "Router address", "This end of the tunnel, e.g. 10.0.0.2/24"),
        Field("listen_port", "Listen port", null),
        Field("peer_public_key", "Peer public key", "The server's public key, base64"),
        Field("peer_endip", "Peer endpoint", "Host or IP of the server"),
        Field("peer_listen_port", "Peer port", null),
        Field("peer_tunnel_ip", "Peer tunnel address", null),
        Field("peer_remote_ip", "Allowed network", "Traffic routed into the tunnel"),
        Field("peer_remote_mask", "Allowed netmask", null),
    )

    /** Everything connect refuses to run without, so the UI can say why. */
    val requiredToConnect = listOf("peer_public_key", "tunnel_ip", "peer_endip")

    data class Field(val key: String, val label: String, val hint: String?)
}
