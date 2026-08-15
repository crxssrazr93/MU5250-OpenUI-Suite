import SwiftUI
import OpenU60Kit

struct SIMView: View {
    @EnvironmentObject private var model: AppModel
    /// Identifiers are hidden until asked for. They identify the subscriber,
    /// and this screen is as likely to be open in a screenshot as anywhere.
    @State private var revealed = false

    var body: some View {
        List {
            Section("SIM") {
                if let sim = model.sim {
                    LabeledContent("Status", value: sim.status.orDash)
                    LabeledContent("Operator", value: sim.operatorName.orDash)
                    LabeledContent("SPN", value: sim.spn.orDash)
                    LabeledContent("MCC/MNC", value: pair(sim.mcc, sim.mnc))
                    LabeledContent("Slot", value: sim.slot.orDash)
                    LabeledContent("ICCID", value: masked(sim.iccid))
                    LabeledContent("IMSI", value: masked(sim.imsi))
                    LabeledContent("MSISDN", value: masked(sim.msisdn))
                }
            }

            Section {
                Toggle("Show identifiers", isOn: $revealed)
            }

            Section("Lock") {
                if let lock = model.lock {
                    LabeledContent("PIN", value: lock.status.orDash)
                    LabeledContent("PIN attempts", value: count(lock.pinAttemptsLeft))
                    LabeledContent("PUK attempts", value: count(lock.pukAttemptsLeft))
                    LabeledContent("Network unlock attempts", value: count(lock.nckAttemptsLeft))
                    if lock.pukNearlyExhausted {
                        Text("Few PUK attempts remain. Running out locks the SIM permanently.")
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
            }
        }
        .navigationTitle("SIM")
        .refreshable { await model.refresh() }
    }

    private func pair(_ first: String, _ second: String) -> String {
        first.isEmpty && second.isEmpty ? "—" : "\(first)/\(second)"
    }

    /// Unknown is not zero. Zero attempts left is a specific and alarming
    /// claim, and it should only be made when the firmware actually said it.
    private func count(_ value: Int?) -> String {
        value.map(String.init) ?? "—"
    }

    private func masked(_ value: String) -> String {
        guard !value.isEmpty else { return "—" }
        guard !revealed else { return value }
        guard value.count > 4 else { return String(repeating: "•", count: value.count) }
        return String(repeating: "•", count: value.count - 4) + value.suffix(4)
    }
}
