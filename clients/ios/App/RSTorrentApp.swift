import SwiftUI
import UIKit

final class RSTorrentAppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        IOSBackgroundTaskBridge.shared.register()
        if let url = launchOptions?[.url] as? URL {
            IOSExternalInputBridge.shared.deliver(url)
        }
        return true
    }

    func application(
        _ app: UIApplication,
        open url: URL,
        options: [UIApplication.OpenURLOptionsKey: Any] = [:]
    ) -> Bool {
        IOSExternalInputBridge.shared.deliver(url)
        return true
    }
}

@main
struct RSTorrentApp: App {
    @UIApplicationDelegateAdaptor(RSTorrentAppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var lifecycle = IOSApplicationLifecycleOwner()

    var body: some Scene {
        WindowGroup {
            ContentView(
                lifecycle: lifecycle,
                appModel: lifecycle.model,
                presentation: lifecycle.model.presentation
            )
            .task { await lifecycle.start() }
            .onOpenURL { lifecycle.receive($0) }
            .onChange(of: scenePhase) { lifecycle.scenePhaseDidChange($0) }
        }
    }
}
