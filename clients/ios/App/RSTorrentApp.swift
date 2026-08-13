import SwiftUI

@main
struct RSTorrentApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            ContentView(appModel: model, presentation: model.presentation)
                .task { await model.start() }
        }
    }
}
