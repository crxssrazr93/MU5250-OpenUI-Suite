import SwiftUI
import OpenU60Kit

struct DashboardView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                if let message = model.message {
                    Section { Text(message).font(.footnote).foregroundStyle(.red) }
                }

                Section("Signal") {
                    if let signal = model.signal {
                        LabeledContent("Operator", value: signal.carrier.orDash)
                        LabeledContent("Network", value: signal.networkType.orDash)
                        rsrpRow(signal)
                        if let bars = signal.bars {
                            LabeledContent("Bars", value: "\(bars)/5")
                        }
                        if !signal.lteBand.isEmpty {
                            LabeledContent("LTE band", value: signal.lteBand)
                        }
                        // Only when the modem reports NR as well: an NR reading
                        // with no NSA link is a measurement, not a connection.
                        if signal.isNSA, let nr = signal.nrRSRP {
                            LabeledContent("NR RSRP", value: "\(Int(nr)) dBm")
                        }
                    } else {
                        Text("—").foregroundStyle(.secondary)
                    }
                }

                Section("Connection") {
                    if let connection = model.connection {
                        LabeledContent("Status", value: connection.connectStatus.orDash)
                        LabeledContent("Mobile data", value: connection.enabled ? "On" : "Off")
                        LabeledContent("Roaming", value: connection.roamingEnabled ? "On" : "Off")
                        LabeledContent("IPv4", value: connection.ipv4.orDash)
                        if !connection.dns.isEmpty {
                            LabeledContent("DNS", value: connection.dns.joined(separator: ", "))
                        }
                    }
                }

                Section("Battery") {
                    if let battery = model.battery {
                        LabeledContent("Charge", value: battery.capacity.map { "\($0)%" } ?? "—")
                        LabeledContent("State", value: battery.charging ? "Charging" : "On battery")
                        if let temperature = battery.temperature {
                            LabeledContent("Temperature", value: "\(Int(temperature))°C")
                        }
                    }
                }

                Section("SIM") {
                    NavigationLink("SIM details") { SIMView() }
                    if let sim = model.sim {
                        LabeledContent("Status", value: sim.status.orDash)
                        LabeledContent("SPN", value: sim.spn.orDash)
                    }
                }

                Section {
                    Button("Sign out", role: .destructive) { Task { await model.signOut() } }
                }
            }
            .navigationTitle("Overview")
            .refreshable { await model.refresh() }
            .task { await model.refresh() }
        }
    }

    @ViewBuilder
    private func rsrpRow(_ signal: SignalInfo) -> some View {
        if let rsrp = signal.primaryRSRP {
            LabeledContent("RSRP") {
                HStack(spacing: 6) {
                    Text("\(Int(rsrp)) dBm").monospacedDigit()
                    Text(signal.quality.rawValue.capitalized)
                        .font(.caption)
                        .foregroundStyle(color(for: signal.quality))
                }
            }
        }
    }

    private func color(for quality: SignalQuality) -> Color {
        switch quality {
        case .excellent, .good: return .green
        case .fair: return .orange
        case .poor: return .red
        case .unknown: return .secondary
        }
    }
}

extension String {
    /// Blank fields are common and mean the firmware has nothing, not that
    /// something failed — shown as a dash rather than an empty row.
    var orDash: String { isEmpty ? "—" : self }
}
