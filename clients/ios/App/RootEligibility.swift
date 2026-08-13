import Foundation

enum RootProviderLookup: String, Codable, Equatable {
    case notQueried = "not_queried"
    case noIdentifier = "no_identifier"
    case identified
    case failed
    case timedOut = "timed_out"
}

enum RootEligibilityClass: String, Codable, Equatable {
    case selectedOnDevice = "selected_on_device"
    case unsupportedProvider = "unsupported_provider"
    case unclassifiable
}

enum RootEligibilityReason: String, Codable, Equatable {
    case acceptedSelectedOnDevice = "accepted_selected_on_device"
    case wrongKind = "wrong_kind"
    case wrongScheme = "wrong_scheme"
    case symbolicLink = "symbolic_link"
    case overlapsRegisteredRoot = "overlaps_registered_root"
    case ubiquitous = "ubiquitous"
    case nonLocalVolume = "non_local_volume"
    case externalVolume = "external_volume"
    case providerIdentified = "provider_identified"
    case providerLookupTimedOut = "provider_lookup_timed_out"
    case missingEvidence = "missing_evidence"
}

struct RootEligibilityObservation: Codable, Equatable {
    var isFileURL: Bool?
    var isDirectory: Bool?
    var isSymbolicLink: Bool?
    var overlapsRegisteredRoot: Bool?
    var isUbiquitousItem: Bool?
    var volumeIsLocal: Bool?
    var volumeIsInternal: Bool?
    var fileProviderLookup: RootProviderLookup
}

struct RootEligibilityDecision: Codable, Equatable {
    var classification: RootEligibilityClass
    var reason: RootEligibilityReason

    var isSupported: Bool { classification == .selectedOnDevice }
}

enum RootEligibility {
    static func decide(_ observation: RootEligibilityObservation) -> RootEligibilityDecision {
        guard observation.isFileURL == true else {
            return .init(classification: .unclassifiable, reason: .wrongScheme)
        }
        guard observation.isDirectory == true else {
            return .init(classification: .unclassifiable, reason: .wrongKind)
        }
        guard observation.isSymbolicLink == false else {
            return .init(classification: .unclassifiable, reason: .symbolicLink)
        }
        guard observation.overlapsRegisteredRoot == false else {
            return .init(classification: .unclassifiable, reason: .overlapsRegisteredRoot)
        }
        if observation.isUbiquitousItem == true {
            return .init(classification: .unsupportedProvider, reason: .ubiquitous)
        }
        if observation.volumeIsLocal == false {
            return .init(classification: .unsupportedProvider, reason: .nonLocalVolume)
        }
        if observation.volumeIsInternal == false {
            return .init(classification: .unsupportedProvider, reason: .externalVolume)
        }
        if observation.fileProviderLookup == .identified {
            return .init(classification: .unsupportedProvider, reason: .providerIdentified)
        }
        if observation.fileProviderLookup == .timedOut {
            return .init(classification: .unclassifiable, reason: .providerLookupTimedOut)
        }
        guard
            observation.isUbiquitousItem == false,
            observation.volumeIsLocal == true,
            observation.volumeIsInternal == true,
            observation.overlapsRegisteredRoot == false,
            observation.fileProviderLookup == .noIdentifier
                || observation.fileProviderLookup == .failed
        else {
            return .init(classification: .unclassifiable, reason: .missingEvidence)
        }
        return .init(
            classification: .selectedOnDevice,
            reason: .acceptedSelectedOnDevice
        )
    }
}
