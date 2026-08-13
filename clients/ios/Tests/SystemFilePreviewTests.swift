import QuickLook
import XCTest
@testable import RSTorrent

@MainActor
final class SystemFilePreviewTests: XCTestCase {
    func testDataSourceExposesOnlyTheExactLeasedURL() {
        let url = URL(fileURLWithPath: "/tmp/rstorrent-preview/video.mp4")
        let coordinator = SystemFilePreviewCoordinator(url: url, onDone: {})
        let controller = QLPreviewController()

        XCTAssertEqual(coordinator.numberOfPreviewItems(in: controller), 1)
        let item = coordinator.previewController(controller, previewItemAt: 0)
        XCTAssertEqual(item as? NSURL, url as NSURL)
    }

    func testDoneActionRequestsPresentationDismissal() {
        var dismissCount = 0
        let coordinator = SystemFilePreviewCoordinator(
            url: URL(fileURLWithPath: "/tmp/rstorrent-preview/video.mp4"),
            onDone: { dismissCount += 1 }
        )

        coordinator.finish()

        XCTAssertEqual(dismissCount, 1)
    }
}
