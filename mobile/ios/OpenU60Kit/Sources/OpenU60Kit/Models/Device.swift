import Foundation

public struct BatteryInfo: Equatable, Sendable {
    public var capacity: Int?
    public var temperature: Double?
    public var charging: Bool
    public var minutesToFull: Int?
    public var minutesToEmpty: Int?

    public init(from object: AgentObject) {
        capacity = object.int("battery_capacity")
        temperature = object.double("battery_temperature")
        charging = object.bool("battery_online")
        // The firmware reports -1 for "not applicable" rather than omitting it.
        minutesToFull = object.int("battery_time_to_full").flatMap { $0 < 0 ? nil : $0 }
        minutesToEmpty = object.int("battery_time_to_empty").flatMap { $0 < 0 ? nil : $0 }
    }
}

public struct CPUInfo: Equatable, Sendable {
    /// Busy percentage across all cores.
    public var overall: Double?
    /// Per-core percentages. The count of these is the core count — the agent
    /// does not report a separate number, and reading `cores` as one is how the
    /// Android dashboard ended up with a blank CPU figure.
    public var cores: [Double]

    public init(from object: AgentObject) {
        overall = object.double("overall")
        cores = []
    }

    public init(overall: Double?, cores: [Double]) {
        self.overall = overall
        self.cores = cores
    }

    public var coreCount: Int { cores.count }
}

/// Throughput. The agent samples over a window and reports bytes per second as
/// `rx_speed` / `tx_speed`; the byte totals alongside them are cumulative.
public struct ThroughputInfo: Equatable, Sendable {
    public var downBytesPerSecond: Double
    public var upBytesPerSecond: Double
    public var rxBytes: Int64
    public var txBytes: Int64

    public init(from object: AgentObject) {
        downBytesPerSecond = object.double("rx_speed") ?? 0
        upBytesPerSecond = object.double("tx_speed") ?? 0
        rxBytes = Int64(object.double("rx_bytes") ?? 0)
        txBytes = Int64(object.double("tx_bytes") ?? 0)
    }
}

/// The data call, which is not the same thing as the radio.
///
/// `/api/modem/data` is backed by the wwan interface object for this reason:
/// the radio can be registered and reporting a fine RSRP with no data call up.
public struct DataConnection: Equatable, Sendable {
    public var connectStatus: String
    public var enabled: Bool
    public var roamingEnabled: Bool
    public var ipv4: String
    public var gateway: String
    public var dns: [String]

    public init(from object: AgentObject) {
        connectStatus = object.string("connect_status") ?? ""
        enabled = object.bool("enable")
        roamingEnabled = object.bool("roam_enable")
        ipv4 = object.string("ipv4_address") ?? ""
        gateway = object.string("ipv4_gateway") ?? ""
        dns = [object.string("ipv4_dns_prefer"), object.string("ipv4_dns_standby")]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
    }

    public var isConnected: Bool { connectStatus.contains("connected") }
}
