import FileProvider
import Foundation

struct ProbeResourceSnapshot: Codable, Equatable {
    var currentSecurityScopes: Int
    var securityScopeHighWater: Int
    var currentCoordinators: Int
    var coordinatorHighWater: Int
    var currentEligibilityRequests: Int
    var eligibilityRequestHighWater: Int
    var currentRootOperations: Int
    var rootOperationHighWater: Int
}

enum ProbeResourceKind {
    case securityScope
    case coordinator
    case eligibilityRequest
    case rootOperation
}

enum ProbeRootAccessError: Error, LocalizedError {
    case nonFileURL
    case resourceLimit(String)
    case securityScopeDenied
    case coordinationFailed(String)
    case coordinationAccessorDidNotRun
    case resourceObservationFailed(String)
    case bookmarkTooLarge(Int)
    case rustOperationFailed(String)

    var errorDescription: String? {
        switch self {
        case .nonFileURL:
            return "selected root is not a file URL"
        case .resourceLimit(let resource):
            return "concurrent \(resource) limit reached"
        case .securityScopeDenied:
            return "security-scoped access was denied"
        case .coordinationFailed(let detail):
            return "file coordination failed: \(detail)"
        case .coordinationAccessorDidNotRun:
            return "file coordinator accessor did not run"
        case .resourceObservationFailed(let detail):
            return "root observation failed: \(detail)"
        case .bookmarkTooLarge(let count):
            return "bookmark has \(count) bytes"
        case .rustOperationFailed(let detail):
            return "Rust operation failed: \(detail)"
        }
    }

    var evidenceCode: String {
        switch self {
        case .nonFileURL:
            return "non_file_url"
        case .resourceLimit:
            return "resource_limit"
        case .securityScopeDenied:
            return "security_scope_denied"
        case .coordinationFailed:
            return "coordination_failed"
        case .coordinationAccessorDidNotRun:
            return "coordination_accessor_did_not_run"
        case .resourceObservationFailed:
            return "resource_observation_failed"
        case .bookmarkTooLarge:
            return "bookmark_too_large"
        case .rustOperationFailed:
            return "rust_operation_failed"
        }
    }
}

final class ProbeResourceLedger: @unchecked Sendable {
    static let shared = ProbeResourceLedger()

    private let lock = NSLock()
    private var securityScopes = 0
    private var securityScopeHighWater = 0
    private var coordinators = 0
    private var coordinatorHighWater = 0
    private var eligibilityRequests = 0
    private var eligibilityRequestHighWater = 0
    private var rootOperations = 0
    private var rootOperationHighWater = 0

    func withResource<T>(_ kind: ProbeResourceKind, _ body: () throws -> T) throws -> T {
        try enter(kind)
        defer { leave(kind) }
        return try body()
    }

    func snapshot() -> ProbeResourceSnapshot {
        lock.withLock {
            ProbeResourceSnapshot(
                currentSecurityScopes: securityScopes,
                securityScopeHighWater: securityScopeHighWater,
                currentCoordinators: coordinators,
                coordinatorHighWater: coordinatorHighWater,
                currentEligibilityRequests: eligibilityRequests,
                eligibilityRequestHighWater: eligibilityRequestHighWater,
                currentRootOperations: rootOperations,
                rootOperationHighWater: rootOperationHighWater
            )
        }
    }

    func evidence() -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return (try? encoder.encode(snapshot()))
            .flatMap { String(data: $0, encoding: .utf8) }
            ?? "encoding-failed"
    }

    private func enter(_ kind: ProbeResourceKind) throws {
        try lock.withLock {
            switch kind {
            case .securityScope:
                guard securityScopes == 0 else {
                    throw ProbeRootAccessError.resourceLimit("security scope")
                }
                securityScopes += 1
                securityScopeHighWater = max(securityScopeHighWater, securityScopes)
            case .coordinator:
                guard coordinators == 0 else {
                    throw ProbeRootAccessError.resourceLimit("coordinator")
                }
                coordinators += 1
                coordinatorHighWater = max(coordinatorHighWater, coordinators)
            case .eligibilityRequest:
                guard eligibilityRequests == 0 else {
                    throw ProbeRootAccessError.resourceLimit("eligibility request")
                }
                eligibilityRequests += 1
                eligibilityRequestHighWater = max(
                    eligibilityRequestHighWater,
                    eligibilityRequests
                )
            case .rootOperation:
                guard rootOperations == 0 else {
                    throw ProbeRootAccessError.resourceLimit("root operation")
                }
                rootOperations += 1
                rootOperationHighWater = max(rootOperationHighWater, rootOperations)
            }
        }
    }

    private func leave(_ kind: ProbeResourceKind) {
        lock.withLock {
            switch kind {
            case .securityScope:
                securityScopes -= 1
            case .coordinator:
                coordinators -= 1
            case .eligibilityRequest:
                eligibilityRequests -= 1
            case .rootOperation:
                rootOperations -= 1
            }
        }
    }
}

struct ProbeObservedRoot {
    var coordinatedURL: URL
    var displayLabel: String
    var observation: ProbeRootEligibilityObservation
    var decision: ProbeRootEligibilityDecision
}

struct ProbeSelectedRootResult {
    var observed: ProbeObservedRoot
    var bookmarkData: Data?
    var stale: Bool
    var rustResult: RustResult?
}

enum ProbeRootAccess {
    static let eligibilityDeadline: TimeInterval = 5

    static func runAppOwned(at url: URL) throws -> (ProbeObservedRoot, RustResult) {
        return try ProbeResourceLedger.shared.withResource(.rootOperation) {
            let observed = try observeResourceValues(
                at: url,
                provenance: .appOwned,
                queryFileProvider: false
            )
            guard observed.decision.classification == .appOwned else {
                throw ProbeRootAccessError.resourceObservationFailed(
                    observed.decision.reason.rawValue
                )
            }
            let rust = try coordinatedRustOperation(at: url, prepareInterruption: false)
            return (observed, rust)
        }
    }

    static func prepareAppOwnedInterruption(at url: URL) throws -> RustResult {
        try ProbeResourceLedger.shared.withResource(.rootOperation) {
            try coordinatedRustOperation(at: url, prepareInterruption: true)
        }
    }

    static func inspectSelected(_ url: URL) throws -> ProbeSelectedRootResult {
        guard url.isFileURL else {
            throw ProbeRootAccessError.nonFileURL
        }
        return try ProbeResourceLedger.shared.withResource(.rootOperation) {
            try withSecurityScope(url) {
                let observed = try observeResourceValues(
                    at: url,
                    provenance: .picker,
                    queryFileProvider: true
                )
                guard observed.decision.classification == .selectedOnDevice else {
                    return ProbeSelectedRootResult(
                        observed: observed,
                        bookmarkData: nil,
                        stale: false,
                        rustResult: nil
                    )
                }
                let bookmark = try createMinimalBookmark(for: observed.coordinatedURL)
                let rust = try coordinatedRustOperation(
                    at: observed.coordinatedURL,
                    prepareInterruption: false
                )
                return ProbeSelectedRootResult(
                    observed: observed,
                    bookmarkData: bookmark,
                    stale: false,
                    rustResult: rust
                )
            }
        }
    }

    static func classifySelected(_ url: URL) throws -> ProbeObservedRoot {
        guard url.isFileURL else {
            throw ProbeRootAccessError.nonFileURL
        }
        return try ProbeResourceLedger.shared.withResource(.rootOperation) {
            try withSecurityScope(url) {
                try observeResourceValues(
                    at: url,
                    provenance: .picker,
                    queryFileProvider: true
                )
            }
        }
    }

    static func restoreSelected(bookmarkData: Data) throws -> ProbeSelectedRootResult {
        var stale = false
        let url = try URL(
            resolvingBookmarkData: bookmarkData,
            options: [.withoutImplicitStartAccessing],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        )
        return try ProbeResourceLedger.shared.withResource(.rootOperation) {
            try withSecurityScope(url) {
                let observed = try observeResourceValues(
                    at: url,
                    provenance: .picker,
                    queryFileProvider: true
                )
                guard observed.decision.classification == .selectedOnDevice else {
                    return ProbeSelectedRootResult(
                        observed: observed,
                        bookmarkData: nil,
                        stale: stale,
                        rustResult: nil
                    )
                }
                let bookmark = stale
                    ? try createMinimalBookmark(for: observed.coordinatedURL)
                    : bookmarkData
                let rust = try coordinatedRustOperation(
                    at: observed.coordinatedURL,
                    prepareInterruption: false
                )
                return ProbeSelectedRootResult(
                    observed: observed,
                    bookmarkData: bookmark,
                    stale: stale,
                    rustResult: rust
                )
            }
        }
    }

    static func prepareSelectedInterruption(bookmarkData: Data) throws -> ProbeSelectedRootResult {
        var stale = false
        let url = try URL(
            resolvingBookmarkData: bookmarkData,
            options: [.withoutImplicitStartAccessing],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        )
        return try ProbeResourceLedger.shared.withResource(.rootOperation) {
            try withSecurityScope(url) {
                let observed = try observeResourceValues(
                    at: url,
                    provenance: .picker,
                    queryFileProvider: true
                )
                guard observed.decision.classification == .selectedOnDevice else {
                    return ProbeSelectedRootResult(
                        observed: observed,
                        bookmarkData: nil,
                        stale: stale,
                        rustResult: nil
                    )
                }
                let bookmark = stale
                    ? try createMinimalBookmark(for: observed.coordinatedURL)
                    : bookmarkData
                let rust = try coordinatedRustOperation(
                    at: observed.coordinatedURL,
                    prepareInterruption: true
                )
                return ProbeSelectedRootResult(
                    observed: observed,
                    bookmarkData: bookmark,
                    stale: stale,
                    rustResult: rust
                )
            }
        }
    }

    private static func withSecurityScope<T>(_ url: URL, _ body: () throws -> T) throws -> T {
        guard url.startAccessingSecurityScopedResource() else {
            throw ProbeRootAccessError.securityScopeDenied
        }
        defer { url.stopAccessingSecurityScopedResource() }
        return try ProbeResourceLedger.shared.withResource(.securityScope, body)
    }

    private static func observeResourceValues(
        at url: URL,
        provenance: ProbeRootProvenance,
        queryFileProvider: Bool
    ) throws -> ProbeObservedRoot {
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
        var accessorRan = false
        var accessorResult: Result<(URL, URLResourceValues, Bool?), Error>?
        let coordinator = NSFileCoordinator(filePresenter: nil)
        try ProbeResourceLedger.shared.withResource(.coordinator) {
            coordinator.coordinate(
                readingItemAt: url,
                options: [],
                error: &coordinationError
            ) { coordinatedURL in
                accessorRan = true
                accessorResult = Result {
                    (
                        coordinatedURL,
                        try coordinatedURL.resourceValues(forKeys: keys),
                        appOwnedOverlap(
                            for: coordinatedURL,
                            provenance: provenance
                        )
                    )
                }
            }
        }
        if let coordinationError {
            throw ProbeRootAccessError.coordinationFailed(
                coordinationError.localizedDescription
            )
        }
        guard accessorRan, let accessorResult else {
            throw ProbeRootAccessError.coordinationAccessorDidNotRun
        }
        let (coordinatedURL, values, overlapsAppOwnedRoot): (
            URL,
            URLResourceValues,
            Bool?
        )
        do {
            (coordinatedURL, values, overlapsAppOwnedRoot) = try accessorResult.get()
        } catch {
            throw ProbeRootAccessError.resourceObservationFailed(
                error.localizedDescription
            )
        }

        let providerLookup = queryFileProvider
            ? try fileProviderLookup(for: coordinatedURL)
            : .notQueried
        let observation = ProbeRootEligibilityObservation(
            isFileURL: coordinatedURL.isFileURL,
            isDirectory: values.isDirectory,
            isSymbolicLink: values.isSymbolicLink,
            overlapsAppOwnedRoot: overlapsAppOwnedRoot,
            isUbiquitousItem: values.isUbiquitousItem,
            volumeIsLocal: values.volumeIsLocal,
            volumeIsInternal: values.volumeIsInternal,
            fileProviderLookup: providerLookup
        )
        return ProbeObservedRoot(
            coordinatedURL: coordinatedURL,
            displayLabel: boundedLabel(values.localizedName ?? values.name),
            observation: observation,
            decision: ProbeRootEligibility.decide(
                provenance: provenance,
                observation: observation
            )
        )
    }

    private static func fileProviderLookup(for url: URL) throws -> ProbeFileProviderLookup {
        try ProbeResourceLedger.shared.withResource(.eligibilityRequest) {
            let semaphore = DispatchSemaphore(value: 0)
            let result = ProbeFileProviderResultBox()
            NSFileProviderManager.getIdentifierForUserVisibleFile(at: url) {
                itemIdentifier,
                domainIdentifier,
                error in
                let status: ProbeFileProviderLookup
                if itemIdentifier != nil || domainIdentifier != nil {
                    status = .identified
                } else if error != nil {
                    status = .failed
                } else {
                    status = .noIdentifier
                }
                result.complete(status)
                semaphore.signal()
            }
            guard semaphore.wait(timeout: .now() + eligibilityDeadline) == .success else {
                return .timedOut
            }
            return result.value ?? .failed
        }
    }

    private static func coordinatedRustOperation(
        at url: URL,
        prepareInterruption: Bool
    ) throws -> RustResult {
        var coordinationError: NSError?
        var rustResult: RustResult?
        let coordinator = NSFileCoordinator(filePresenter: nil)
        try ProbeResourceLedger.shared.withResource(.coordinator) {
            coordinator.coordinate(
                writingItemAt: url,
                options: .forMerging,
                error: &coordinationError
            ) { coordinatedURL in
                rustResult = prepareInterruption
                    ? RustProbe.prepareInterruptedStorage(at: coordinatedURL)
                    : RustProbe.storage(at: coordinatedURL)
            }
        }
        if let coordinationError {
            throw ProbeRootAccessError.coordinationFailed(
                coordinationError.localizedDescription
            )
        }
        guard let rustResult else {
            throw ProbeRootAccessError.coordinationAccessorDidNotRun
        }
        guard rustResult.ok else {
            throw ProbeRootAccessError.rustOperationFailed(rustResult.json)
        }
        return rustResult
    }

    private static func createMinimalBookmark(for url: URL) throws -> Data {
        let bookmark = try url.bookmarkData(
            options: .minimalBookmark,
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
        guard bookmark.count <= ProbeRootStore.maximumBookmarkBytes else {
            throw ProbeRootAccessError.bookmarkTooLarge(bookmark.count)
        }
        return bookmark
    }

    private static func boundedLabel(_ value: String?) -> String {
        let fallback = "Selected on-device folder"
        guard var label = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !label.isEmpty
        else {
            return fallback
        }
        while label.lengthOfBytes(using: .utf8) > ProbeRootStore.maximumDisplayLabelBytes {
            label.removeLast()
        }
        return label.isEmpty ? fallback : label
    }


    private static func appOwnedOverlap(
        for selectedURL: URL,
        provenance: ProbeRootProvenance
    ) -> Bool? {
        guard provenance == .picker else { return false }
        let appOwnedURL = ProbePaths.documents
        do {
            var appToSelected = FileManager.URLRelationship.other
            try FileManager.default.getRelationship(
                &appToSelected,
                ofDirectoryAt: appOwnedURL,
                toItemAt: selectedURL
            )
            if appToSelected == .same || appToSelected == .contains {
                return true
            }
            var selectedToApp = FileManager.URLRelationship.other
            try FileManager.default.getRelationship(
                &selectedToApp,
                ofDirectoryAt: selectedURL,
                toItemAt: appOwnedURL
            )
            return selectedToApp == .same || selectedToApp == .contains
        } catch {
            return nil
        }
    }
}

private final class ProbeFileProviderResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: ProbeFileProviderLookup?

    var value: ProbeFileProviderLookup? {
        lock.withLock { stored }
    }

    func complete(_ value: ProbeFileProviderLookup) {
        lock.withLock {
            if stored == nil {
                stored = value
            }
        }
    }
}
