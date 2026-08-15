import Foundation

/// What the SIM screen shows.
///
/// Built from `AgentObject` rather than decoded into `Codable` properties: the
/// route is a passthrough of the vendor's `get_sim_info`, which sends most of
/// these as strings, some as numbers, and not always the same way between
/// firmware builds. Reading keys through `AgentValue` survives that; a struct
/// with `let simSlot: Int` does not.
public struct SIMInfo: Equatable, Sendable {
    public var status: String
    public var operatorName: String
    public var iccid: String
    public var imsi: String
    public var msisdn: String
    public var spn: String
    public var mcc: String
    public var mnc: String
    public var slot: String
    public var pinStatus: String
    public var pinAttempts: Int?
    public var pukAttempts: Int?

    public init(from object: AgentObject) {
        status = object.string("sim_states") ?? ""
        operatorName = object.string("Operator") ?? ""
        iccid = object.string("sim_iccid") ?? ""
        imsi = object.string("sim_imsi") ?? ""
        msisdn = object.string("msisdn") ?? ""
        spn = SIMInfo.decodeSPN(object.string("spn_name_data") ?? "")
        mcc = object.string("mdm_mcc") ?? ""
        mnc = object.string("mdm_mnc") ?? ""
        slot = object.string("current_sim_slot") ?? ""
        pinStatus = object.string("pin_status") ?? ""
        pinAttempts = object.int("pinnumber")
        pukAttempts = object.int("puknumber")
    }

    /// The service provider name, which arrives as UCS-2 hex.
    ///
    /// "Dialog" is sent as 004400690061006C006F0067. Anything that is not clean
    /// hex is passed through unchanged, because some builds send it as plain
    /// text and mangling that would be worse than not decoding it.
    static func decodeSPN(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return "" }
        guard trimmed.count % 4 == 0,
              trimmed.allSatisfy({ $0.isHexDigit }) else { return trimmed }

        var scalars = String.UnicodeScalarView()
        var index = trimmed.startIndex
        while index < trimmed.endIndex {
            let next = trimmed.index(index, offsetBy: 4)
            guard let code = UInt32(trimmed[index..<next], radix: 16) else { return trimmed }
            // 0xFFFF pads a fixed-width field; surrogates are not standalone
            // characters and would produce nothing useful.
            if code == 0xFFFF || code == 0 { break }
            guard let scalar = Unicode.Scalar(code) else { return trimmed }
            scalars.append(scalar)
            index = next
        }
        let decoded = String(scalars).trimmingCharacters(in: .whitespaces)
        return decoded.isEmpty ? trimmed : decoded
    }
}

/// PIN, PUK and network-lock attempt counts.
public struct SIMLockInfo: Equatable, Sendable {
    public var status: String
    public var simState: String
    public var pinAttemptsLeft: Int?
    public var pukAttemptsLeft: Int?
    /// Network (carrier) unlock attempts. Nil, never zero, when it cannot be
    /// read: zero here means none remain, and guessing that is not harmless.
    public var nckAttemptsLeft: Int?
    public var pukNearlyExhausted: Bool

    public init(from object: AgentObject) {
        status = object.string("status") ?? "unknown"
        simState = object.string("sim_state") ?? ""
        pinAttemptsLeft = object.int("pin_attempts_left")
        pukAttemptsLeft = object.int("puk_attempts_left")
        nckAttemptsLeft = object.int("available_trials")
        pukNearlyExhausted = object.bool("puk_nearly_exhausted")
    }
}
