package com.openu60.core.model

import com.openu60.feature.bandlock.BandLockViewModel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Parsers against payloads this agent actually returns.
 *
 * Each fixture below was captured from a live agent on
 * `XCBZ_HK_MU5250V1.0.0B04`. The bug class these guard is a parser reading a
 * key nothing sends: the call succeeds, the screen renders, and every field is
 * blank or false. It is invisible to a path-level contract check, which is why
 * the firewall screen reported every switch as off regardless of the router's
 * configuration.
 */
class RouterPayloadTest {

    // GET /api/firewall/config, captured live.
    private val firewallPayload = mapOf<String, Any?>(
        "dmz_enabled" to false,
        "dmz_ip" to "",
        "filter_policy" to "ACCEPT",
        "firewall_enabled" to true,
        "mac_ip_port_filter_enabled" to false,
        "nat_enabled" to true,
        "port_forward_enabled" to false,
        "port_mapping_enabled" to false,
        "remote_admin_enabled" to false,
        "wan_ping_enabled" to false,
    )

    @Test
    fun `firewall config reads the keys the agent emits`() {
        val config = FirewallParser.parseConfig(firewallPayload)
        assertTrue("firewall is on in the fixture", config.enabled)
        assertTrue("NAT is on in the fixture", config.nat)
        assertFalse(config.portForwardEnabled)
        assertFalse(config.portMappingEnabled)
        assertFalse(config.remoteAdminEnabled)
        assertFalse(config.wanPingEnabled)
    }

    @Test
    fun `the old upstream key names would have read everything as off`() {
        // Kept as a test rather than a comment: this is what the screen was
        // doing, and it is the exact shape of the failure to watch for.
        val upstreamShaped = mapOf<String, Any?>(
            "firewall_switch" to true,
            "nat_switch" to true,
        )
        val config = FirewallParser.parseConfig(upstreamShaped)
        assertFalse("no agent sends firewall_switch", config.enabled)
        assertFalse("no agent sends nat_switch", config.nat)
    }

    @Test
    fun `an empty port forward list is empty, not a crash`() {
        // What the live agent returns with nothing configured.
        assertTrue(
            FirewallParser.parsePortForwardRules(mapOf("rules" to emptyMap<String, Any?>()))
                .isEmpty(),
        )
        assertTrue(FirewallParser.parsePortForwardRules(emptyMap()).isEmpty())
    }

    @Test
    fun `port forward rules parse whether the vendor sends a list or a map`() {
        val rule = mapOf(
            "section_id" to "cfg0392b1",
            "comment" to "ssh",
            "proto" to "TCP",
            "src_dport" to "2222",
            "dest_ip" to "192.168.0.50",
            "dest_port" to "22",
            "enabled" to "1",
        )
        val fromList = FirewallParser.parsePortForwardRules(mapOf("rules" to listOf(rule)))
        val fromMap = FirewallParser.parsePortForwardRules(
            mapOf("rules" to mapOf("cfg0392b1" to rule)),
        )
        for (parsed in listOf(fromList, fromMap)) {
            assertEquals(1, parsed.size)
            assertEquals("cfg0392b1", parsed[0].id)
            assertEquals("ssh", parsed[0].name)
            assertEquals("2222", parsed[0].wanPort)
            assertEquals("192.168.0.50", parsed[0].lanIP)
            assertTrue(parsed[0].enabled)
        }
    }

    @Test
    fun `domain filter rules read fqdn, not domain`() {
        val parsed = TelemetryParser.parseDomainFilter(
            mapOf(
                "rules" to listOf(
                    mapOf("section_id" to "cfg01", "fqdn" to "dclient.ztems.com", "enable" to "1"),
                ),
            ),
        )
        assertEquals(1, parsed.rules.size)
        assertEquals("dclient.ztems.com", parsed.rules[0].domain)
        assertTrue(parsed.rules[0].enabled)
    }

    @Test
    fun `schedule reboot reads a scheduler job, not a vendor setting`() {
        // GET /api/device/schedule-reboot returns the agent's own jobs.
        val payload = mapOf<String, Any?>(
            "jobs" to listOf(
                mapOf(
                    "id" to 3,
                    "name" to "Scheduled reboot",
                    "action" to "reboot",
                    "time" to "04:15",
                    "days" to listOf(0, 3),
                    "enabled" to true,
                ),
            ),
        )
        val config = ScheduleRebootParser.parse(payload)
        assertEquals(3, config.id)
        assertEquals("04:15", config.time)
        assertEquals("0,3", config.days)
        assertTrue(config.enabled)
    }

    @Test
    fun `no scheduled reboot is the empty config, not a default time`() {
        val config = ScheduleRebootParser.parse(mapOf("jobs" to emptyList<Any?>()))
        assertEquals(null, config.id)
        assertFalse(config.enabled)
    }

    @Test
    fun `LTE band lock decodes from the vendor bitmask`() {
        // Band N is bit N-1. 133 = 1 + 4 + 128 = bands 1, 3 and 8.
        assertEquals(setOf(1, 3, 8), BandLockViewModel.lteBandsFromMask("133"))
        assertEquals(emptySet<Int>(), BandLockViewModel.lteBandsFromMask("0"))
        assertEquals(emptySet<Int>(), BandLockViewModel.lteBandsFromMask(null))
        assertEquals(emptySet<Int>(), BandLockViewModel.lteBandsFromMask(""))
    }

    @Test
    fun `a band lock above bit 63 still decodes`() {
        // B66 needs bit 65, which is why this cannot be a Long.
        assertEquals(setOf(66), BandLockViewModel.lteBandsFromMask("36893488147419103232"))
    }

    @Test
    fun `a malformed mask is no lock rather than an exception`() {
        assertEquals(emptySet<Int>(), BandLockViewModel.lteBandsFromMask("not-a-number"))
    }

    @Test
    fun `NR band lock decodes from the comma list`() {
        assertEquals(setOf(1, 3, 78), BandLockViewModel.nrBandsFromList("1,3,78"))
        assertEquals(setOf(78), BandLockViewModel.nrBandsFromList("n78"))
        assertEquals(emptySet<Int>(), BandLockViewModel.nrBandsFromList(""))
    }

    @Test
    fun `wifi status is the read side and its keys match the parser`() {
        // GET /api/wifi/status, captured live with the key redacted.
        val payload = mapOf<String, Any?>(
            "ssid_2g" to "ZTE_ED9300",
            "ssid_5g" to "ZTE_ED9300",
            "key_2g" to "redacted",
            "key_5g" to "redacted",
            "channel_2g" to "0",
            "channel_5g" to "0",
            "txpower_2g" to "100",
            "txpower_5g" to "100",
            "encryption_2g" to "sae-mixed",
            "encryption_5g" to "sae-mixed",
            "wifi_onoff" to "1",
            "hidden_2g" to "0",
            "hidden_5g" to "0",
            "radio2_disabled" to "0",
            "radio5_disabled" to "0",
            "htmode_2g" to "EHT40",
            "htmode_5g" to "EHT160",
        )
        val config = WiFiParser.parse(payload)
        assertEquals("ZTE_ED9300", config.ssid2g)
        assertEquals("sae-mixed", config.encryption5g)
        assertTrue(config.wifiOnOff)
        assertFalse(config.radio2gDisabled)
    }

    @Test
    fun `the band lock mask arrives hex-prefixed on this firmware`() {
        // lte_band_lock reads "0x87e29a0e00df" live. BigInteger("0x…") throws,
        // so decimal-only parsing reported no lock on a unit that had one.
        val bands = BandLockViewModel.lteBandsFromMask("0x87e29a0e00df")
        assertTrue("expected B1 in the mask", 1 in bands)
        assertTrue("expected B48 in the mask", 48 in bands)
        assertFalse("B66 is not set in this mask", 66 in bands)
        assertEquals(22, bands.size)
        // Decimal still works; the setter uses it.
        assertEquals(setOf(1, 3, 8), BandLockViewModel.lteBandsFromMask("133"))
        assertEquals(emptySet<Int>(), BandLockViewModel.lteBandsFromMask("0x0"))
    }

    @Test
    fun `cell lock reads the keys the firmware reports`() {
        // From a live /api/modem/cell-lock: PCI is a number, the lock state
        // lives in lock_lte_cell, and cell_lock_status does not exist.
        val unlocked = CellLockParser.parse(
            mapOf("lte_pci" to 389, "lock_lte_cell" to "", "lock_nr_cell" to ""),
        )
        assertEquals("389", unlocked.ltePCI)
        assertFalse(unlocked.locked)

        val locked = CellLockParser.parse(
            mapOf("lte_pci" to 389, "lock_lte_cell" to "389,1725", "lock_nr_cell" to ""),
        )
        assertTrue(locked.locked)
    }

    @Test
    fun `neighbours parse per RAT and stay positional`() {
        val payload = mapOf<String, Any?>(
            "lte" to listOf(mapOf("fields" to listOf("227", "1725", "-99", "-11"))),
            "nr" to emptyList<Any>(),
        )
        val lte = CellLockParser.parseNeighbors(payload, "lte")
        assertEquals(1, lte.size)
        assertEquals("227  1725  -99  -11", lte[0].display)
        assertTrue(CellLockParser.parseNeighbors(payload, "nr").isEmpty())
        // The old key matched nothing, which is why the list was always empty.
        assertTrue(CellLockParser.parseNeighbors(payload, "neighbor").isEmpty())
    }
}
