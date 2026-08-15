import Foundation

public enum SignalQuality: String, Sendable {
    case excellent, good, fair, poor, unknown

    /// Thresholds are RSRP in dBm, matching the web dashboard so the same radio
    /// does not get described two different ways on two screens.
    public init(rsrp: Double?) {
        guard let rsrp, rsrp < 0, rsrp >= -140 else { self = .unknown; return }
        switch rsrp {
        case (-80)...: self = .excellent
        case (-90)...: self = .good
        case (-100)...: self = .fair
        default: self = .poor
        }
    }
}

/// The serving cell, as the modem describes it.
public struct SignalInfo: Equatable, Sendable {
    public var networkType: String
    public var carrier: String
    public var bars: Int?
    public var lteRSRP: Double?
    public var lteRSRQ: Double?
    public var lteSINR: Double?
    public var lteBand: String
    public var lteEARFCN: Int?
    public var ltePCI: Int?
    public var nrRSRP: Double?
    public var nrRSRQ: Double?
    public var nrBand: String

    public init(from object: AgentObject) {
        networkType = object.string("network_type") ?? ""
        carrier = object.string("network_provider_fullname")
            ?? object.string("network_provider") ?? ""
        bars = object.int("signalbar")
        lteRSRP = object.double("lte_rsrp")
        lteRSRQ = object.double("lte_rsrq")
        lteSINR = object.double("lte_snr")
        lteBand = object.string("wan_active_band") ?? ""
        lteEARFCN = object.int("wan_active_channel")
        ltePCI = object.int("lte_pci")
        nrRSRP = object.double("nr5g_rsrp")
        nrRSRQ = object.double("nr5g_rsrq")
        nrBand = object.string("nr5g_action_band") ?? ""
    }

    /// The figure to headline.
    ///
    /// LTE first even in NSA: the anchor is what the unit is actually camped
    /// on, and an NR RSRP with no NR carrier reported is the modem describing a
    /// measurement, not a connection.
    public var primaryRSRP: Double? { lteRSRP ?? nrRSRP }

    public var quality: SignalQuality { SignalQuality(rsrp: primaryRSRP) }

    /// True when the modem reports NR alongside LTE.
    public var isNSA: Bool {
        let type = networkType.uppercased()
        return type.contains("ENDC") || type.contains("NSA")
    }
}
