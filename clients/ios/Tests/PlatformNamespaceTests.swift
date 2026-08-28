import Darwin
import RSTorrentIOS
import XCTest
@testable import RSTorrent

final class PlatformNamespaceTests: XCTestCase {
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
        let firstTarget = try PlatformStorageBridge.storageTarget(
            root: root,
            components: ["first.bin"]
        )
        let secondTarget = try PlatformStorageBridge.storageTarget(
            root: root,
            components: ["second.bin"]
        )

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

    func testExactTargetWriterSerializesTheSameFile() throws {
        let root = try temporaryRoot(named: "same-target")
        defer { try? FileManager.default.removeItem(at: root) }
        let target = try PlatformStorageBridge.storageTarget(
            root: root,
            components: ["same.bin"]
        )
        let firstEntered = DispatchSemaphore(value: 0)
        let releaseFirst = DispatchSemaphore(value: 0)
        let secondEntered = DispatchSemaphore(value: 0)
        let workers = DispatchGroup()

        workers.enter()
        DispatchQueue.global().async {
            defer { workers.leave() }
            var error: NSError?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: target,
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
                writingItemAt: target,
                options: .forMerging,
                error: &error
            ) { _ in secondEntered.signal() }
        }
        XCTAssertEqual(secondEntered.wait(timeout: .now() + 0.2), .timedOut)
        releaseFirst.signal()
        XCTAssertEqual(secondEntered.wait(timeout: .now() + 2), .success)
        XCTAssertEqual(workers.wait(timeout: .now() + 2), .success)
    }

    func testProductionDescriptorOpenCreatesNestedTargetAndRejectsParentSymlink() throws {
        let root = try temporaryRoot(named: "descriptor")
        let outside = try temporaryRoot(named: "outside")
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: outside)
        }

        let components = ["staging", "nested", "payload.bin"]
        let target = try PlatformStorageBridge.storageTarget(
            root: root,
            components: components
        )
        var coordinationError: NSError?
        var accessorResult: Result<Void, Error>?
        NSFileCoordinator(filePresenter: nil).coordinate(
            writingItemAt: target,
            options: .forMerging,
            error: &coordinationError
        ) { coordinatedTarget in
            accessorResult = Result {
                try PlatformStorageBridge.validateCoordinatedTarget(
                    coordinatedTarget,
                    requested: target
                )
                let descriptor = try PlatformStorageBridge.openDescriptor(
                    root: root,
                    components: components,
                    access: .readWriteCreate
                )
                Darwin.close(descriptor)
            }
        }
        XCTAssertNil(coordinationError)
        try XCTUnwrap(accessorResult).get()
        XCTAssertTrue(FileManager.default.fileExists(atPath: target.path))

        let link = root.appendingPathComponent("linked")
        XCTAssertEqual(Darwin.symlink(outside.path, link.path), 0)
        XCTAssertThrowsError(
            try PlatformStorageBridge.openDescriptor(
                root: root,
                components: ["linked", "escape.bin"],
                access: .readWriteCreate
            )
        )
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: outside.appendingPathComponent("escape.bin").path
            )
        )
    }

    func testCoordinatedTargetSubstitutionFailsClosed() throws {
        let root = try temporaryRoot(named: "substitution")
        defer { try? FileManager.default.removeItem(at: root) }
        let requested = try PlatformStorageBridge.storageTarget(
            root: root,
            components: ["requested.bin"]
        )
        let substituted = root.appendingPathComponent("substituted.bin")

        XCTAssertThrowsError(
            try PlatformStorageBridge.validateCoordinatedTarget(
                substituted,
                requested: requested
            )
        )
    }

    func testRootObservationSupportsTheEmptyHealthProbePath() throws {
        let root = try temporaryRoot(named: "root-observation")
        defer { try? FileManager.default.removeItem(at: root) }

        XCTAssertEqual(
            try PlatformStorageBridge.storageTarget(root: root, components: []),
            root
        )
        let observation = try PlatformStorageBridge.observe(root: root, components: [])
        XCTAssertTrue(observation.exists)
        XCTAssertEqual(observation.kind, .directory)
        XCTAssertNil(observation.length)
        XCTAssertThrowsError(
            try PlatformStorageBridge.openDescriptor(
                root: root,
                components: [],
                access: .readWriteCreate
            )
        )
    }

    func testObservationAndDeleteDoNotFollowSymlinks() throws {
        let root = try temporaryRoot(named: "observe-delete")
        let outside = try temporaryRoot(named: "observe-delete-outside")
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: outside)
        }
        let payload = root.appendingPathComponent("payload.bin")
        try Data([1, 2, 3, 4]).write(to: payload)

        let present = try PlatformStorageBridge.observe(
            root: root,
            components: ["payload.bin"]
        )
        XCTAssertTrue(present.exists)
        XCTAssertEqual(present.kind, .file)
        XCTAssertEqual(present.length, 4)

        let missing = try PlatformStorageBridge.observe(
            root: root,
            components: ["missing", "payload.bin"]
        )
        XCTAssertFalse(missing.exists)

        let link = root.appendingPathComponent("outside-link")
        XCTAssertEqual(Darwin.symlink(outside.path, link.path), 0)
        let linked = try PlatformStorageBridge.observe(
            root: root,
            components: ["outside-link"]
        )
        XCTAssertTrue(linked.exists)
        XCTAssertEqual(linked.kind, .other)
        try PlatformStorageBridge.delete(root: root, components: ["outside-link"])
        XCTAssertFalse(FileManager.default.fileExists(atPath: link.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: outside.path))
        try PlatformStorageBridge.delete(root: root, components: ["outside-link"])
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
