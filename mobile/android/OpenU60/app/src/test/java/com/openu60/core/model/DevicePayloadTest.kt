package com.openu60.core.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Dashboard parsers against live `/api/device/system`, `/api/memory`,
 * `/api/wifi/status` and `/api/network/clients` payloads.
 *
 * Every one of these read a key the agent does not send, and every one failed
 * silently: an uptime of zero, a blank network name, and an empty client list
 * while a machine was plainly attached. None of it raised an error, which is
 * why it survived so long.
 */
class DevicePayloadTest {

    @Test
    fun `clients parse from the agent's shape, not OpenWrt host hints`() {
        // GET /api/network/clients, captured live.
        val payload = mapOf<String, Any?>(
            "clients" to listOf(
                mapOf(
                    "connected_secs" to null,
                    "hostname" to "RAZR93-PC",
                    "interface" to "ecm0",
                    "ip" to "192.168.0.178",
                    "mac" to "b8:d4:bc:ed:95:00",
                    "medium" to "usb-c",
                    "signal_dbm" to null,
                ),
            ),
        )
        val devices = DeviceParser.parseHostHints(payload)
        assertEquals(1, devices.size)
        assertEquals("192.168.0.178", devices[0].ipAddress)
        assertEquals("b8:d4:bc:ed:95:00", devices[0].macAddress)
        assertEquals("RAZR93-PC", devices[0].displayName)
        assertEquals("usb-c", devices[0].medium)
        // A wired client has no signal; null is not the same claim as 0 dBm.
        assertNull(devices[0].signalDbm)
    }

    @Test
    fun `the old host-hint shape would have found nothing`() {
        // What the parser used to expect. Kept as a test because the failure
        // was invisible: no error, just "No devices found".
        val hostHints = mapOf<String, Any?>(
            "b8:d4:bc:ed:95:00" to mapOf("name" to "RAZR93-PC", "ipaddrs" to listOf("192.168.0.178")),
        )
        assertTrue(DeviceParser.parseHostHints(hostHints).isEmpty())
    }

    @Test
    fun `an empty client list is empty rather than a crash`() {
        assertTrue(DeviceParser.parseHostHints(mapOf("clients" to emptyList<Any>())).isEmpty())
        assertTrue(DeviceParser.parseHostHints(emptyMap()).isEmpty())
    }

    @Test
    fun `wifi tile reads the ssid keys the agent sends`() {
        val payload = mapOf<String, Any?>(
            "wifi_onoff" to "1",
            "ssid_2g" to "ZTE_ED9300",
            "ssid_5g" to "ZTE_ED9300",
            "radio2_disabled" to "0",
            "radio5_disabled" to "0",
        )
        val status = DeviceParser.parseWifiStatus(payload)
        assertEquals("ZTE_ED9300", status.ssid2g)
        assertEquals("ZTE_ED9300", status.ssid5g)
        assertTrue(status.wifiOn)
    }

    @Test
    fun `the upstream ssid names would have left the tile blank`() {
        val upstream = mapOf<String, Any?>("main2g_ssid" to "ZTE_ED9300", "wifi_onoff" to "1")
        assertEquals("", DeviceParser.parseWifiStatus(upstream).ssid2g)
    }

    @Test
    fun `uptime and load come from the keys this agent uses`() {
        // GET /api/device/system, captured live.
        val system = mapOf<String, Any?>(
            "hostname" to "OpenWrt",
            "uptime_secs" to 89055,
            "load_avg" to listOf(2.3, 2.67, 2.71),
        )
        val info = DeviceParser.parseSystemInfo(system, cpuCores = 4)
        assertEquals(89055, info.uptime)
        // 2.3 over 4 cores is 57.5% — the fixed-point reading would divide by
        // another 65536 and report 0.
        assertEquals(57.5, info.cpuUsagePercent, 0.1)
    }

    @Test
    fun `the fixed-point load spelling is still handled, and not confused`() {
        // The upstream agent reported load as procfs fixed point. 2.3 in that
        // form is 2.3 * 65536.
        val system = mapOf<String, Any?>("load" to listOf(150732), "uptime" to 100)
        val info = DeviceParser.parseSystemInfo(system, cpuCores = 4)
        assertEquals(100, info.uptime)
        assertEquals(57.5, info.cpuUsagePercent, 0.5)
    }

    @Test
    fun `memory comes from its own route and is kilobytes`() {
        // /api/device/system carries no memory at all, which is why both
        // figures used to be zero.
        val memory = mapOf<String, Any?>(
            "total_kb" to 1628400,
            "available_kb" to 818228,
            "free_kb" to 340752,
        )
        val info = DeviceParser.parseSystemInfo(emptyMap(), cpuCores = 4, memory = memory)
        assertEquals(1628400L, info.memTotal)
        // Available, not free: free excludes the cache the kernel would hand
        // back on demand, and reads as far less memory than there is.
        assertEquals(818228L, info.memFree)
    }

    @Test
    fun `no memory route means zero rather than a wrong number`() {
        val info = DeviceParser.parseSystemInfo(mapOf("uptime_secs" to 10), cpuCores = 1)
        assertEquals(0L, info.memTotal)
    }
}
