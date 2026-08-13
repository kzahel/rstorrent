import QuickLook
import SwiftUI

struct SystemFilePreview: UIViewControllerRepresentable {
    let url: URL
    let onDone: () -> Void

    func makeCoordinator() -> SystemFilePreviewCoordinator {
        SystemFilePreviewCoordinator(url: url, onDone: onDone)
    }

    func makeUIViewController(context: Context) -> UINavigationController {
        let preview = QLPreviewController()
        preview.dataSource = context.coordinator
        preview.navigationItem.leftBarButtonItem = UIBarButtonItem(
            barButtonSystemItem: .done,
            target: context.coordinator,
            action: #selector(SystemFilePreviewCoordinator.finish)
        )
        return UINavigationController(rootViewController: preview)
    }

    func updateUIViewController(
        _ uiViewController: UINavigationController,
        context: Context
    ) {}
}

@MainActor
final class SystemFilePreviewCoordinator: NSObject, QLPreviewControllerDataSource {
    private let url: URL
    private let onDone: () -> Void

    init(url: URL, onDone: @escaping () -> Void) {
        self.url = url
        self.onDone = onDone
    }

    func numberOfPreviewItems(in controller: QLPreviewController) -> Int {
        1
    }

    func previewController(
        _ controller: QLPreviewController,
        previewItemAt index: Int
    ) -> QLPreviewItem {
        precondition(index == 0)
        return url as NSURL
    }

    @objc func finish() {
        onDone()
    }
}
