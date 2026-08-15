package com.openu60.core.model

import kotlin.math.abs
import kotlin.math.pow
import kotlin.math.roundToLong

data class BatteryStatus(
    val capacity: Int = 0,
    val temperature: Double = 0.0,
    val charging: String = "",
    val chargeStatus: Int = 0,
    val timeToFull: Int = -1,
    val timeToEmpty: Int = -1,
    val currentMA: Int? = null,
    val voltageMV: Int? = null,
) {
    companion object {
        val empty = BatteryStatus()
    }
}

data class ThermalStatus(
    val cpuTemp: Double = 0.0,
) {
    companion object {
        val empty = ThermalStatus()
    }
}

data class TrafficStats(
    val rxBytes: Long = 0,
    val txBytes: Long = 0,
    val timestamp: Long = System.currentTimeMillis(),
    val source: String = "",
    val precomputedRxRate: Double? = null,
    val precomputedTxRate: Double? = null,
    val serverRxSpeed: Double? = null,
    val serverTxSpeed: Double? = null,
) {
    companion object {
        val empty = TrafficStats()
    }
}

data class TrafficSpeed(
    val downloadBytesPerSec: Double = 0.0,
    val uploadBytesPerSec: Double = 0.0,
) {
    companion object {
        val zero = TrafficSpeed()
    }
}

data class ConnectedDevice(
    val id: String,
    val name: String,
    val ipAddress: String,
    val ip6Addresses: List<String>,
    val macAddress: String,
    val dhcpHostname: String,
    /** "wifi", "usb-c", "ethernet" — how the agent says this client is attached. */
    val medium: String = "",
    /** Wi-Fi only; null on a wired client, which is not the same as 0 dBm. */
    val signalDbm: Int? = null,
) {
    val displayName: String
        get() = when {
            dhcpHostname.isNotEmpty() -> dhcpHostname
            name.isNotEmpty() -> name
            else -> macAddress
        }
}

data class DeviceIdentity(
    val imei: String = "",
    val simICCID: String = "",
    val simIMSI: String = "",
    val msisdn: String = "",
    val wanIPv4: String = "",
    val wanIPv6: List<String> = emptyList(),
    val lanIP: String = "",
    val spn: String = "",
    val mcc: String = "",
    val mnc: String = "",
    val simStatus: String = "",
) {
    companion object {
        val empty = DeviceIdentity()
    }
}

data class WifiStatus(
    val wifiOn: Boolean = false,
    val ssid2g: String = "",
    val ssid5g: String = "",
    val channel2g: String = "",
    val channel5g: String = "",
    val radio2gDisabled: Boolean = false,
    val radio5gDisabled: Boolean = false,
    val encryption2g: String = "",
    val encryption5g: String = "",
    val hidden2g: Boolean = false,
    val hidden5g: Boolean = false,
    val txPower2g: String = "",
    val txPower5g: String = "",
    val bandwidth2g: String = "",
    val bandwidth5g: String = "",
    val clientsTotal: Int = 0,
    val wifi6: Boolean = false,
    val guestEnabled: Boolean = false,
    val guestSsid: String = "",
) {
    companion object {
        val empty = WifiStatus()
    }
}

data class CpuStatSample(
    val idle: Long,
    val total: Long,
)

data class SystemInfo(
    val cpuUsagePercent: Double = 0.0,
    val cpuUsageIsEstimate: Boolean = true,
    val cpuCores: Int = 1,
    val uptime: Int = 0,
    val memTotal: Long = 0,
    val memFree: Long = 0,
) {
    companion object {
        val empty = SystemInfo()
    }
}

data class USBStatus(
    val mode: String = "",
    val typecCC: String = "no_cc",
    val dataConnected: Boolean = false,
    val powerbankActive: Boolean = false,
) {
    val cableAttached: Boolean get() = typecCC != "no_cc"

    companion object {
        val empty = USBStatus()
    }
}

// MARK: - Parsers

object DeviceParser {

    fun parseBattery(data: Map<String, Any?>): BatteryStatus {
        return BatteryStatus(
            capacity = asInt(data["battery_capacity"]) ?: 0,
            temperature = asDouble(data["battery_temperature"]) ?: 0.0,
            charging = "",
            timeToFull = asInt(data["battery_time_to_full"]) ?: -1,
            timeToEmpty = asInt(data["battery_time_to_empty"]) ?: -1,
        )
    }

    fun parseCharger(data: Map<String, Any?>, battery: BatteryStatus, chargeControl: Map<String, Any?>? = null): BatteryStatus {
        val chargeStatus = asInt(data["charge_status"]) ?: 0
        val chargerConnected = asInt(data["charger_connect"]) == 1
        val chargingStopped = chargeControl?.get("charging_stopped") as? Boolean ?: false
        val charging = when {
            chargerConnected && chargingStopped -> "stopped"
            chargeStatus == 1 -> "charging"
            else -> "discharging"
        }
        return battery.copy(chargeStatus = chargeStatus, charging = charging)
    }

    fun parseThermal(data: Map<String, Any?>): ThermalStatus {
        return ThermalStatus(cpuTemp = asDouble(data["cpuss_temp"]) ?: 0.0)
    }

    // parseTraffic and parseWwandstTraffic were removed here.
    //
    // parseTraffic read data["statistics"] and had no callers at all.
    // parseWwandstTraffic read real_rx_bytes / real_rx_speed, which no route
    // on this agent returns, from a tier that could not be reached anyway.
    // Both showed up as keys nothing sends in the field-contract sweep, which
    // is what they were.

    fun computeSpeed(previous: TrafficStats, current: TrafficStats): TrafficSpeed {
        // Priority 1: server-computed speeds from zte-agent
        val sRx = current.serverRxSpeed
        val sTx = current.serverTxSpeed
        if (sRx != null && sTx != null) {
            return TrafficSpeed(downloadBytesPerSec = sRx, uploadBytesPerSec = sTx)
        }
        // Priority 2: pre-computed rates from ZTE daemon
        val pRx = current.precomputedRxRate
        val pTx = current.precomputedTxRate
        if (pRx != null && pTx != null) {
            return TrafficSpeed(downloadBytesPerSec = pRx, uploadBytesPerSec = pTx)
        }
        // Priority 3: client-side delta (skip when source changes)
        if (previous.source.isNotEmpty() && current.source.isNotEmpty() && previous.source != current.source) {
            return TrafficSpeed.zero
        }
        val elapsed = (current.timestamp - previous.timestamp) / 1000.0
        if (elapsed <= 0) return TrafficSpeed.zero
        val rxDelta = if (current.rxBytes > previous.rxBytes) current.rxBytes - previous.rxBytes else 0
        val txDelta = if (current.txBytes > previous.txBytes) current.txBytes - previous.txBytes else 0
        return TrafficSpeed(
            downloadBytesPerSec = rxDelta / elapsed,
            uploadBytesPerSec = txDelta / elapsed,
        )
    }

    /**
     * The clients the agent can see.
     *
     * This used to expect OpenWrt's `getHostHints` shape — a map keyed by MAC,
     * each value carrying `name`, `ipaddrs` and `ip6addrs`. The agent returns
     * `{"clients": [...]}` instead, so the loop iterated one entry whose value
     * was a list, failed the cast, and returned nothing: both the dashboard
     * and the Connected Devices screen read "No devices found" while a client
     * was plainly attached.
     *
     * The agent already joins the DHCP lease in, which is why there is no
     * separate enrichment step any more — `hostname` arrives with the row.
     */
    fun parseHostHints(data: Map<String, Any?>): List<ConnectedDevice> {
        val clients = data["clients"] as? List<*> ?: return emptyList()
        return clients.mapNotNull { item ->
            val info = item as? Map<*, *> ?: return@mapNotNull null
            val mac = info["mac"] as? String ?: return@mapNotNull null
            val ip = info["ip"] as? String ?: ""
            ConnectedDevice(
                id = mac,
                name = info["hostname"] as? String ?: "",
                ipAddress = ip,
                // No IPv6 field on the agent's client rows, so this stays
                // empty rather than reading a key nothing sends.
                ip6Addresses = emptyList(),
                macAddress = mac,
                // The agent's hostname is the DHCP lease, so it goes here as
                // well as in `name`: displayName prefers it, and a client with
                // no lease still shows its MAC rather than nothing.
                dhcpHostname = info["hostname"] as? String ?: "",
                medium = info["medium"] as? String ?: "",
                signalDbm = asInt(info["signal_dbm"]),
            )
        }.sortedWith(compareBy { it.ipAddress })
    }

    fun parseIdentity(
        simInfo: Map<String, Any?>,
        imeiData: Map<String, Any?>,
        wanStatus: Map<String, Any?>,
        wan6Status: Map<String, Any?>,
        lanStatus: Map<String, Any?>,
    ): DeviceIdentity {
        val wanIPv4 = (wanStatus["ipv4-address"] as? List<*>)
            ?.firstOrNull()?.let { (it as? Map<*, *>)?.get("address") as? String } ?: ""

        val wanIPv6 = (wan6Status["ipv6-address"] as? List<*>)
            ?.mapNotNull { entry ->
                val addr = (entry as? Map<*, *>)?.get("address") as? String
                addr?.takeIf { !it.startsWith("fe80") }
            } ?: emptyList()

        val lanIP = (lanStatus["ipv4-address"] as? List<*>)
            ?.firstOrNull()?.let { (it as? Map<*, *>)?.get("address") as? String } ?: ""

        val spnHex = simInfo["spn_name_data"] as? String
        val spn = if (spnHex != null) decodeSpn(spnHex) else ""

        return DeviceIdentity(
            imei = imeiData["imei"] as? String ?: "",
            simICCID = simInfo["sim_iccid"] as? String ?: "",
            simIMSI = simInfo["sim_imsi"] as? String ?: "",
            msisdn = simInfo["msisdn"] as? String ?: "",
            wanIPv4 = wanIPv4,
            wanIPv6 = wanIPv6,
            lanIP = lanIP,
            spn = spn,
            mcc = simInfo["mdm_mcc"] as? String ?: "",
            mnc = simInfo["mdm_mnc"] as? String ?: "",
            simStatus = simInfo["sim_states"] as? String ?: "",
        )
    }

    // MARK: - SPN Decoder

    fun decodeSpn(hex: String): String {
        val trimmed = hex.trim()
        if (trimmed.isEmpty() || trimmed.length % 4 != 0) return ""
        val sb = StringBuilder()
        var i = 0
        while (i + 3 < trimmed.length) {
            val code = trimmed.substring(i, i + 4).toIntOrNull(16)
            if (code != null && code != 0) {
                sb.append(code.toChar())
            }
            i += 4
        }
        return sb.toString()
    }

    // MARK: - USB Parser

    fun parseUSBStatus(usbData: Map<String, Any?>, chargerData: Map<String, Any?>?): USBStatus {
        return USBStatus(
            mode = usbData["mode"] as? String ?: "",
            typecCC = usbData["typec_cc"] as? String ?: "no_cc",
            dataConnected = asInt(usbData["connect"]) == 1,
            powerbankActive = asInt(chargerData?.get("otg_powerbank_state")) == 1,
        )
    }

    // MARK: - WiFi Parser

    /**
     * `/api/wifi/status`. The SSID keys are `ssid_2g` / `ssid_5g`.
     *
     * They were read as `main2g_ssid` / `main5g_ssid`, the upstream agent's
     * names, so the dashboard's Wi-Fi tile showed an empty network name while
     * the radio state beside it — read under the right names — was correct.
     * The same object feeds WiFiParser, which had the names right all along.
     */
    fun parseWifiStatus(data: Map<String, Any?>): WifiStatus {
        return WifiStatus(
            wifiOn = (data["wifi_onoff"] as? String) == "1",
            ssid2g = data["ssid_2g"] as? String ?: "",
            ssid5g = data["ssid_5g"] as? String ?: "",
            radio2gDisabled = (data["radio2_disabled"] as? String) == "1",
            radio5gDisabled = (data["radio5_disabled"] as? String) == "1",
        )
    }

    fun formatEncryption(raw: String): String = when (raw.lowercase()) {
        "psk2", "psk2+ccmp" -> "WPA2"
        "sae" -> "WPA3"
        "sae-mixed", "sae+psk2" -> "WPA2/3"
        "psk-mixed", "psk+psk2" -> "WPA/2"
        "psk" -> "WPA"
        "none", "" -> "Open"
        else -> raw.uppercase()
    }

    // MARK: - System Parser

    /**
     * System summary from `/api/device/system`, plus `/api/memory` if given.
     *
     * The key names here were the upstream agent's and none of them match this
     * one, which is why the dashboard showed an uptime of zero: it read
     * `uptime` where the agent returns `uptime_secs`, and `load` where it
     * returns `load_avg`.
     *
     * The two load spellings are not the same number. `load_avg` is already a
     * float — 2.42 means a load of 2.42 — while the procfs-style `load` array
     * upstream used is fixed point and has to be divided by 65536. Reading one
     * as the other is off by a factor of 65536, so they are handled apart
     * rather than merged with a fallback.
     */
    fun parseSystemInfo(
        data: Map<String, Any?>,
        cpuCores: Int = 1,
        memory: Map<String, Any?> = emptyMap(),
    ): SystemInfo {
        var cpuUsage = 0.0
        val loadAvg = (data["load_avg"] as? List<*>)?.firstOrNull()?.let { asDouble(it) }
            ?: (data["load"] as? List<*>)?.firstOrNull()?.let { asDouble(it)?.div(65536.0) }
        if (loadAvg != null) {
            cpuUsage = minOf(loadAvg / maxOf(cpuCores, 1).toDouble() * 100.0, 100.0)
        }
        val uptime = asInt(data["uptime_secs"]) ?: asInt(data["uptime"]) ?: 0
        // /api/memory reports kilobytes; the agent has no memory field on
        // /api/device/system at all, which is why this read zero for both.
        val memTotal = asLong(memory["total_kb"]) ?: asLong(data["memory_total"]) ?: 0
        val memFree = asLong(memory["available_kb"]) ?: asLong(memory["free_kb"])
            ?: asLong(data["memory_free"]) ?: 0
        return SystemInfo(
            cpuUsagePercent = cpuUsage,
            cpuUsageIsEstimate = true,
            cpuCores = cpuCores,
            uptime = uptime,
            memTotal = memTotal,
            memFree = memFree,
        )
    }

    // MARK: - WAN Parser

    fun parseWanIPv4(data: Map<String, Any?>): String {
        val ipv4Arr = data["ipv4-address"] as? List<*> ?: return ""
        val first = ipv4Arr.firstOrNull() as? Map<*, *> ?: return ""
        return first["address"] as? String ?: ""
    }

    fun parseWanIPv6(data: Map<String, Any?>): String {
        val ipv6Arr = data["ipv6-address"] as? List<*>
        if (ipv6Arr != null) {
            for (entry in ipv6Arr) {
                val addr = (entry as? Map<*, *>)?.get("address") as? String
                if (addr != null && !addr.startsWith("fe80")) return addr
            }
        }
        val ipv6Prefix = data["ipv6-prefix-assignment"] as? List<*>
        if (ipv6Prefix != null) {
            for (entry in ipv6Prefix) {
                val addr = (entry as? Map<*, *>)?.get("address") as? String
                if (addr != null && !addr.startsWith("fe80")) return addr
            }
        }
        return ""
    }

    // MARK: - Formatting

    data class FormattedValue(val number: Double, val unit: String, val decimalPlaces: Int)

    private fun adaptiveDecimals(value: Double): Int {
        val a = abs(value)
        return when {
            a < 10 -> 2
            a < 100 -> 1
            else -> 0
        }
    }

    private fun roundTo(v: Double, decimals: Int): Double {
        val factor = 10.0.pow(decimals)
        return (v * factor).roundToLong() / factor
    }

    fun speedComponents(bytesPerSec: Double): FormattedValue {
        val bits = bytesPerSec * 8.0
        val gb = 1_000_000_000.0
        val mb = 1_000_000.0
        val kb = 1_000.0
        val (raw, unit) = when {
            bits >= gb -> bits / gb to " Gb/s"
            bits >= mb -> bits / mb to " Mb/s"
            bits >= kb -> bits / kb to " Kb/s"
            else -> bits to " b/s"
        }
        return FormattedValue(number = roundTo(raw, 1), unit = unit, decimalPlaces = 1)
    }

    fun bytesComponents(bytes: Long): FormattedValue {
        val b = bytes.toDouble()
        val tb = 1024.0 * 1024.0 * 1024.0 * 1024.0
        val gb = 1024.0 * 1024.0 * 1024.0
        val mb = 1024.0 * 1024.0
        val kb = 1024.0
        val (raw, unit) = when {
            b >= tb -> b / tb to " TB"
            b >= gb -> b / gb to " GB"
            b >= mb -> b / mb to " MB"
            b >= kb -> b / kb to " KB"
            else -> b to " B"
        }
        val dp = adaptiveDecimals(raw)
        return FormattedValue(number = roundTo(raw, dp), unit = unit, decimalPlaces = dp)
    }

    fun formatSpeed(bytesPerSec: Double): String {
        val c = speedComponents(bytesPerSec)
        return "%.${c.decimalPlaces}f${c.unit.trim()}".format(c.number)
    }

    fun formatBytes(bytes: Long): String {
        val c = bytesComponents(bytes)
        return "%.${c.decimalPlaces}f${c.unit.trim()}".format(c.number)
    }

    // MARK: - Helpers

    fun asInt(value: Any?): Int? = when (value) {
        is Int -> value
        is Long -> value.toInt()
        is Double -> value.toInt()
        is String -> value.toIntOrNull()
        else -> null
    }

    fun asDouble(value: Any?): Double? = when (value) {
        is Double -> value
        is Int -> value.toDouble()
        is Long -> value.toDouble()
        is String -> value.toDoubleOrNull()
        else -> null
    }

    fun asLong(value: Any?): Long? = when (value) {
        is Long -> value
        is Int -> value.toLong()
        is Double -> value.toLong()
        is String -> value.toLongOrNull()
        else -> null
    }

    fun asBool(value: Any?): Boolean = when (value) {
        is Boolean -> value
        is String -> value == "1" || value.lowercase() == "true" || value.lowercase() == "on"
        is Int -> value != 0
        else -> false
    }
}

// MARK: - Process Monitor

data class ProcessInfo(
    val pid: Int,
    val name: String,
    val cpuPct: Double,
    val rssKb: Long,
    val state: String,
    val isBloat: Boolean,
)

data class ProcessListResponse(
    val processes: List<ProcessInfo>,
    val totalCount: Int,
    val bloatCount: Int,
    val bloatCpuPct: Double,
    val bloatRssKb: Long,
)

data class KilledProcess(
    val pid: Int,
    val name: String,
)

data class KillBloatResponse(
    val killed: List<KilledProcess>,
    val skipped: List<KilledProcess>,
    val freedRssKb: Long,
)
