package com.openu60.core.model

import java.util.UUID

// MARK: - DNS

data class DNSConfig(
    val wanDnsMode: String = "",
    val primaryDns: String = "",
    val secondaryDns: String = "",
    val ipv6PrimaryDns: String = "",
    val ipv6SecondaryDns: String = "",
    val ipv6DnsMode: String = "",
) {
    val isManual: Boolean get() = wanDnsMode == "manual"

    companion object {
        val empty = DNSConfig()
    }
}

object DNSParser {
    /**
     * IPv6 DNS is write-only on this firmware.
     *
     * `router_get_dns_para` returns three keys and no v6 among them, so the v6
     * boxes cannot be filled from the router and are left empty deliberately.
     * The setter does take `ipv6_wan_prefer_dns_manual` and its standby, which
     * is why the fields are still offered — you can set them, you just cannot
     * read them back, and the screen says so rather than showing blanks that
     * look like a failed load.
     */
    fun parse(data: Map<String, Any?>): DNSConfig {
        return DNSConfig(
            wanDnsMode = data["dns_mode"] as? String ?: "",
            primaryDns = data["prefer_dns_manual"] as? String ?: "",
            secondaryDns = data["standby_dns_manual"] as? String ?: "",
        )
    }
}

// MARK: - Firewall

data class FirewallConfig(
    val enabled: Boolean = false,
    val nat: Boolean = false,
    val dmzEnabled: Boolean = false,
    val dmzHost: String = "",
    val wanPingEnabled: Boolean = false,
    val remoteAdminEnabled: Boolean = false,
    val portForwardEnabled: Boolean = false,
    val portMappingEnabled: Boolean = false,
) {
    companion object {
        val empty = FirewallConfig()
    }
}

data class PortForwardRule(
    val id: String = "",
    val name: String = "",
    val protocol: String = "",
    val wanPort: String = "",
    val lanIP: String = "",
    val lanPort: String = "",
    val enabled: Boolean = false,
)

data class FilterRule(
    val id: String = "",
    val srcMac: String = "",
    val srcIP: String = "",
    val srcPort: String = "",
    val destIP: String = "",
    val destPort: String = "",
    val protocol: String = "",
    val enabled: Boolean = false,
)

object FirewallParser {
    // Key names taken from a live GET /api/firewall/config, not from upstream:
    // this parser used to read firewall_switch, nat_switch and firewall_level,
    // none of which the agent emits, so every switch on the screen read false
    // no matter how the router was configured. `level` is gone with them — the
    // firmware returns it empty, so there was never a level to show.
    fun parseConfig(data: Map<String, Any?>): FirewallConfig {
        return FirewallConfig(
            enabled = DeviceParser.asBool(data["firewall_enabled"]),
            nat = DeviceParser.asBool(data["nat_enabled"]),
            dmzEnabled = DeviceParser.asBool(data["dmz_enabled"]),
            dmzHost = data["dmz_ip"] as? String ?: "",
            wanPingEnabled = DeviceParser.asBool(data["wan_ping_enabled"]),
            remoteAdminEnabled = DeviceParser.asBool(data["remote_admin_enabled"]),
            portForwardEnabled = DeviceParser.asBool(data["port_forward_enabled"]),
            portMappingEnabled = DeviceParser.asBool(data["port_mapping_enabled"]),
        )
    }

    /**
     * The agent returns the vendor's reply under `rules`, unreshaped.
     *
     * It is `{}` with nothing configured, which is the only state this has been
     * seen in — adding a rule to a live router to learn the populated shape
     * opens an inbound path, so it was not done. Both a list and a map of
     * sections are therefore accepted, and unknown field names leave a blank
     * rather than dropping the rule, so a rule that exists is always listed.
     */
    fun parsePortForwardRules(data: Map<String, Any?>): List<PortForwardRule> {
        val container = data["rules"] ?: data["rule_list"]
        val items: List<Any?> = when (container) {
            is List<*> -> container
            is Map<*, *> -> container.values.toList()
            else -> return emptyList()
        }
        return items.mapIndexedNotNull { index, item ->
            val rule = item as? Map<*, *> ?: return@mapIndexedNotNull null
            fun text(vararg keys: String): String =
                keys.firstNotNullOfOrNull { rule[it] as? String } ?: ""
            PortForwardRule(
                id = text("section_id", "id", ".name").ifBlank { "$index" },
                name = text("comment", "name"),
                protocol = text("proto", "protocol"),
                wanPort = text("src_dport", "wan_port"),
                lanIP = text("dest_ip", "lan_ip"),
                lanPort = text("dest_port", "lan_port"),
                enabled = DeviceParser.asBool(rule["enabled"] ?: rule["enable"]),
            )
        }
    }

    fun parseFilterRules(data: Map<String, Any?>): List<FilterRule> {
        val rules = data["rule_list"] as? List<*> ?: return emptyList()
        return rules.mapIndexed { index, item ->
            val rule = item as? Map<*, *> ?: return@mapIndexed null
            FilterRule(
                id = rule["id"] as? String ?: "$index",
                srcMac = rule["src_mac"] as? String ?: "",
                srcIP = rule["src_ip"] as? String ?: "",
                srcPort = rule["src_port"] as? String ?: "",
                destIP = rule["dest_ip"] as? String ?: "",
                destPort = rule["dest_port"] as? String ?: "",
                protocol = rule["protocol"] as? String ?: "",
                enabled = DeviceParser.asBool(rule["enabled"]),
            )
        }.filterNotNull()
    }
}

// MARK: - Telemetry / Domain Filter

data class DomainFilterConfig(
    val enabled: Boolean = false,
    val rules: List<DomainFilterRule> = emptyList(),
) {
    companion object {
        val empty = DomainFilterConfig()
    }
}

data class DomainFilterRule(
    val id: String = "",
    val domain: String = "",
    val enabled: Boolean = false,
)

object TelemetryParser {
    /**
     * The agent returns the vendor's reply under `rules`, unreshaped.
     *
     * `{}` when nothing is filtered, which is the only state observed. As with
     * the port-forward list, both a list and a map of sections are accepted and
     * an unrecognised field leaves a blank rather than dropping the rule.
     *
     * There is no filter-wide enable to read: the vendor has `dnsquery_action`
     * and `dnsquery_target` per rule and no master switch, so `enabled` here
     * means "at least one rule exists" rather than a setting.
     */
    fun parseDomainFilter(data: Map<String, Any?>): DomainFilterConfig {
        val container = data["rules"] ?: data["rule_list"]
        val items: List<Any?> = when (container) {
            is List<*> -> container
            is Map<*, *> -> container.values.toList()
            else -> emptyList()
        }
        val rules = items.mapIndexedNotNull { index, item ->
            val rule = item as? Map<*, *> ?: return@mapIndexedNotNull null
            fun text(vararg keys: String): String =
                keys.firstNotNullOfOrNull { rule[it] as? String } ?: ""
            DomainFilterRule(
                id = text("section_id", "id", ".name").ifBlank { "$index" },
                domain = text("fqdn", "domain"),
                enabled = DeviceParser.asBool(rule["enable"] ?: rule["enabled"]),
            )
        }
        return DomainFilterConfig(enabled = rules.isNotEmpty(), rules = rules)
    }

    val knownTelemetryDomains = listOf(
        "dclient.ztems.com",
        "dconfig.ztems.com",
        "iot.ztems.com",
        "mcs-cloud.ztems.com",
        "update.ztems.com",
    )
}

// MARK: - DoH (DNS-over-HTTPS)

data class DoHStatus(
    val enabled: Boolean = false,
    val upstreamUrl: String = "",
    val cacheEntries: Int = 0,
    val cacheHits: Int = 0,
    val cacheMisses: Int = 0,
    val queriesTotal: Int = 0,
) {
    val hitRatio: Double
        get() {
            val total = cacheHits + cacheMisses
            return if (total > 0) cacheHits.toDouble() / total * 100.0 else 0.0
        }

    companion object {
        val empty = DoHStatus()
    }
}

data class DoHCacheEntry(
    val domain: String = "",
    val type: String = "?",
    val typeId: Int = 0,
    val ttl: Int = 0,
) {
    val id: String get() = "$domain-$typeId"
}

object DoHParser {
    fun parse(data: Map<String, Any?>): DoHStatus {
        val running = data["running"] as? Boolean ?: false
        val config = data["config"] as? Map<*, *> ?: emptyMap<String, Any>()
        val stats = data["stats"] as? Map<*, *> ?: emptyMap<String, Any>()
        return DoHStatus(
            enabled = running,
            upstreamUrl = config["upstream_url"] as? String ?: "",
            cacheEntries = DeviceParser.asInt(stats["cache_entries"]) ?: 0,
            cacheHits = DeviceParser.asInt(stats["cache_hits"]) ?: 0,
            cacheMisses = DeviceParser.asInt(stats["cache_misses"]) ?: 0,
            queriesTotal = DeviceParser.asInt(stats["queries_total"]) ?: 0,
        )
    }

    fun parseCacheEntries(list: List<Map<String, Any?>>): List<DoHCacheEntry> {
        return list.map { entry ->
            DoHCacheEntry(
                domain = entry["domain"] as? String ?: "",
                type = entry["type"] as? String ?: "?",
                typeId = DeviceParser.asInt(entry["type_id"]) ?: 0,
                ttl = DeviceParser.asInt(entry["ttl"]) ?: 0,
            )
        }
    }
}
