import SwiftUI

private enum AppRoute: Hashable {
    case torrent(String)
    case settings
}

struct ContentView: View {
    @ObservedObject var lifecycle: IOSApplicationLifecycleOwner
    @ObservedObject var appModel: AppModel
    @ObservedObject var presentation: IOSPresentationRepository
    @State private var path: [AppRoute] = []

    var body: some View {
        NavigationStack(path: $path) {
            TorrentListScreen(
                appModel: appModel,
                presentation: presentation,
                onOpenSettings: { path.append(.settings) },
                onTorrentSelected: { path.append(.torrent($0)) }
            )
            .navigationDestination(for: AppRoute.self) { route in
                switch route {
                case .torrent(let torrentID):
                    TorrentDetailScreen(
                        appModel: appModel,
                        presentation: presentation,
                        torrentID: torrentID
                    )
                case .settings:
                    SettingsScreen(
                        lifecycle: lifecycle,
                        appModel: appModel,
                        presentation: presentation
                    )
                }
            }
        }
    }
}
