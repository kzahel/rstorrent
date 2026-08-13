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

    func testRootWideWriterBlocksSiblingFileCoordination() throws {
        let root = try temporaryRoot(named: "root-wide")
        defer { try? FileManager.default.removeItem(at: root) }

        let firstEntered = DispatchSemaphore(value: 0)
        let releaseFirst = DispatchSemaphore(value: 0)
        let secondEntered = DispatchSemaphore(value: 0)
        let workers = DispatchGroup()

        workers.enter()
        DispatchQueue.global().async {
            defer { workers.leave() }
            var error: NSError?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: root,
                options: .forMerging,
                error: &error
            ) { _ in
                firstEntered.signal()
                releaseFirst.wait()
            }
        }
        XCTAssertEqual(firstEntered.wait(timeout: .now() + 2), .success)

        workers.enter()
        DispatchQueue.global().async {
            defer { workers.leave() }
            var error: NSError?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: root,
                options: .forMerging,
                error: &error
            ) { _ in secondEntered.signal() }
        }

        XCTAssertEqual(secondEntered.wait(timeout: .now() + 0.2), .timedOut)
        releaseFirst.signal()
        XCTAssertEqual(secondEntered.wait(timeout: .now() + 2), .success)
        XCTAssertEqual(workers.wait(timeout: .now() + 2), .success)
    }

    func testExactTargetWriterAllowsConcurrentSiblingTarget() throws {
        let root = try temporaryRoot(named: "exact-target")
        defer { try? FileManager.default.removeItem(at: root) }
        let firstTarget = root.appendingPathComponent("first.bin")
        let secondTarget = root.appendingPathComponent("second.bin")

        let firstEntered = DispatchSemaphore(value: 0)
        let releaseFirst = DispatchSemaphore(value: 0)
        let secondEntered = DispatchSemaphore(value: 0)
        let workers = DispatchGroup()

        workers.enter()
        DispatchQueue.global().async {
            defer { workers.leave() }
            var error: NSError?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: firstTarget,
                options: .forMerging,
                error: &error
            ) { _ in
                firstEntered.signal()
                releaseFirst.wait()
            }
        }
        XCTAssertEqual(firstEntered.wait(timeout: .now() + 2), .success)

        workers.enter()
        DispatchQueue.global().async {
            defer { workers.leave() }
            var error: NSError?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: secondTarget,
                options: .forMerging,
                error: &error
            ) { _ in secondEntered.signal() }
        }

        XCTAssertEqual(secondEntered.wait(timeout: .now() + 2), .success)
        releaseFirst.signal()
        XCTAssertEqual(workers.wait(timeout: .now() + 2), .success)
    }

    private func temporaryRoot(named name: String) throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("rstorrent-\(name)-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(
            at: root,
            withIntermediateDirectories: false
        )
        return root
    }
}
