import XCTest
@testable import OpenU60Kit

/// Fixtures are the shapes the firmware really sends, with the identifiers
/// replaced. Never put a real ICCID, IMSI or EID in this repository — the
/// values below are made up, and the ICCID deliberately ends in F because that
/// is the character that broke the Android client.
final class DecodingTests: XCTestCase {

    private func object(_ json: String) throws -> AgentObject {
        try JSONDecoder().decode(AgentObject.self, from: Data(json.utf8))
    }

    // MARK: - The bug this type exists to prevent

    func test_an_identifier_ending_in_F_stays_a_string() throws {
        // Java's parseDouble reads a trailing F as a float suffix, so the
        // Android client turned this ICCID into 8.99e18 and the SIM screen
        // showed nothing. Swift's Double(_:) accepts "1F" too, hence the
        // hand-rolled numeric check in AgentValue.
        let sim = SIMInfo(from: try object(#"{"sim_iccid": "8900000000000000123F"}"#))
        XCTAssertEqual(sim.iccid, "8900000000000000123F")
    }

    func test_a_quoted_number_is_readable_as_both() throws {
        let fields = try object(#"{"mdm_mcc": "413", "current_sim_slot": "1"}"#)
        XCTAssertEqual(fields.string("mdm_mcc"), "413")
        XCTAssertEqual(fields.int("mdm_mcc"), 413)
        XCTAssertEqual(fields.string("current_sim_slot"), "1")
    }

    func test_an_unquoted_number_is_readable_as_text() throws {
        // The same key is a number on some builds and a string on others, and a
        // screen printing it must not care which.
        let fields = try object(#"{"lte_rsrp": -97, "signalbar": 4}"#)
        XCTAssertEqual(fields.double("lte_rsrp"), -97)
        XCTAssertEqual(fields.string("lte_rsrp"), "-97")
        XCTAssertEqual(fields.string("signalbar"), "4")
    }

    func test_a_whole_number_does_not_gain_a_decimal_point() throws {
        // A slot displayed as "1.0" is the kind of thing nobody files a bug for
        // and everybody notices.
        XCTAssertEqual(try object(#"{"slot": 1}"#).string("slot"), "1")
    }

    func test_text_that_merely_looks_numeric_is_not_a_number() throws {
        let fields = try object(#"{"a": "1F", "b": "0x1p3", "c": "infinity", "d": "nan", "e": "1e5"}"#)
        for key in ["a", "b", "c", "d", "e"] {
            XCTAssertNil(fields.double(key), "\(key) must not parse as a number")
        }
    }

    func test_the_firmwares_several_spellings_of_yes() throws {
        let fields = try object(#"{"a": "1", "b": "true", "c": "on", "d": true, "e": 1, "f": "0", "g": ""}"#)
        for key in ["a", "b", "c", "d", "e"] {
            XCTAssertTrue(fields.bool(key), "\(key) should be true")
        }
        XCTAssertFalse(fields.bool("f"))
        XCTAssertFalse(fields.bool("g"))
    }

    func test_a_missing_key_is_absent_not_zero() throws {
        // A count that defaults to 0 reads as "no attempts left", which is a
        // materially different claim from "could not read it".
        let lock = SIMLockInfo(from: try object(#"{"status": "ready"}"#))
        XCTAssertNil(lock.nckAttemptsLeft)
        XCTAssertNil(lock.pinAttemptsLeft)
    }

    // MARK: - SIM

    func test_sim_info_from_a_real_shaped_payload() throws {
        let sim = SIMInfo(from: try object("""
        {"Operator": "", "current_sim_slot": "1", "mdm_mcc": "413", "mdm_mnc": "02",
         "msisdn": "", "pin_status": "0", "pinnumber": "5", "puknumber": "10",
         "sim_iccid": "8900000000000000123F", "sim_imsi": "413020000000000",
         "sim_states": "sim ready", "spn_name_data": "004400690061006C006F0067"}
        """))
        XCTAssertEqual(sim.status, "sim ready")
        XCTAssertEqual(sim.mcc, "413")
        XCTAssertEqual(sim.mnc, "02")
        XCTAssertEqual(sim.slot, "1")
        XCTAssertEqual(sim.spn, "Dialog")
        XCTAssertEqual(sim.pinAttempts, 5)
        XCTAssertEqual(sim.pukAttempts, 10)
        // Genuinely empty in the firmware, not a parsing failure.
        XCTAssertEqual(sim.operatorName, "")
    }

    func test_spn_that_is_not_hex_is_left_alone() {
        XCTAssertEqual(SIMInfo.decodeSPN("Dialog"), "Dialog")
        XCTAssertEqual(SIMInfo.decodeSPN(""), "")
    }

    func test_spn_padding_ends_the_string() {
        XCTAssertEqual(SIMInfo.decodeSPN("0044FFFF"), "D")
    }

    // MARK: - Signal

    func test_signal_prefers_the_lte_anchor() throws {
        let signal = SignalInfo(from: try object("""
        {"network_type": "ENDC", "network_provider_fullname": "Dialog", "signalbar": 4,
         "lte_rsrp": -97, "lte_rsrq": -14, "nr5g_rsrp": -113}
        """))
        XCTAssertEqual(signal.primaryRSRP, -97)
        XCTAssertEqual(signal.quality, .fair)
        XCTAssertTrue(signal.isNSA)
        XCTAssertEqual(signal.bars, 4)
    }

    func test_quality_thresholds() {
        XCTAssertEqual(SignalQuality(rsrp: -70), .excellent)
        XCTAssertEqual(SignalQuality(rsrp: -85), .good)
        XCTAssertEqual(SignalQuality(rsrp: -95), .fair)
        XCTAssertEqual(SignalQuality(rsrp: -113), .poor)
        XCTAssertEqual(SignalQuality(rsrp: nil), .unknown)
        // Out of range readings are not "excellent"; the modem reports 0 when
        // it has nothing, and that must not render as a perfect signal.
        XCTAssertEqual(SignalQuality(rsrp: 0), .unknown)
        XCTAssertEqual(SignalQuality(rsrp: -200), .unknown)
    }

    // MARK: - Device

    func test_battery_treats_minus_one_as_not_applicable() throws {
        let battery = BatteryInfo(from: try object("""
        {"battery_capacity": 92, "battery_temperature": 37, "battery_online": 1,
         "battery_time_to_full": -1, "battery_time_to_empty": 2091}
        """))
        XCTAssertEqual(battery.capacity, 92)
        XCTAssertTrue(battery.charging)
        XCTAssertNil(battery.minutesToFull)
        XCTAssertEqual(battery.minutesToEmpty, 2091)
    }

    func test_data_connection_reads_the_wwan_interface() throws {
        let data = DataConnection(from: try object("""
        {"connect_status": "ipv4_connected", "enable": 1, "roam_enable": 0,
         "ipv4_address": "100.87.23.156", "ipv4_gateway": "100.87.23.157",
         "ipv4_dns_prefer": "202.69.205.1", "ipv4_dns_standby": ""}
        """))
        XCTAssertTrue(data.isConnected)
        XCTAssertTrue(data.enabled)
        XCTAssertFalse(data.roamingEnabled)
        // The empty standby entry is dropped rather than shown as a blank row.
        XCTAssertEqual(data.dns, ["202.69.205.1"])
    }

    func test_throughput_uses_the_agents_computed_speed() throws {
        let speed = ThroughputInfo(from: try object("""
        {"rx_speed": 1234.5, "tx_speed": 99, "rx_bytes": 577621, "tx_bytes": 339046}
        """))
        XCTAssertEqual(speed.downBytesPerSecond, 1234.5)
        XCTAssertEqual(speed.upBytesPerSecond, 99)
        XCTAssertEqual(speed.rxBytes, 577621)
    }

    // MARK: - Envelope and addresses

    func test_a_list_or_scalar_body_yields_no_fields_rather_than_throwing() throws {
        XCTAssertTrue(try object("[]").fields.isEmpty)
        XCTAssertTrue(try object("5").fields.isEmpty)
    }

    func test_private_addresses_only() {
        for good in ["192.168.0.1", "10.0.0.7", "172.16.5.9", "172.31.0.1", "127.0.0.1", "localhost"] {
            XCTAssertTrue(AgentClient.isPrivateAddress(good), "\(good) should be allowed")
        }
        for bad in ["8.8.8.8", "172.15.0.1", "172.32.0.1", "1.1.1.1", "example.com",
                    "192.168.0", "192.168.0.1.5", "192.168.0.256", "", "192.168.0.a"] {
            XCTAssertFalse(AgentClient.isPrivateAddress(bad), "\(bad) should be refused")
        }
    }
}
