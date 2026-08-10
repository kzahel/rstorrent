import Foundation

enum ProbeRootKind: String, Codable, Equatable {
    case appOwned = "app_owned"
    case selectedOnDevice = "selected_on_device"
}

enum ProbePendingPhase: String, Codable, Equatable {
    case preparedPartialWorkspace = "prepared_partial_workspace"
}

struct ProbeRootRecord: Codable, Equatable {
    var schemaVersion: Int
    var stableRootID: String
    var kind: ProbeRootKind
    var generation: UInt64
    var displayLabel: String
    var bookmarkData: Data?
    var lastEligibilityClass: ProbeRootEligibilityClass
}

struct ProbePendingOperation: Codable, Equatable {
    var rootID: String
    var rootGeneration: UInt64
    var phase: ProbePendingPhase
}

struct ProbeRootRegistry: Codable, Equatable {
    var schemaVersion: Int
    var roots: [ProbeRootRecord]
    var pendingOperation: ProbePendingOperation?

    static let empty = ProbeRootRegistry(
        schemaVersion: ProbeRootStore.schemaVersion,
        roots: [],
        pendingOperation: nil
    )
}

enum ProbeRootStoreError: Error, Equatable, LocalizedError {
    case unsupportedSchema(Int)
    case tooManyRoots(Int)
    case duplicateRootID
    case duplicateRootKind
    case invalidRootID
    case invalidGeneration
    case invalidDisplayLabel
    case invalidBookmark
    case invalidEligibility
    case invalidPendingOperation
    case generationOverflow
    case encodingFailed

    var errorDescription: String? {
        switch self {
        case .unsupportedSchema(let version):
            return "unsupported root registry schema \(version)"
        case .tooManyRoots(let count):
            return "root registry contains \(count) roots"
        case .duplicateRootID:
            return "root registry contains a duplicate root ID"
        case .duplicateRootKind:
            return "root registry contains a duplicate root kind"
        case .invalidRootID:
            return "root registry contains an invalid root ID"
        case .invalidGeneration:
            return "root registry contains an invalid generation"
        case .invalidDisplayLabel:
            return "root registry contains an invalid display label"
        case .invalidBookmark:
            return "root registry contains invalid bookmark data"
        case .invalidEligibility:
            return "root registry contains an invalid eligibility class"
        case .invalidPendingOperation:
            return "root registry contains an invalid pending operation"
        case .generationOverflow:
            return "root generation overflowed"
        case .encodingFailed:
            return "root registry could not be encoded"
        }
    }
}

final class ProbeRootStore {
    static let schemaVersion = 1
    static let maximumRoots = 2
    static let maximumBookmarkBytes = 64 * 1024
    static let maximumDisplayLabelBytes = 256

    private let defaults: UserDefaults
    private let key: String

    init(defaults: UserDefaults = .standard, key: String = "probe.root-registry.v1") {
        self.defaults = defaults
        self.key = key
    }

    func load() throws -> ProbeRootRegistry {
        guard let data = defaults.data(forKey: key) else {
            return .empty
        }
        let registry = try JSONDecoder().decode(ProbeRootRegistry.self, from: data)
        try Self.validate(registry)
        return registry
    }

    @discardableResult
    func ensureAppOwned(displayLabel: String) throws -> ProbeRootRecord {
        var registry = try load()
        if let existing = registry.roots.first(where: { $0.kind == .appOwned }) {
            return existing
        }
        let record = ProbeRootRecord(
            schemaVersion: Self.schemaVersion,
            stableRootID: UUID().uuidString.lowercased(),
            kind: .appOwned,
            generation: 1,
            displayLabel: displayLabel,
            bookmarkData: nil,
            lastEligibilityClass: .appOwned
        )
        registry.roots.append(record)
        try save(registry)
        return record
    }

    @discardableResult
    func installSelected(bookmarkData: Data, displayLabel: String) throws -> ProbeRootRecord {
        var registry = try load()
        let record: ProbeRootRecord
        if let index = registry.roots.firstIndex(where: { $0.kind == .selectedOnDevice }) {
            let current = registry.roots[index]
            guard current.generation < UInt64.max else {
                throw ProbeRootStoreError.generationOverflow
            }
            record = ProbeRootRecord(
                schemaVersion: Self.schemaVersion,
                stableRootID: current.stableRootID,
                kind: .selectedOnDevice,
                generation: current.generation + 1,
                displayLabel: displayLabel,
                bookmarkData: bookmarkData,
                lastEligibilityClass: .selectedOnDevice
            )
            registry.roots[index] = record
        } else {
            record = ProbeRootRecord(
                schemaVersion: Self.schemaVersion,
                stableRootID: UUID().uuidString.lowercased(),
                kind: .selectedOnDevice,
                generation: 1,
                displayLabel: displayLabel,
                bookmarkData: bookmarkData,
                lastEligibilityClass: .selectedOnDevice
            )
            registry.roots.append(record)
        }
        try save(registry)
        return record
    }

    func beginPendingOperation(for root: ProbeRootRecord) throws {
        var registry = try load()
        guard registry.roots.contains(where: {
            $0.stableRootID == root.stableRootID && $0.generation == root.generation
        }) else {
            throw ProbeRootStoreError.invalidPendingOperation
        }
        registry.pendingOperation = ProbePendingOperation(
            rootID: root.stableRootID,
            rootGeneration: root.generation,
            phase: .preparedPartialWorkspace
        )
        try save(registry)
    }

    @discardableResult
    func completePendingOperation(rootID: String, generation: UInt64) throws -> Bool {
        var registry = try load()
        guard
            let pending = registry.pendingOperation,
            pending.rootID == rootID,
            pending.rootGeneration == generation
        else {
            return false
        }
        registry.pendingOperation = nil
        try save(registry)
        return true
    }

    func reset() {
        defaults.removeObject(forKey: key)
    }

    private func save(_ registry: ProbeRootRegistry) throws {
        try Self.validate(registry)
        guard let data = try? JSONEncoder().encode(registry) else {
            throw ProbeRootStoreError.encodingFailed
        }
        defaults.set(data, forKey: key)
        defaults.synchronize()
    }

    static func validate(_ registry: ProbeRootRegistry) throws {
        guard registry.schemaVersion == schemaVersion else {
            throw ProbeRootStoreError.unsupportedSchema(registry.schemaVersion)
        }
        guard registry.roots.count <= maximumRoots else {
            throw ProbeRootStoreError.tooManyRoots(registry.roots.count)
        }

        var rootIDs = Set<String>()
        var kinds = Set<ProbeRootKind>()
        for root in registry.roots {
            guard root.schemaVersion == schemaVersion else {
                throw ProbeRootStoreError.unsupportedSchema(root.schemaVersion)
            }
            guard UUID(uuidString: root.stableRootID) != nil else {
                throw ProbeRootStoreError.invalidRootID
            }
            guard rootIDs.insert(root.stableRootID).inserted else {
                throw ProbeRootStoreError.duplicateRootID
            }
            guard kinds.insert(root.kind).inserted else {
                throw ProbeRootStoreError.duplicateRootKind
            }
            guard root.generation > 0 else {
                throw ProbeRootStoreError.invalidGeneration
            }
            let labelBytes = root.displayLabel.lengthOfBytes(using: .utf8)
            guard labelBytes > 0, labelBytes <= maximumDisplayLabelBytes else {
                throw ProbeRootStoreError.invalidDisplayLabel
            }
            switch root.kind {
            case .appOwned:
                guard
                    root.bookmarkData == nil,
                    root.lastEligibilityClass == .appOwned
                else {
                    throw ProbeRootStoreError.invalidEligibility
                }
            case .selectedOnDevice:
                guard
                    let bookmark = root.bookmarkData,
                    !bookmark.isEmpty,
                    bookmark.count <= maximumBookmarkBytes
                else {
                    throw ProbeRootStoreError.invalidBookmark
                }
                guard root.lastEligibilityClass == .selectedOnDevice else {
                    throw ProbeRootStoreError.invalidEligibility
                }
            }
        }

        if let pending = registry.pendingOperation {
            guard registry.roots.contains(where: {
                $0.stableRootID == pending.rootID
                    && $0.generation == pending.rootGeneration
            }) else {
                throw ProbeRootStoreError.invalidPendingOperation
            }
        }
    }
}
