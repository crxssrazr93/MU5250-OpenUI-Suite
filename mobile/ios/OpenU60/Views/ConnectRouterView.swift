import SwiftUI

/// Which router to talk to.
///
/// iOS asks the user for permission the first time an app touches the local
/// network, and that prompt appears on the first request, not here. So this
/// screen only records the address; a refusal shows up as a failure to sign in.
/// `NSLocalNetworkUsageDescription` has to be set or the request fails with no
/// prompt at all.
struct ConnectRouterView: View {
    @EnvironmentObject private var model: AppModel
    @State private var address = "192.168.0.1"
    @State private var error: String?

    var body: some View {
        VStack(spacing: 20) {
            Spacer()

            Image(systemName: "wifi.router")
                .font(.system(size: 44))
                .foregroundStyle(.tint)
            Text("ZTE U60 Pro").font(.title2.bold())
            Text("Which router should this connect to?")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 12) {
                TextField("192.168.0.1", text: $address)
                    .textFieldStyle(.roundedBorder)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.numbersAndPunctuation)
                    .onChange(of: address) { _ in error = nil }

                Text("The router's LAN address. The agent's port is added automatically.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if let error {
                    Text(error).font(.caption).foregroundStyle(.red)
                }

                Button("Continue") {
                    Task {
                        if await model.connect(to: address) == false {
                            error = "Use the router's address on your own network, such as 192.168.0.1"
                        }
                    }
                }
                .buttonStyle(.borderedProminent)
                .frame(maxWidth: .infinity)
                .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding()
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 14))

            Spacer()
        }
        .padding()
    }
}
