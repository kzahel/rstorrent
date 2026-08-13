import SwiftUI

@main
struct RSTorrentApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            FoundationScreen()
                .environmentObject(model)
                .task { await model.start() }
        }
    }
}
