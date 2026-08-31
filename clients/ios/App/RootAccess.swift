import FileProvider
import Foundation
import RSTorrentIOS

enum RootAccessError: Error, LocalizedError {
    case securityScopeDenied
    case coordinationFailed(String)
    case coordinationAccessorDidNotRun
    case ineligible(RootEligibilityReason)
    case bookmarkTooLarge(Int)

    var errorDescription: String? {
        switch self {
        case .securityScopeDenied:
            return String(localized: "ios_root_error_security_scope")
        case .coordinationFailed(let detail):
            return String(
                format: String(localized: "ios_root_error_coordination"),
                locale: .current,
                detail
            )
        case .coordinationAccessorDidNotRun:
            return String(localized: "ios_root_error_coordinator_missing")
        case .ineligible(let reason):
            switch reason {
            case .ubiquitous:
                return String(localized: "ios_root_error_icloud")
            case .providerIdentified:
                return String(localized: "ios_root_error_file_provider")
            default:
                return String(
                    format: String(localized: "ios_root_error_ineligible"),
                    locale: .current,
                    reason.rawValue
                )
            }
        case .bookmarkTooLarge(let count):
            return String(
                format: String(localized: "ios_root_error_bookmark_size"),
                locale: .current,
                count.formatted(.byteCount(style: .file))
            )
        }
    }
}

struct QualifiedSelectedRoot {
    var coordinatedURL: URL
    var displayLabel: String
    var bookmarkData: Data
    var decision: RootEligibilityDecision
    var qualification: IosRootQualification
}

struct RestoredSelectedRoot {
    var coordinatedURL: URL
    var displayLabel: String
    var bookmarkData: Data
    var stale: Bool
    var decision: RootEligibilityDecision
}

enum RootAccess {
    static let providerDeadline: TimeInterval = 5

    static func qualifySelection(
        _ url: URL,
        registeredURLs: [URL]
    ) throws -> QualifiedSelectedRoot {
        try withSecurityScope(url) {
            let observed = try observe(url, registeredURLs: registeredURLs)
            guard observed.decision.isSupported else {
                throw RootAccessError.ineligible(observed.decision.reason)
            }
            var coordinationError: NSError?
            var qualification: Result<IosRootQualification, Error>?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: observed.url,
                options: .forMerging,
                error: &coordinationError
            ) { coordinatedURL in
                qualification = Result {
                    try qualifyRoot(rootPath: coordinatedURL.path)
                }
            }
            if let coordinationError {
                throw RootAccessError.coordinationFailed(
                    coordinationError.localizedDescription
                )
            }
            guard let qualification else {
                throw RootAccessError.coordinationAccessorDidNotRun
            }
            let bookmark = try minimalBookmark(for: observed.url)
            return QualifiedSelectedRoot(
                coordinatedURL: observed.url,
                displayLabel: observed.displayLabel,
                bookmarkData: bookmark,
                decision: observed.decision,
                qualification: try qualification.get()
            )
        }
    }

    static func restore(
        _ record: SelectedRootRecord,
        registeredURLs: [URL]
    ) throws -> RestoredSelectedRoot {
        var stale = false
        let url = try URL(
            resolvingBookmarkData: record.bookmarkData,
            options: [.withoutImplicitStartAccessing],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        )
        return try withSecurityScope(url) {
            let observed = try observe(url, registeredURLs: registeredURLs)
            guard observed.decision.isSupported else {
                throw RootAccessError.ineligible(observed.decision.reason)
            }
            return RestoredSelectedRoot(
                coordinatedURL: observed.url,
                displayLabel: observed.displayLabel,
                bookmarkData: stale ? try minimalBookmark(for: observed.url) : record.bookmarkData,
                stale: stale,
                decision: observed.decision
            )
        }
    }

    static func resolveBookmark(_ bookmarkData: Data) throws -> URL {
        var stale = false
        return try URL(
            resolvingBookmarkData: bookmarkData,
            options: [.withoutImplicitStartAccessing],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        )
    }

    static func withSecurityScope<T>(_ url: URL, body: () throws -> T) throws -> T {
        guard url.startAccessingSecurityScopedResource() else {
            throw RootAccessError.securityScopeDenied
        }
        defer { url.stopAccessingSecurityScopedResource() }
        return try body()
    }

    private struct ObservedRoot {
        var url: URL
        var displayLabel: String
        var decision: RootEligibilityDecision
    }

    private static func observe(
        _ url: URL,
        registeredURLs: [URL]
    ) throws -> ObservedRoot {
        let keys: Set<URLResourceKey> = [
            .isDirectoryKey,
            .isSymbolicLinkKey,
            .isUbiquitousItemKey,
            .volumeIsLocalKey,
            .volumeIsInternalKey,
            .nameKey,
            .localizedNameKey,
        ]
        var coordinationError: NSError?
        var result: Result<(URL, URLResourceValues), Error>?
        NSFileCoordinator(filePresenter: nil).coordinate(
            readingItemAt: url,
            options: [],
            error: &coordinationError
        ) { coordinatedURL in
            result = Result {
                (coordinatedURL, try coordinatedURL.resourceValues(forKeys: keys))
            }
        }
        if let coordinationError {
            throw RootAccessError.coordinationFailed(coordinationError.localizedDescription)
        }
        guard let result else {
            throw RootAccessError.coordinationAccessorDidNotRun
        }
        let (coordinatedURL, values) = try result.get()
        let observation = RootEligibilityObservation(
            isFileURL: coordinatedURL.isFileURL,
            isDirectory: values.isDirectory,
            isSymbolicLink: values.isSymbolicLink,
            overlapsRegisteredRoot: overlaps(
                coordinatedURL,
                registeredURLs: registeredURLs
            ),
            isUbiquitousItem: values.isUbiquitousItem,
            volumeIsLocal: values.volumeIsLocal,
            volumeIsInternal: values.volumeIsInternal,
            fileProviderLookup: fileProviderLookup(for: coordinatedURL)
        )
        return ObservedRoot(
            url: coordinatedURL,
            displayLabel: RootRegistryStore.boundedLabel(
                values.localizedName ?? values.name ?? "Selected on-device folder"
            ),
            decision: RootEligibility.decide(observation)
        )
    }

    private static func fileProviderLookup(for url: URL) -> RootProviderLookup {
        let semaphore = DispatchSemaphore(value: 0)
        let result = ProviderResultBox()
        NSFileProviderManager.getIdentifierForUserVisibleFile(at: url) {
            itemIdentifier,
            domainIdentifier,
            error in
            if itemIdentifier != nil || domainIdentifier != nil {
                result.complete(.identified)
            } else if error != nil {
                result.complete(.failed)
            } else {
                result.complete(.noIdentifier)
            }
            semaphore.signal()
        }
        guard semaphore.wait(timeout: .now() + providerDeadline) == .success else {
            return .timedOut
        }
        return result.value ?? .failed
    }

    private static func overlaps(_ url: URL, registeredURLs: [URL]) -> Bool? {
        for registered in registeredURLs {
            do {
                var first = FileManager.URLRelationship.other
                try FileManager.default.getRelationship(
                    &first,
                    ofDirectoryAt: registered,
                    toItemAt: url
                )
                if first == .same || first == .contains { return true }
                var second = FileManager.URLRelationship.other
                try FileManager.default.getRelationship(
                    &second,
                    ofDirectoryAt: url,
                    toItemAt: registered
                )
                if second == .same || second == .contains { return true }
            } catch {
                return nil
            }
        }
        return false
    }

    private static func minimalBookmark(for url: URL) throws -> Data {
        let bookmark = try url.bookmarkData(
            options: .minimalBookmark,
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
        guard bookmark.count <= RootRegistryStore.maximumBookmarkBytes else {
            throw RootAccessError.bookmarkTooLarge(bookmark.count)
        }
        return bookmark
    }
}

private final class ProviderResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: RootProviderLookup?

    var value: RootProviderLookup? {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func complete(_ value: RootProviderLookup) {
        lock.lock()
        defer { lock.unlock() }
        if stored == nil { stored = value }
    }
}
