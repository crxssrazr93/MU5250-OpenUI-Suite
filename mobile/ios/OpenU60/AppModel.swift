import Foundation
import OpenU60Kit

/// One place holding the client, the session, and what the screens are showing.
///
/// The dashboard reads several routes that the agent serves separately, and the
/// agent serializes some of them, so they are fetched in sequence rather than
/// concurrently. Firing them all at once produced a queue on the device, not a
/// faster screen.
@MainActor
final class AppModel: ObservableObject {

    enum Stage: Equatable {
        case chooseRouter
        case signIn
        case ready
    }

    @Published private(set) var stage: Stage = .chooseRouter
    @Published private(set) var signal: SignalInfo?
    @Published private(set) var battery: BatteryInfo?
    @Published private(set) var connection: DataConnection?
    @Published private(set) var sim: SIMInfo?
    @Published private(set) var lock: SIMLockInfo?
    @Published var message: String?
    @Published private(set) var isBusy = false

    private let client = AgentClient()
    private let hostKey = "agent_host"

    init() {
        if let saved = UserDefaults.standard.string(forKey: hostKey) {
            Task { await adopt(host: saved) }
        }
    }

    private func adopt(host: String) async {
        if await client.setHost(host) {
            stage = .signIn
        }
    }

    /// Returns false for anything outside the private ranges, so the form can
    /// say so rather than the user waiting on a request that cannot work.
    func connect(to host: String) async -> Bool {
        guard await client.setHost(host) else { return false }
        UserDefaults.standard.set(host, forKey: hostKey)
        stage = .signIn
        return true
    }

    func forgetRouter() {
        UserDefaults.standard.removeObject(forKey: hostKey)
        stage = .chooseRouter
    }

    func signIn(password: String) async {
        isBusy = true
        defer { isBusy = false }
        do {
            try await client.login(password: password)
            stage = .ready
            await refresh()
        } catch {
            message = error.localizedDescription
        }
    }

    func signOut() async {
        await client.setToken(nil)
        stage = .signIn
    }

    func refresh() async {
        isBusy = true
        defer { isBusy = false }
        do {
            signal = SignalInfo(from: try await client.get("/api/network/signal"))
            battery = BatteryInfo(from: try await client.get("/api/device/battery-info"))
            connection = DataConnection(from: try await client.get("/api/modem/data"))
            sim = SIMInfo(from: try await client.get("/api/sim/info"))
            lock = SIMLockInfo(from: try await client.get("/api/sim/lock"))
            message = nil
        } catch AgentError.unauthorized {
            stage = .signIn
        } catch {
            message = error.localizedDescription
        }
    }

    var routerAddress: String {
        get async { await client.currentHost() ?? "" }
    }
}
