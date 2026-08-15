import SwiftUI

@main
struct OpenU60App: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(model)
                .preferredColorScheme(.dark)
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        switch model.stage {
        case .chooseRouter:
            ConnectRouterView()
        case .signIn:
            LoginView()
        case .ready:
            DashboardView()
        }
    }
}
