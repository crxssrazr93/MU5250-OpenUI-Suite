package com.openu60.core.model

/** Whether the slot holds a card, and whether that card is an eUICC. */
data class EuiccStatus(
    val cardPresent: Boolean,
    val euiccAvailable: Boolean,
    val detail: String,
) {
    companion object {
        val empty = EuiccStatus(cardPresent = false, euiccAvailable = false, detail = "")
    }
}

data class EuiccProfile(
    val iccid: String?,
    val isdpAid: String?,
    /** "enabled", "disabled" or "unknown". */
    val state: String,
    val enabled: Boolean,
    /** "operational", "provisioning", "test" or "unknown". */
    val profileClass: String,
    val nickname: String?,
    val serviceProvider: String?,
    val name: String?,
) {
    /** What to call this profile in a list, in the order a user would expect. */
    val displayName: String
        get() = nickname?.takeIf { it.isNotBlank() }
            ?: name?.takeIf { it.isNotBlank() }
            ?: serviceProvider?.takeIf { it.isNotBlank() }
            ?: "Unnamed profile"

    /** True when the agent returned this masked, so it cannot be used as a key. */
    val iccidIsMasked: Boolean get() = iccid?.contains('*') == true
}

/** A pending RSP notification the card wants delivered to an operator. */
data class EuiccNotification(
    val seqNumber: Int,
    val iccid: String,
    val notificationAddress: String,
    val profileManagementOperation: String,
)

/**
 * Result of an lpac-backed write.
 *
 * [rebootRequired] is set when the card switched but the modem did not: this
 * modem rejects ES10c EnableProfile's refresh flag, so a switch only takes
 * effect on reboot. A client that does not surface [notice] shows a successful
 * operation with no visible change, which reads as a failure.
 */
data class EuiccOperation(
    val progress: List<String>,
    val rebootRequired: Boolean,
    val notice: String?,
)

/** Chip details, as far as this app reads them. */
data class EuiccChipInfo(
    val firmwareVersion: String?,
    val profileVersion: String?,
    val freeNonVolatileMemory: Long?,
    val sasAccreditationNumber: String?,
    val defaultSmdpAddress: String?,
    val rootDsAddress: String?,
)

/**
 * A download request, in either of the two forms a user can supply.
 *
 * Either the activation code parts, or the SM-DP+ address and matching ID
 * typed in by hand — the same split EasyLPAC uses, because it matches what
 * people actually have: a QR code, or an operator email full of fields.
 */
data class EuiccDownloadRequest(
    val smdp: String,
    val matchingId: String,
    val confirmationCode: String? = null,
    val imei: String? = null,
    /** Have this phone carry the ES9+ traffic, for a router with no WAN. */
    val useRelay: Boolean = false,
) {
    fun toBody(): Map<String, Any?> = buildMap {
        put("smdp", smdp)
        put("matching_id", matchingId)
        confirmationCode?.takeIf { it.isNotBlank() }?.let { put("confirmation_code", it) }
        imei?.takeIf { it.isNotBlank() }?.let { put("imei", it) }
        put("relay", useRelay)
    }
}

/** Parsed GSMA SGP.22 §4.1 activation code. */
data class ActivationCode(
    val smdp: String,
    val matchingId: String,
    val smdpOid: String? = null,
    val confirmationCodeRequired: Boolean = false,
) {
    companion object {
        /**
         * Pull an activation code out of whatever was scanned or pasted.
         *
         * Accepts the bare `LPA:1$...` form, the same without the `LPA:`
         * scheme (some QR codes omit it), and the universal links operators
         * hand out — Apple's `?carddata=` and Android's `?data=`, which is
         * what you get from copying the link out of a camera app rather than
         * the code itself.
         */
        fun parse(raw: String): ActivationCode? {
            var text = raw.trim()
            if (text.isEmpty()) return null

            if (text.startsWith("http://", true) || text.startsWith("https://", true)) {
                val query = runCatching { android.net.Uri.parse(text) }.getOrNull() ?: return null
                val embedded = query.getQueryParameter("carddata")
                    ?: query.getQueryParameter("data")
                    ?: query.getQueryParameter("lpa")
                    ?: return null
                text = embedded.trim()
            }

            if (text.startsWith("LPA:", true)) text = text.substring(4).trim()

            val parts = text.split("$")
            // Format version, SM-DP+ address, matching ID, then two optional fields.
            if (parts.size < 3 || parts[0] != "1") return null

            val smdp = parts[1].trim()
            val matchingId = parts[2].trim()
            if (smdp.isEmpty() || matchingId.isEmpty()) return null

            return ActivationCode(
                smdp = smdp,
                matchingId = matchingId,
                smdpOid = parts.getOrNull(3)?.trim()?.takeIf { it.isNotEmpty() },
                // "1" is the only value meaning required; anything else, including
                // an absent field, means it is not.
                confirmationCodeRequired = parts.getOrNull(4)?.trim() == "1",
            )
        }
    }
}

/** Reads the agent's eUICC payloads into the models above. */
object EuiccParser {

    fun parseStatus(data: Map<String, Any?>) = EuiccStatus(
        cardPresent = data["card_present"] == true,
        euiccAvailable = data["euicc_available"] == true,
        detail = data["detail"]?.toString().orEmpty(),
    )

    fun parseEid(data: Map<String, Any?>): String? = data["eid"]?.toString()

    @Suppress("UNCHECKED_CAST")
    fun parseProfiles(data: Map<String, Any?>): List<EuiccProfile> {
        val list = data["profiles"] as? List<*> ?: return emptyList()
        return list.mapNotNull { item ->
            val p = item as? Map<String, Any?> ?: return@mapNotNull null
            EuiccProfile(
                iccid = p["iccid"]?.toString(),
                isdpAid = p["isdp_aid"]?.toString(),
                state = p["state"]?.toString() ?: "unknown",
                enabled = p["enabled"] == true,
                profileClass = p["class"]?.toString() ?: "unknown",
                nickname = p["nickname"]?.toString(),
                serviceProvider = p["service_provider"]?.toString(),
                name = p["name"]?.toString(),
            )
        }
    }

    @Suppress("UNCHECKED_CAST")
    fun parseNotifications(data: Map<String, Any?>): List<EuiccNotification> {
        val list = data["result"] as? List<*> ?: return emptyList()
        return list.mapNotNull { item ->
            val n = item as? Map<String, Any?> ?: return@mapNotNull null
            val seq = (n["seqNumber"] as? Number)?.toInt() ?: return@mapNotNull null
            EuiccNotification(
                seqNumber = seq,
                iccid = n["iccid"]?.toString().orEmpty(),
                notificationAddress = n["notificationAddress"]?.toString().orEmpty(),
                profileManagementOperation = n["profileManagementOperation"]?.toString().orEmpty(),
            )
        }
    }

    @Suppress("UNCHECKED_CAST")
    fun parseOperation(data: Map<String, Any?>) = EuiccOperation(
        progress = (data["progress"] as? List<*>).orEmpty().map { it.toString() },
        rebootRequired = data["reboot_required"] == true,
        notice = data["notice"]?.toString(),
    )

    @Suppress("UNCHECKED_CAST")
    fun parseChipInfo(data: Map<String, Any?>): EuiccChipInfo {
        val result = data["result"] as? Map<String, Any?> ?: emptyMap()
        val info2 = result["EUICCInfo2"] as? Map<String, Any?> ?: emptyMap()
        // lpac decodes extCardResource into byte counts rather than leaving it
        // as the raw BER-TLV string; confirmed against the card.
        val resource = info2["extCardResource"] as? Map<String, Any?> ?: emptyMap()
        val addresses = result["EuiccConfiguredAddresses"] as? Map<String, Any?> ?: emptyMap()
        return EuiccChipInfo(
            firmwareVersion = info2["euiccFirmwareVer"]?.toString(),
            profileVersion = info2["profileVersion"]?.toString(),
            freeNonVolatileMemory = (resource["freeNonVolatileMemory"] as? Number)?.toLong(),
            sasAccreditationNumber = info2["sasAcreditationNumber"]?.toString(),
            defaultSmdpAddress = addresses["defaultDpAddress"]?.toString(),
            rootDsAddress = addresses["rootDsAddress"]?.toString(),
        )
    }
}
