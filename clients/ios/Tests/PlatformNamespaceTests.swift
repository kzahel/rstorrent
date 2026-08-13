import XCTest
@testable import RSTorrent

final class PlatformNamespaceTests: XCTestCase {
    func testManagedArtifactsAreExactlyPublishedStagingAndParts() {
        XCTAssertEqual(
            PlatformStorageBridge.managedArtifactNames(
                torrentID: "0123456789abcdef",
                publishedName: "Example Download"
            ),
            [
                "Example Download",
                ".0123456789abcdef.rstorrent-staging",
                ".0123456789abcdef.rstorrent-parts",
            ]
        )
    }
}
