import Foundation

// The firmware is not consistent about JSON types. `lte_rsrp` arrives as a
// number, `mdm_mcc` as the string "413", `pin_status` as "0", and the same key
// can change type between firmware builds. Decoding it as whatever Swift type
// seems natural gives a decoding error, or worse, silently drops the field.
//
// The Android app got the mirror image of this wrong and it cost most of a day:
// its client tried intOrNull before falling back to the string, so quoted values
// were retyped as numbers and every `as? String` on them returned nil. Half the
// SIM screen read "--". An ICCID ending in F was the memorable one, because
// Java's parseDouble accepts a trailing F as a float suffix and turned it into
// 8.99e18.
//
// So: never infer the type from the contents. Read the JSON type that is there,
// and convert only when a caller asks for a specific one.

/// A decoded JSON scalar with its original type intact.
public enum AgentValue: Decodable, Equatable, Sendable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case null

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else {
            self = .null
        }
    }

    /// The value as text, whatever it arrived as.
    ///
    /// Numbers that are whole are rendered without a decimal point, so a slot
    /// number does not display as "1.0".
    public var stringValue: String? {
        switch self {
        case .string(let s): return s
        case .number(let d): return d == d.rounded() && abs(d) < 1e15
            ? String(Int64(d))
            : String(d)
        case .bool(let b): return b ? "1" : "0"
        case .null: return nil
        }
    }

    /// The value as a number, parsing a string only if it is entirely numeric.
    ///
    /// Deliberately stricter than Double(_:), which accepts "1F", "0x1p3",
    /// "infinity" and "nan". An identifier that happens to end in F is not a
    /// number, and treating it as one is the exact bug this type exists to
    /// prevent.
    public var doubleValue: Double? {
        switch self {
        case .number(let d): return d
        case .bool(let b): return b ? 1 : 0
        case .null: return nil
        case .string(let s):
            let trimmed = s.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { return nil }
            var seenDot = false
            for (index, character) in trimmed.enumerated() {
                if character == "-" && index == 0 { continue }
                if character == "." && !seenDot { seenDot = true; continue }
                guard character.isASCII, character.isNumber else { return nil }
            }
            return Double(trimmed)
        }
    }

    public var intValue: Int? {
        guard let value = doubleValue, value.isFinite else { return nil }
        guard value >= Double(Int.min), value <= Double(Int.max) else { return nil }
        return Int(value)
    }

    /// The firmware's several spellings of yes.
    public var boolValue: Bool {
        switch self {
        case .bool(let b): return b
        case .number(let d): return d != 0
        case .null: return false
        case .string(let s):
            let lowered = s.lowercased()
            return lowered == "1" || lowered == "true" || lowered == "on" || lowered == "yes"
        }
    }

    public var isEmpty: Bool {
        switch self {
        case .null: return true
        case .string(let s): return s.isEmpty
        default: return false
        }
    }
}

/// An untyped object from the agent, keyed as the firmware sends it.
///
/// Used for the vendor passthrough routes, whose shape is the firmware's to
/// decide and changes between builds. Screens read the keys they need and
/// tolerate the rest being absent, which is the only thing that survives a
/// firmware update.
public struct AgentObject: Decodable, Sendable {
    public let fields: [String: AgentValue]

    private struct Key: CodingKey {
        let stringValue: String
        var intValue: Int? { nil }
        init?(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { nil }
    }

    public init(from decoder: Decoder) throws {
        // A route can legitimately answer with a list or a scalar; that is not
        // an error worth throwing, it just means no fields to read.
        guard let container = try? decoder.container(keyedBy: Key.self) else {
            fields = [:]
            return
        }
        var found: [String: AgentValue] = [:]
        for key in container.allKeys {
            found[key.stringValue] = (try? container.decode(AgentValue.self, forKey: key)) ?? .null
        }
        fields = found
    }

    public init(_ fields: [String: AgentValue]) { self.fields = fields }

    public subscript(key: String) -> AgentValue { fields[key] ?? .null }

    public func string(_ key: String) -> String? { self[key].stringValue }
    public func int(_ key: String) -> Int? { self[key].intValue }
    public func double(_ key: String) -> Double? { self[key].doubleValue }
    public func bool(_ key: String) -> Bool { self[key].boolValue }
}
