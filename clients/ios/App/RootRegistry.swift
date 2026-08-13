import Foundation

struct SelectedRootRecord: Codable, Equatable, Identifiable {
    static let schemaVersion = 1

    var schemaVersion: Int
    var id: String
    var generation: UInt64
    var displayLabel: String
    var bookmarkData: Data
    var lastEligibilityClass: RootEligibilityClass
}

struct RootRegistryDocument: Codable, Equatable {
    static let schemaVersion = 1

    var schemaVersion: Int
    var selectedRoots: [SelectedRootRecord]

    static let empty = RootRegistryDocument(
        schemaVersion: schemaVersion,
        selectedRoots: []
    )
}

enum RootRegistryError: Error, LocalizedError, Equatable {
    case unsupportedSchema(Int)
    case tooManyRoots(Int)
    case duplicateRootID
    case invalidRootID
    case invalidGeneration
    case invalidLabel
    case invalidBookmark
    case generationOverflow

    var errorDescription: String? {
        switch self {
        case .unsupportedSchema(let version):
            return "unsupported root registry schema \(version)"
        case .tooManyRoots(let count):
            return "root registry contains \(count) selected roots"
        case .duplicateRootID:
            return "root registry contains a duplicate root ID"
        case .invalidRootID:
            return "root registry contains an invalid root ID"
        case .invalidGeneration:
            return "root registry contains an invalid generation"
        case .invalidLabel:
            return "root registry contains an invalid label"
        case .invalidBookmark:
            return "root registry contains invalid bookmark data"
        case .generationOverflow:
            return "root generation overflowed"
        }
    }
}

actor RootRegistryStore {
    static let maximumRoots = 8
    static let maximumSelectedRoots = maximumRoots - 1
    static let maximumBookmarkBytes = 64 * 1024
    static let maximumLabelBytes = 256

    private let fileURL: URL

    init(fileURL: URL) {
        self.fileURL = fileURL
    }

    func load() throws -> RootRegistryDocument {
        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            return .empty
        }
        let document = try JSONDecoder().decode(
            RootRegistryDocument.self,
            from: Data(contentsOf: fileURL)
        )
        try Self.validate(document)
        return document
    }

    @discardableResult
    func install(bookmarkData: Data, displayLabel: String) throws -> SelectedRootRecord {
        var document = try load()
        let record = SelectedRootRecord(
            schemaVersion: SelectedRootRecord.schemaVersion,
            id: "ios-selected-\(UUID().uuidString.lowercased())",
            generation: 1,
            displayLabel: Self.boundedLabel(displayLabel),
            bookmarkData: bookmarkData,
            lastEligibilityClass: .selectedOnDevice
        )
        document.selectedRoots.append(record)
        try save(document)
        return record
    }

    @discardableResult
    func replace(
        id: String,
        bookmarkData: Data,
        displayLabel: String
    ) throws -> SelectedRootRecord {
        var document = try load()
        guard let index = document.selectedRoots.firstIndex(where: { $0.id == id }) else {
            throw RootRegistryError.invalidRootID
        }
        let previous = document.selectedRoots[index]
        guard previous.generation < UInt64.max else {
            throw RootRegistryError.generationOverflow
        }
        let record = SelectedRootRecord(
            schemaVersion: SelectedRootRecord.schemaVersion,
            id: previous.id,
            generation: previous.generation + 1,
            displayLabel: Self.boundedLabel(displayLabel),
            bookmarkData: bookmarkData,
            lastEligibilityClass: .selectedOnDevice
        )
        document.selectedRoots[index] = record
        try save(document)
        return record
    }

    func remove(id: String) throws {
        var document = try load()
        document.selectedRoots.removeAll { $0.id == id }
        try save(document)
    }

    private func save(_ document: RootRegistryDocument) throws {
        try Self.validate(document)
        try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        let data = try JSONEncoder().encode(document)
        try data.write(to: fileURL, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    static func validate(_ document: RootRegistryDocument) throws {
        guard document.schemaVersion == RootRegistryDocument.schemaVersion else {
            throw RootRegistryError.unsupportedSchema(document.schemaVersion)
        }
        guard document.selectedRoots.count <= maximumSelectedRoots else {
            throw RootRegistryError.tooManyRoots(document.selectedRoots.count)
        }
        var ids = Set<String>()
        for root in document.selectedRoots {
            guard root.schemaVersion == SelectedRootRecord.schemaVersion else {
                throw RootRegistryError.unsupportedSchema(root.schemaVersion)
            }
            guard
                !root.id.isEmpty,
                root.id.lengthOfBytes(using: .utf8) <= 128,
                !root.id.utf8.contains(0)
            else {
                throw RootRegistryError.invalidRootID
            }
            guard ids.insert(root.id).inserted else {
                throw RootRegistryError.duplicateRootID
            }
            guard root.generation > 0 else {
                throw RootRegistryError.invalidGeneration
            }
            guard
                !root.displayLabel.isEmpty,
                root.displayLabel.lengthOfBytes(using: .utf8) <= maximumLabelBytes
            else {
                throw RootRegistryError.invalidLabel
            }
            guard
                !root.bookmarkData.isEmpty,
                root.bookmarkData.count <= maximumBookmarkBytes,
                root.lastEligibilityClass == .selectedOnDevice
            else {
                throw RootRegistryError.invalidBookmark
            }
        }
    }

    static func boundedLabel(_ raw: String) -> String {
        var label = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if label.isEmpty { label = "Selected on-device folder" }
        while label.lengthOfBytes(using: .utf8) > maximumLabelBytes {
            label.removeLast()
        }
        return label
    }
}
