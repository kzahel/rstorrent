import Foundation

enum ProbeRootProvenance: String, Codable, Equatable {
    case appOwned = "app_owned"
    case picker
}

enum ProbeRootEligibilityClass: String, Codable, Equatable {
    case appOwned = "app_owned"
    case selectedOnDevice = "selected_on_device"
    case unsupportedProvider = "unsupported_provider"
    case unclassifiable
}

enum ProbeFileProviderLookup: String, Codable, Equatable {
    case noIdentifier = "no_identifier"
    case identified
    case failed
    case timedOut = "timed_out"
}

enum ProbeRootEligibilityReason: String, Codable, Equatable {
    case acceptedAppOwned = "accepted_app_owned"
    case acceptedSelectedOnDevice = "accepted_selected_on_device"
    case wrongKind = "wrong_kind"
    case symbolicLink = "symbolic_link"
    case ubiquitous = "ubiquitous"
    case nonLocalVolume = "non_local_volume"
    case externalVolume = "external_volume"
    case providerIdentified = "provider_identified"
    case providerLookupFailed = "provider_lookup_failed"
    case providerLookupTimedOut = "provider_lookup_timed_out"
    case missingEvidence = "missing_evidence"
}

struct ProbeRootEligibilityObservation: Codable, Equatable {
    var isDirectory: Bool?
    var isSymbolicLink: Bool?
    var isUbiquitousItem: Bool?
    var volumeIsLocal: Bool?
    var volumeIsInternal: Bool?
    var fileProviderLookup: ProbeFileProviderLookup
}

struct ProbeRootEligibilityDecision: Codable, Equatable {
    var classification: ProbeRootEligibilityClass
    var reason: ProbeRootEligibilityReason

    var isSupported: Bool {
        classification == .appOwned || classification == .selectedOnDevice
    }
}

enum ProbeRootEligibility {
    static func decide(
        provenance: ProbeRootProvenance,
        observation: ProbeRootEligibilityObservation
    ) -> ProbeRootEligibilityDecision {
        guard observation.isDirectory == true else {
            return .init(classification: .unclassifiable, reason: .wrongKind)
        }
        guard observation.isSymbolicLink != true else {
            return .init(classification: .unclassifiable, reason: .symbolicLink)
        }

        if provenance == .appOwned {
            return .init(classification: .appOwned, reason: .acceptedAppOwned)
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

        switch observation.fileProviderLookup {
        case .identified:
            return .init(classification: .unclassifiable, reason: .providerIdentified)
        case .failed:
            return .init(classification: .unclassifiable, reason: .providerLookupFailed)
        case .timedOut:
            return .init(classification: .unclassifiable, reason: .providerLookupTimedOut)
        case .noIdentifier:
            break
        }

        guard
            observation.isUbiquitousItem == false,
            observation.volumeIsLocal == true,
            observation.volumeIsInternal == true
        else {
            return .init(classification: .unclassifiable, reason: .missingEvidence)
        }

        return .init(
            classification: .selectedOnDevice,
            reason: .acceptedSelectedOnDevice
        )
    }
}
