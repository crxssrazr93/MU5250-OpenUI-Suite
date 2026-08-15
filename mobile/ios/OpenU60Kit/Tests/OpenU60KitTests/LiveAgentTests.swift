import XCTest
@testable import OpenU60Kit

/// Talks to a real agent. Skipped unless you point it at one:
///
///     AGENT_HOST=192.168.0.1 AGENT_PASSWORD=… swift test
///
/// The decoding tests above prove the parsers against payloads that are checked
/// in and therefore frozen. This proves the same parsers against whatever the
/// firmware is sending today, which is the failure the frozen ones cannot see:
/// a key renamed by a firmware update, or a value that changes type.
///
/// No credentials in this file, and nothing it prints includes an identifier.
final class LiveAgentTests: XCTestCase {

    private func makeClient() async throws -> AgentClient? {
        let environment = ProcessInfo.processInfo.environment
        guard let host = environment["AGENT_HOST"],
              let password = environment["AGENT_PASSWORD"] else { return nil }
        let client = AgentClient()
        guard await client.setHost(host) else {
            XCTFail("AGENT_HOST must be a private address")
            return nil
        }
        try await client.login(password: password)
        return client
    }

    func test_sign_in_and_read_the_screens_the_app_opens_with() async throws {
        guard let client = try await makeClient() else {
            throw XCTSkip("set AGENT_HOST and AGENT_PASSWORD to run this")
        }

        let signal = SignalInfo(from: try await client.get("/api/network/signal"))
        XCTAssertFalse(signal.networkType.isEmpty, "network_type missing")
        XCTAssertNotNil(signal.primaryRSRP, "no RSRP on either radio")
        if let rsrp = signal.primaryRSRP {
            XCTAssertTrue((-140...(-30)).contains(rsrp), "implausible RSRP \(rsrp)")
        }

        let sim = SIMInfo(from: try await client.get("/api/sim/info"))
        XCTAssertFalse(sim.status.isEmpty, "sim_states missing")
        // Length, never the value: an ICCID must not be printed or asserted on.
        if !sim.iccid.isEmpty {
            XCTAssertTrue((18...22).contains(sim.iccid.count), "ICCID looks mangled")
        }

        let battery = BatteryInfo(from: try await client.get("/api/device/battery-info"))
        XCTAssertNotNil(battery.capacity, "battery_capacity missing")

        let cpu: AgentObject = try await client.get("/api/cpu")
        XCTAssertNotNil(cpu.double("overall"), "cpu overall missing")

        let data = DataConnection(from: try await client.get("/api/modem/data"))
        XCTAssertFalse(data.connectStatus.isEmpty, "connect_status missing")

        let lock = SIMLockInfo(from: try await client.get("/api/sim/lock"))
        XCTAssertFalse(lock.status.isEmpty)
    }

    func test_a_route_the_firmware_does_not_serve_is_reported_as_such() async throws {
        guard let client = try await makeClient() else {
            throw XCTSkip("set AGENT_HOST and AGENT_PASSWORD to run this")
        }
        do {
            let _: AgentObject = try await client.get("/api/definitely/not/a/route")
            XCTFail("expected a not-found")
        } catch AgentError.notFound {
            // Told apart from a transport failure on purpose: screens ask the
            // agent whether a feature exists by calling it.
        }
    }
}
