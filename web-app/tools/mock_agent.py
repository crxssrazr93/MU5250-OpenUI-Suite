#!/usr/bin/env python3
"""Mock zte-agent for local dashboard demos.

Serves realistic U60 Pro data on :9090 so the dashboard can be reviewed
without the device. Read endpoints return plausible values (with a little
live jitter); mutating endpoints just succeed.

Usage:  python3 web-app/tools/mock_agent.py [--port 9090]
"""
import argparse
import json
import random
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

BOOT = time.time()


def uptime():
    return int(time.time() - BOOT) + 384200  # pretend ~4.4 days of uptime


def jitter(base, pct=0.15):
    # No clamp: RSRP/RSRQ are negative dBm; positive bases stay positive
    # because (1 ± pct) never crosses zero for pct < 1.
    return base * (1 + random.uniform(-pct, pct))


# ── Fixture builders ──────────────────────────────────────────────────────────

def signal_raw():
    return {
        "network_type": "ENDC",
        "network_provider_fullname": "Telstra",
        "network_provider": "Telstra",
        "signalbar": "4",
        "cell_id": 134479973,
        "net_select": "WL_AND_5G",
        # LTE PCC: B8, EARFCN 3650, PCI 312, 20 MHz
        "lte_pci": 312,
        "wan_active_channel": 3650,
        "wan_active_band": "B8",
        "lte_rsrp": str(int(jitter(-71, 0.04))),
        "lte_rsrq": "-10.8",
        "lte_snr": "19.5",
        "lte_rssi": "-62",
        # LTE CA: PCC + one SCC (B1, EARFCN 300, PCI 314, 15 MHz)
        "lteca": "312,8,1,3650,20,1;314,1,1,300,15,1",
        "ltecasig": "-79,-12.1,14.0,-71,1,2",
        # NR PCC: n78, ARFCN 630912, PCI 801, 100 MHz
        "nr5g_rsrp": str(int(jitter(-77, 0.04))),
        "nr5g_action_band": "n78",
        "nr5g_action_channel": 630912,
        "nr5g_pci": 801,
        "nr5g_bandwidth": "100",
        "nr5g_snr": "23.5",
        "nr5g_rsrq": "-9.2",
        "nr5g_rssi": "-58",
        "nr5g_cell_id": 268566611,
        # NR CA: one SCC (n40, ARFCN 472000, PCI 803, 40 MHz).
        # Indices: 0=ul,1=pci,2=active,3=band,4=arfcn,5=bw,6=pad,7=rsrp,8=rsrq,9=sinr,10=rssi
        "nrca": "1,803,2,40,472000,40,0,-88,-11.4,12.5,-69",
        # No locks active
        "lte_band_lock": "0",
        "nr5g_sa_band_lock": "",
        "nr5g_nsa_band_lock": "",
    }


def dashboard_batch():
    rx = jitter(610_000_000 / 8, 0.3)  # ~610 Mbps in bps
    tx = jitter(38_000_000 / 8, 0.3)
    return {
        "device": {
            "hostname": "U60-Pro",
            "uptime_secs": uptime(),
            "load_avg": [0.42, 0.35, 0.31],
            "kernel": "Linux version 5.15.170-perf (builder@zte) "
                      "(aarch64-openwrt-linux-musl-gcc 12.3.0) #1 SMP PREEMPT",
        },
        "battery": {
            "capacity": 78,
            "status": "Charging",
            "voltage_uv": 4_210_000,
            "temperature": 330,  # tenths of a degree
            "current_ua": 1_450_000,
        },
        "cpu": {"overall": round(jitter(23, 0.4), 1), "cores": [31, 22, 19, 20]},
        "memory": {"total_kb": 1_638_000, "used_kb": 612_000, "free_kb": 1_026_000, "usage_pct": 37.4},
        # Mirrors agent/src/system.rs::SpeedSnapshot exactly. Rates are bytes/sec.
        "speed": {
            "rx_bytes": 18_400_000_000,
            "tx_bytes": 1_260_000_000,
            "rx_speed": int(rx),
            "tx_speed": int(tx),
            "max_rx_speed": 712_000_000 // 8,
            "max_tx_speed": 46_000_000 // 8,
            "elapsed_ms": 16_000,
        },
        "data_usage": {
            "day": {"rx_bytes": 2_350_000_000, "tx_bytes": 118_000_000, "time_secs": 32_400},
            "month": {"rx_bytes": 64_800_000_000, "tx_bytes": 3_900_000_000, "time_secs": 640_000},
            "cycle": {"rx_bytes": 64_800_000_000, "tx_bytes": 3_900_000_000, "time_secs": 640_000},
            "since_power_on": {"rx_bytes": 18_400_000_000, "tx_bytes": 1_260_000_000, "time_secs": uptime()},
            "total": {"rx_bytes": 402_000_000_000, "tx_bytes": 21_700_000_000, "time_secs": 4_120_000},
            "reset_day": 1,
            "reset_enabled": True,
            "clear_date_record": "2026-08-01",
            "next_clear_date": "2026-09-01",
        },
        "signal": signal_raw(),
        "wan": {
            "up": True,
            "ipv4-address": [{"address": "10.150.82.14"}],
            "ipv6-address": [{"address": "2406:3400:8123:4a00::1c"}],
            "route": [{"nexthop": "10.150.82.1"}],
            "dns-server": ["10.150.82.1"],
            "proto": "qmi",
        },
        "wan6": {
            "up": True,
            "ipv6-address": [{"address": "2406:3400:8123:4a00::1c", "mask": 64}],
            "ipv6-prefix": [{"address": "2406:3400:8123:4a00", "mask": 64}],
            "dns-server": ["2406:3400:8123::1"],
        },
        "thermal": {"cpuss_temp": round(jitter(61, 0.06), 1)},
    }


def clients():
    return {
        "clients": [
            {"mac": "3C:22:FB:90:21:4A", "ip": "192.168.0.101", "hostname": "Davids-MacBook-Pro",
             "medium": "wifi", "medium_detail": "wifi_5ghz", "wifi_band": "5 GHz",
             "signal_dbm": -42, "tx_bitrate_mbps": 2401.9, "rx_bitrate_mbps": 2401.9,
             "expected_throughput_mbps": 1680.0, "connected_secs": 184_200},
            {"mac": "F0:BE:2B:77:8C:11", "ip": "192.168.0.102", "hostname": "iPhone-16",
             "medium": "wifi", "medium_detail": "wifi_5ghz", "wifi_band": "5 GHz",
             "signal_dbm": -55, "tx_bitrate_mbps": 1152.0, "rx_bitrate_mbps": 864.0,
             "expected_throughput_mbps": 780.0, "connected_secs": 96_500},
            {"mac": "8C:3B:32:0E:CD:5F", "ip": "192.168.0.103", "hostname": "living-room-tv",
             "medium": "wifi", "medium_detail": "wifi_2ghz", "wifi_band": "2.4 GHz",
             "signal_dbm": -67, "tx_bitrate_mbps": 144.4, "rx_bitrate_mbps": 115.6,
             "expected_throughput_mbps": 90.0, "connected_secs": 402_100},
            {"mac": "A4:83:E7:51:09:B2", "ip": "192.168.0.104", "hostname": "work-laptop",
             "medium": "usb-c", "medium_detail": "usb_c", "interface": "ncm0",
             "connected_secs": 7_800},
            {"mac": "DC:A6:32:B4:44:91", "ip": "192.168.0.105", "hostname": "office-pc",
             "medium": "ethernet", "medium_detail": "ethernet", "interface": "eth0",
             "wired_link_mbps": 1000, "connected_secs": 512_000},
        ]
    }


def wifi_defaults():
    return {
        "wifi_onoff": "1", "wifi_onoff_supported": True,
        "wifi6_switch": "1", "wifi6_supported": True,
        "radio2_disabled": "0", "radio5_disabled": "0",
        "channel_2g": "0", "channel_5g": "44",
        "actual_channel_2g": 6, "actual_channel_5g": 44,
        "actual_bw_2g": "40 MHz", "actual_bw_5g": "160 MHz",
        "htmode_2g": "HT40", "htmode_5g": "HT160",
        "txpower_2g": "100", "txpower_5g": "100",
        "country_code": "AU",
        "ssid_2g": "U60Pro-Home", "ssid_5g": "U60Pro-Home",
        "key_2g": "", "key_5g": "", "has_key_2g": True, "has_key_5g": True,
        "encryption_2g": "psk3-mixed", "encryption_5g": "psk3-mixed",
        "hidden_2g": "0", "hidden_5g": "0",
        "clients_2g": 1, "clients_5g": 2, "clients_total": 3,
        "guest_ssid": "",
        "guest_disabled_2g": "1", "guest_disabled_5g": "1",
    }


def usb_defaults():
    return {
        "mode": "user",
        "active_mode": "ecm",
        "default_mode": "ecm",
        "ncm_persist_on_boot": False,
        "supported_modes": ["rndis", "ecm", "ncm"],
        "experimental_modes": ["ncm"],
        "mode_capabilities": [
            {"mode": "rndis", "supported": True, "experimental": False, "function": "gsi.rndis"},
            {"mode": "ecm", "supported": True, "experimental": False, "function": "gsi.ecm"},
            {"mode": "ncm", "supported": True, "experimental": True, "function": "ncm.0",
             "note": "configfs NCM exists, but ZTE's ubus USB switch does not expose it"},
        ],
        "composition_functions": ["gsi.ecm", "mass_storage.0"],
        "configfs": {"present": True, "ncm": True, "gsi_ecm": True, "gsi_rndis": True},
        "bridge": {"name": "br-lan", "members": ["ecm0", "wlan0", "wlan2", "ncm0"]},
        "interfaces": {"ecm0": True, "rndis0": False, "ncm0": True, "ncm_ifname": None},
        "usb_ids": {"vendor": "0x19d2", "product": "0x1405"},
        "connect": 1,
        "typec_cc": "cc1",
        "link": {
            "negotiated": "super-speed", "negotiated_label": "USB 3.0", "negotiated_mbps": 5000,
            "max": "super-speed-plus", "max_label": "USB 3.1 Gen2", "max_mbps": 10000,
            "at_full_speed": False,
        },
    }


def battery_detail():
    return {
        "capacity": 78, "status": "Charging",
        "voltage_mv": 4210, "voltage_max_mv": 4500, "voltage_ocv_mv": 4190,
        "current_ma": 1450, "power_mw": 6105, "temperature_c": 33.0,
        "charge_type": "Fast", "health": "Good", "cycle_count": 214,
        "charge_counter_mah": 7800, "charge_full_mah": 9410, "charge_full_design_mah": 10000,
        "time_to_full_secs": 3300, "time_to_empty_secs": -1,
    }


def thermal_all():
    return {
        "cpu_0": 61.2, "cpu_1": 60.8, "cpu_2": 59.7, "cpu_3": 60.1,
        "modem": 55.0, "modem_ss0": 52.0, "modem_ss1": 51.0, "modem_ss2": 50.0,
        "battery": 33.0, "usb": 38.0, "eth_phy": 44.0, "pmic": 49.0,
        "xo_therm": 35.0, "pa": 47.0, "sdr": 45.0,
    }


def sms_list():
    return {
        "messages": [
            {"id": 3721, "number": "+61412345678", "content": "Your Telstra usage is at 80% of your plan.",
             "date": "2026-08-08 16:42:11", "tag": 0, "mem_store": 1},
            {"id": 3720, "number": "Telstra", "content": "Welcome to 5G. Your plan now includes 5G access.",
             "date": "2026-08-07 09:15:02", "tag": 1, "mem_store": 1},
            {"id": 3719, "number": "+61498765432", "content": "Are you coming over on the weekend?",
             "date": "2026-08-06 19:03:44", "tag": 1, "mem_store": 1},
            {"id": 3718, "number": "+61412345678", "content": "On my way, should be there in 20.",
             "date": "2026-08-06 18:40:12", "tag": 2, "mem_store": 1},
        ]
    }


def apn_profiles():
    return {
        "apnListArray": [
            {"profilename": "Telstra", "wanapn": "telstra.internet", "username": "", "password": "",
             "pdpType": 3, "pppAuthMode": 0, "profileId": "1", "isEnable": True},
            {"profilename": "Telstra M2M", "wanapn": "telstra.m2m", "username": "", "password": "",
             "pdpType": 1, "pppAuthMode": 0, "profileId": "2", "isEnable": False},
        ]
    }


def system_top():
    # Mirrors agent/src/system.rs::ProcessListResult / ProcessEntry exactly.
    procs = [
        {"pid": 487, "name": "zte_topsw_tr069", "cpu_pct": 3.2, "rss_kb": 23300, "state": "sleeping", "is_bloat": True},
        {"pid": 611, "name": "zte_router", "cpu_pct": 2.4, "rss_kb": 9800, "state": "sleeping", "is_bloat": False},
        {"pid": 512, "name": "zte_mqtt_sdk_st", "cpu_pct": 1.8, "rss_kb": 11100, "state": "sleeping", "is_bloat": True},
        {"pid": 534, "name": "zte_topsw_nwinfo", "cpu_pct": 1.1, "rss_kb": 6400, "state": "sleeping", "is_bloat": False},
        {"pid": 702, "name": "hostapd", "cpu_pct": 0.9, "rss_kb": 4100, "state": "sleeping", "is_bloat": False},
        {"pid": 811, "name": "zte-agent", "cpu_pct": 0.6, "rss_kb": 2048, "state": "running", "is_bloat": False},
        {"pid": 850, "name": "uhttpd", "cpu_pct": 0.3, "rss_kb": 1900, "state": "sleeping", "is_bloat": False},
        {"pid": 1, "name": "init", "cpu_pct": 0.1, "rss_kb": 1200, "state": "sleeping", "is_bloat": False},
    ]
    bloat = [p for p in procs if p["is_bloat"]]
    return {
        "processes": procs,
        "total_count": 142,
        "bloat_count": len(bloat),
        "bloat_cpu_pct": round(sum(p["cpu_pct"] for p in bloat), 1),
        "bloat_rss_kb": sum(p["rss_kb"] for p in bloat),
    }


# Headers mirror agent/src/{signal,connection}_logger.rs::HEADER.
SIGNAL_LOG_CSV = (
    "timestamp,datetime,network_type,carrier,cell_id,lte_band,lte_pci,lte_earfcn,"
    "lte_rsrp,lte_rsrq,lte_sinr,lte_rssi,nr_band,nr_pci,nr_arfcn,nr_rsrp,nr_rsrq,"
    "nr_sinr,nr_rssi,lte_ca_bands,nr_ca_bands\n"
    "1754700000,2026-08-09T09:20:00,ENDC,Telstra,134479973,B8,312,3650,"
    "-71,-10.8,19.5,-62,n78,801,630912,-77,-9.2,23.5,-58,B8+B1,n78+n40\n"
    "1754700003,2026-08-09T09:20:03,ENDC,Telstra,134479973,B8,312,3650,"
    "-72,-10.9,19.1,-63,n78,801,630912,-78,-9.4,22.8,-59,B8+B1,n78+n40\n"
)

CONNECTION_LOG_CSV = (
    "timestamp,datetime,event_type,detail,old_value,new_value\n"
    "1754700012,2026-08-09T09:20:12,nr_band_change,NR band changed,n78,n40\n"
    "1754700190,2026-08-09T09:23:10,cell_handover,cell_id changed,134479973,134479981\n"
)

# ── Mutable demo state ────────────────────────────────────────────────────────
# The real agent answers a mutation with the resulting state and the dashboard
# renders that, so a mock that ignored writes made every control look broken:
# a toggle would flip and immediately snap back. Keep enough state to make the
# UI behave the way it will on the device.

STATE = {
    "charge_control": {
        "charging_stopped": False, "battery_status": "Charging", "capacity": 78,
        "charge_limit_enabled": False, "charge_limit": 90, "hysteresis": 5,
        "manual_override": False,
    },
    "charger": {"otg_powerbank_state": 0, "direct_power_supply_mode": "disable"},
    "ttl": {"active": False, "ipv6_active": False, "ttl_value": 0},
    "wifi": wifi_defaults(),
    "usb": usb_defaults(),
    "dns": {
        "prefer_dns_manual": "1.1.1.1", "standby_dns_manual": "1.0.0.1",
        "ipv6_wan_prefer_dns_manual": "2606:4700:4700::1111",
        "ipv6_wan_standby_dns_manual": "2606:4700:4700::1001",
    },
    "lan": {
        "lan_ipaddr": "192.168.0.1", "lan_netmask": "255.255.255.0",
        "dhcp_start": "192.168.0.100", "dhcp_end": "192.168.0.200",
        "dhcp_lease_time": "43200",
    },
    "apn_mode": {"apn_mode": 1},
}


def put_charge_control(body):
    """Mirrors agent/src/device_ext.rs::charge_control_set, including its
    inverted charger semantics and the manual-override interaction."""
    cc = STATE["charge_control"]
    if "charging_stopped" in body:
        cc["charging_stopped"] = bool(body["charging_stopped"])
        cc["manual_override"] = cc["charging_stopped"]
        cc["battery_status"] = "Not charging" if cc["charging_stopped"] else "Charging"
    if any(k in body for k in ("charge_limit_enabled", "charge_limit", "hysteresis")):
        for k in ("charge_limit_enabled", "charge_limit", "hysteresis"):
            if k in body:
                cc[k] = body[k]
        # The enforcer clears a manual override whenever limit config changes.
        cc["manual_override"] = False
    return cc


def put_ttl_set(body):
    ttl = int(body.get("ttl") or 0)
    STATE["ttl"] = {"active": True, "ipv6_active": True, "ttl_value": ttl}
    return {"ttl": ttl, "ipv4": True, "ipv6": True}


def put_wifi_settings(body):
    wifi = STATE["wifi"]
    for key, value in (body or {}).items():
        if key in ("key_2g", "key_5g"):
            wifi[key] = value
            wifi["has_key_" + key.split("_")[1]] = bool(value)
        elif key in wifi:
            wifi[key] = value
    # Auto channel ("0") means the runtime picks one; otherwise it follows.
    for band, fallback in (("2g", 6), ("5g", 44)):
        configured = str(wifi.get("channel_" + band, "0"))
        wifi["actual_channel_" + band] = fallback if configured in ("0", "auto", "") else int(configured)
    return {"status": "ok"}


def put_usb_mode(body):
    mode = (body or {}).get("mode")
    if mode:
        STATE["usb"]["active_mode"] = mode
    return {"status": "ok", "active_mode": STATE["usb"]["active_mode"]}


def put_usb_default(body):
    mode = (body or {}).get("mode")
    if mode:
        STATE["usb"]["default_mode"] = mode
        STATE["usb"]["ncm_persist_on_boot"] = mode == "ncm"
    return {"status": "ok", "default_mode": STATE["usb"]["default_mode"]}


def put_powerbank(body):
    STATE["charger"]["otg_powerbank_state"] = int((body or {}).get("state") or 0)
    return {"status": "ok"}


def merge_into(slot):
    def handler(body):
        STATE[slot].update(
            {k: v for k, v in (body or {}).items() if k in STATE[slot]}
        )
        return STATE[slot]
    return handler


ROUTES_PUT = {
    "/api/device/charge-control": put_charge_control,
    "/api/ttl/set": put_ttl_set,
    "/api/wifi/settings": put_wifi_settings,
    "/api/usb/mode": put_usb_mode,
    "/api/usb/default": put_usb_default,
    "/api/usb/powerbank": put_powerbank,
    "/api/router/dns": merge_into("dns"),
    "/api/router/lan": merge_into("lan"),
    "/api/router/apn/mode": merge_into("apn_mode"),
}


# Demo identifiers. Fake, but full-length so the dashboard's reveal toggle has
# something to reveal — the agent masks these server-side and only returns the
# full value for ?full=true.
# Routes that return identifiers masked unless ?full=true.
MASKING_ROUTES = {"/api/euicc/eid", "/api/euicc/profiles", "/api/euicc/notifications"}

DEMO_EID = "89044012345678901234567890121234"
DEMO_ICCID = "89610112345678901234"
DEMO_ICCID_2 = "89440112345678905678"


def mask(value: str) -> str:
    """Mirror the agent's masking: first four and last four digits."""
    if len(value) <= 8:
        return "*" * len(value)
    return value[:4] + "*" * (len(value) - 8) + value[-4:]


ROUTES_GET = {
    "/api/dashboard": dashboard_batch,
    "/api/network/clients": clients,
    "/api/device": lambda: dashboard_batch()["device"],
    "/api/cpu": lambda: dashboard_batch()["cpu"],
    "/api/memory": lambda: dashboard_batch()["memory"],
    "/api/wifi/status": lambda: STATE["wifi"],
    "/api/usb/status": lambda: STATE["usb"],
    "/api/device/charger": lambda: STATE["charger"],
    "/api/device/charge-control": lambda: STATE["charge_control"],
    "/api/device/thermal/all": thermal_all,
    "/api/device/battery/detail": battery_detail,
    "/api/device/battery-info": lambda: {
        "battery_online": 1, "battery_low_power": 0, "battery_using_hw_fg_chip": 1,
        "battery_time_to_full": 55, "battery_time_to_empty": 0,
    },
    "/api/router/dns": lambda: STATE["dns"],
    "/api/router/lan": lambda: STATE["lan"],
    "/api/router/apn/mode": lambda: STATE["apn_mode"],
    "/api/router/apn/profiles": apn_profiles,
    "/api/sim/info": lambda: {
        "sim_iccid": "89610112345678901234", "sim_imsi": "505011234567890",
        "sim_states": "SIM_READY", "mdm_mcc": "505", "mdm_mnc": "01",
    },
    "/api/sim/imei": lambda: {"imei": "351234567890123"},
    "/api/ttl/status": lambda: STATE["ttl"],
    # eUICC — a commissioned card with one enabled operational profile, the
    # shape the real device returns. Identifiers are fake and pre-masked.
    "/api/euicc/status": lambda: {
        "card_present": True, "euicc_available": True,
        "detail": "ISD-R selected successfully",
    },
    "/api/euicc/eid": lambda full=False: {
        "eid": DEMO_EID if full else mask(DEMO_EID), "masked": not full,
    },
    "/api/euicc/profiles": lambda full=False: {
        "profiles": [{
            "iccid": DEMO_ICCID if full else mask(DEMO_ICCID),
            "isdp_aid": "A0000005591010FFFFFFFF8900001100",
            "state": "enabled", "enabled": True, "class": "operational",
            "nickname": None, "service_provider": "Demo Mobile", "name": "Demo Mobile",
        }, {
            "iccid": DEMO_ICCID_2 if full else mask(DEMO_ICCID_2),
            "isdp_aid": "A0000005591010FFFFFFFF8900001101",
            "state": "disabled", "enabled": False, "class": "operational",
            "nickname": "Travel", "service_provider": "Demo Roaming", "name": "Demo Roaming",
        }],
        "count": 2, "masked": not full,
    },
    "/api/euicc/chip": lambda: {
        "result": {"EUICCInfo2": {"euiccFirmwareVer": "36.17.4",
                                  "extCardResource": {"freeNonVolatileMemory": 1488672}}},
        "progress": [],
    },
    # The relay is idle in the mock: there is no download in flight, so a
    # dashboard poll should report exactly that rather than a fake wait.
    "/api/euicc/relay/status": lambda: {
        "active": False,
        "waiting_for_client": False,
        "client_connected": False,
    },
    "/api/operator/scan": lambda: {
        "state": "manual_selected",
        "scanning": False,
        "failed": False,
        "operators": [
            {"status": "current", "name": "Mobitel", "plmn": "41301", "mcc": "413",
             "mnc": "01", "rat": "LTE", "select": "41301,7"},
            {"status": "available", "name": "DIALOG", "plmn": "41302", "mcc": "413",
             "mnc": "02", "rat": "LTE", "select": "41302,7"},
            {"status": "forbidden", "name": "Hutch", "plmn": "41308", "mcc": "413",
             "mnc": "08", "rat": "LTE", "select": "41308,7"},
        ],
    },
    "/api/tunnel/wireguard/profiles": lambda: {
        "masked": True,
        "profiles": [
            {"id": 1, "name": "Provider, Mumbai", "active": True, "settings": {
                "peer_host": "in2.example.com", "peer_listen_port": "1024",
                "tunnel_ip": "192.168.6.190", "private_key": "ULHa****pls=",
                "peer_public_key": "T3BlblU2MCBmYWJyaWNhdGVkIHdnIHB1YmtleSEhISE=",
                "peer_remote_ip": "0.0.0.0", "peer_remote_mask": "0", "listen_port": "51820"}},
            {"id": 2, "name": "Home", "active": False, "settings": {
                "peer_host": "home.example.net", "peer_listen_port": "51820",
                "tunnel_ip": "10.0.0.2", "private_key": "aB3d****xY9=",
                "peer_public_key": "T3BlblU2MCBmYWJyaWNhdGVkIHdnIHByaXZrZXkhISE=",
                "peer_remote_ip": "10.0.0.0", "peer_remote_mask": "24", "listen_port": "51820"}},
        ],
    },
    # A configured-but-down tunnel, which is the state worth looking at: an
    # unconfigured one renders almost nothing.
    "/api/tunnel/wireguard": lambda: {
        "available": True,
        "configured": True,
        "is_active_tunnel": False,
        "connect_status": "disconnected",
        "masked": True,
        "settings": {
            "tunnel_ip": "10.0.0.2/24",
            "listen_port": "51820",
            "peer_public_key": "kQd8••••••••••••••••••••••••••••••••••••3xA=",
            "peer_endip": "vpn.example.com",
            "peer_listen_port": "51820",
            "peer_tunnel_ip": "10.0.0.1/32",
            "peer_remote_ip": "0.0.0.0",
            "peer_remote_mask": "0.0.0.0",
            "auto_start": "0",
        },
    },
    "/api/euicc/notifications": lambda full=False: {
        "result": [
            {"seqNumber": 0, "iccid": DEMO_ICCID if full else mask(DEMO_ICCID),
             "notificationAddress": "demo.prod.ondemandconnectivity.com",
             "profileManagementOperation": "install"},
        ],
        "progress": [], "masked": not full,
    },
    "/api/capabilities": lambda: {
        "agent_version": "0.1.0", "api_version": 1,
        "supported": {
            "dashboard_batch": True, "wifi": True, "sms": True, "sim": True,
            "cell_lock": True, "band_lock": True, "apn": True, "router_dns": True,
            "router_lan": True, "usb": True, "ttl": True, "charge_control": True,
            "signal_logger": True, "connection_logger": True,
            "at_console_readonly": True, "euicc_read": True,
            "euicc_write": True, "euicc_http_relay": True,
        },
        "unsupported": {
            "at_console_write": "removed: unrestricted AT access can brick the modem",
            "doh_proxy": "removed: could leave DNS pointing at a dead local port",
            "sms_forward": "removed: background service without maintained UI",
            "scheduler": "removed: background service without maintained UI",
            "speedtest": "removed",
            "telephony": "removed",
            "tailscale": "removed: outside the conservative deployment boundary",
        },
    },
    "/api/system/top": system_top,
    "/api/logger/signal/status": lambda: {
        "running": False, "samples": 0, "elapsed_secs": 0, "duration_secs": 3600, "interval_secs": 3,
    },
    "/api/logger/connection/status": lambda: {
        "running": False, "events": 0, "elapsed_secs": 0, "duration_secs": 3600, "interval_secs": 3,
    },
    "/api/logger/signal/download": lambda: {"csv": SIGNAL_LOG_CSV},
    "/api/logger/connection/download": lambda: {"csv": CONNECTION_LOG_CSV},
    "/api/at/port": lambda: {"port": "/dev/at_mdm0", "available": True},
}

ROUTES_POST = {
    "/api/sms/list": lambda body: sms_list(),
    "/api/system/kill-bloat": lambda body: {
        "killed": [
            {"pid": p["pid"], "name": p["name"]}
            for p in system_top()["processes"] if p["is_bloat"]
        ],
        "skipped": [],
        "freed_rss_kb": system_top()["bloat_rss_kb"],
    },
    "/api/euicc/download": lambda body: {"result": {"iccid": DEMO_ICCID}, "progress": ["downloading"]},
    "/api/euicc/enable": lambda body: {
        "result": None, "progress": ["es10c_enable_profile"],
        "reboot_required": True,
        "notice": "The profile was switched on the card, but this modem cannot be "
                  "refreshed live. Reboot the router for it to take effect.",
    },
    "/api/euicc/disable": lambda body: {
        "result": None, "progress": ["es10c_disable_profile"],
        "reboot_required": True,
        "notice": "The profile was switched on the card, but this modem cannot be "
                  "refreshed live. Reboot the router for it to take effect.",
    },
    "/api/euicc/delete": lambda body: {"result": None, "progress": []},
    "/api/euicc/nickname": lambda body: {"result": None, "progress": []},
    "/api/euicc/notifications/process": lambda body: {"result": None, "progress": []},
    "/api/euicc/notifications/remove": lambda body: {"result": None, "progress": []},
    "/api/at/send": lambda body: {
        "command": (body or {}).get("command", "AT"),
        "response": f"{(body or {}).get('command', 'AT')}\r\nOK",
        "port": "/dev/at_mdm0", "elapsed_ms": 42,
    },
}


class Handler(BaseHTTPRequestHandler):
    def _cors(self):
        origin = self.headers.get("Origin", "*")
        self.send_header("Access-Control-Allow-Origin", origin)
        self.send_header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Authorization, Content-Type, X-Confirm")
        self.send_header("Access-Control-Max-Age", "86400")

    def _send(self, payload, status=200):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self._cors()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        length = int(self.headers.get("Content-Length") or 0)
        if length == 0:
            return {}
        try:
            return json.loads(self.rfile.read(length))
        except Exception:
            return {}

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/api/auth/login":
            return self._send({"ok": False, "error": "use POST"}, 405)
        fn = ROUTES_GET.get(path)
        if fn:
            if path in MASKING_ROUTES:
                query = parse_qs(urlparse(self.path).query)
                full = query.get("full", [""])[0] in ("true", "1", "yes")
                return self._send({"ok": True, "data": fn(full)})
            return self._send({"ok": True, "data": fn()})
        return self._send({"ok": True, "data": {}})

    def do_POST(self):
        path = self.path.split("?")[0]
        body = self._body()
        if path == "/api/auth/login":
            return self._send({"ok": True, "data": {"token": "demo-token"}})
        fn = ROUTES_POST.get(path)
        if fn:
            return self._send({"ok": True, "data": fn(body)})
        return self._send({"ok": True, "data": {}})

    def do_PUT(self):
        path = self.path.split("?")[0]
        body = self._body()
        fn = ROUTES_PUT.get(path)
        if fn:
            return self._send({"ok": True, "data": fn(body)})
        return self._send({"ok": True, "data": {}})

    def do_DELETE(self):
        path = self.path.split("?")[0]
        if path == "/api/ttl/clear":
            STATE["ttl"] = {"active": False, "ipv6_active": False, "ttl_value": 0}
        return self._send({"ok": True, "data": {}})

    def log_message(self, fmt, *args):  # quiet
        pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=9090)
    args = ap.parse_args()
    srv = ThreadingHTTPServer(("0.0.0.0", args.port), Handler)
    print(f"mock zte-agent listening on http://localhost:{args.port}")
    srv.serve_forever()


if __name__ == "__main__":
    main()
