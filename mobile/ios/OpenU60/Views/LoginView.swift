import SwiftUI

struct LoginView: View {
    @EnvironmentObject private var model: AppModel
    @State private var password = ""

    var body: some View {
        VStack(spacing: 20) {
            Spacer()

            Image(systemName: "wifi.router")
                .font(.system(size: 44))
                .foregroundStyle(.tint)
            Text("ZTE U60 Pro").font(.title2.bold())
            Text("Sign in to the dashboard")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            VStack(spacing: 12) {
                SecureField("Agent password", text: $password)
                    .textFieldStyle(.roundedBorder)
                    .textContentType(.password)
                    .submitLabel(.go)
                    .onSubmit { submit() }

                if let message = model.message {
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.leading)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                Button(action: submit) {
                    if model.isBusy {
                        ProgressView().frame(maxWidth: .infinity)
                    } else {
                        Text("Sign in").frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(password.isEmpty || model.isBusy)
            }
            .padding()
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 14))

            Button("Use a different router") { model.forgetRouter() }
                .font(.footnote)

            Spacer()
        }
        .padding()
    }

    private func submit() {
        guard !password.isEmpty else { return }
        Task { await model.signIn(password: password) }
    }
}
