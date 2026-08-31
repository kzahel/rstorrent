import Foundation

enum EnginePresentationStatus: Equatable {
    case starting
    case ready
    case stopped
    case unavailable(String)

    var text: String {
        switch self {
        case .starting:
            return String(localized: "ios_engine_starting")
        case .ready:
            return String(localized: "ios_engine_ready")
        case .stopped:
            return String(localized: "ios_engine_stopped")
        case .unavailable(let detail):
            return String(
                format: String(localized: "ios_engine_unavailable"),
                locale: .current,
                detail
            )
        }
    }
}

enum SelectionPresentationStatus: Equatable {
    case noneSelected
    case externalFoldersReady
    case folderNeedsRepair
    case checkingSelection
    case checkingReplacement
    case folderReady(String)
    case folderRepaired(String)
    case appFolderNeedsNoRepair
    case appFolderIsDefault
    case incomingMagnetInvalid
    case incomingTypeUnsupported
    case incomingAlreadyHandled
    case incomingOccupied
    case incomingAccepted
    case error(String)

    var text: String {
        switch self {
        case .noneSelected:
            return String(localized: "ios_selection_none")
        case .externalFoldersReady:
            return String(localized: "ios_selection_external_ready")
        case .folderNeedsRepair:
            return String(localized: "ios_selection_needs_repair")
        case .checkingSelection:
            return String(localized: "ios_selection_checking")
        case .checkingReplacement:
            return String(localized: "ios_selection_checking_replacement")
        case .folderReady(let label):
            return String(
                format: String(localized: "ios_selection_folder_ready"),
                locale: .current,
                label
            )
        case .folderRepaired(let label):
            return String(
                format: String(localized: "ios_selection_folder_repaired"),
                locale: .current,
                label
            )
        case .appFolderNeedsNoRepair:
            return String(localized: "ios_selection_app_folder_no_repair")
        case .appFolderIsDefault:
            return String(localized: "ios_selection_app_folder_default")
        case .incomingMagnetInvalid:
            return String(localized: "ios_input_magnet_invalid")
        case .incomingTypeUnsupported:
            return String(localized: "ios_input_type_unsupported")
        case .incomingAlreadyHandled:
            return String(localized: "ios_input_already_handled")
        case .incomingOccupied:
            return String(localized: "ios_input_occupied")
        case .incomingAccepted:
            return String(localized: "ios_input_accepted")
        case .error(let detail):
            return detail
        }
    }

    var isError: Bool {
        switch self {
        case .folderNeedsRepair, .incomingMagnetInvalid, .incomingTypeUnsupported,
             .incomingAlreadyHandled, .incomingOccupied, .error:
            return true
        default:
            return false
        }
    }
}

enum BackgroundPresentationStatus: Equatable {
    case foreground
    case inactive
    case finiteUIKitTime
    case continuedRequested
    case notificationsEnabled
    case notificationsUnauthorized
    case notificationsDisabled
    case continuedActive
    case continuedUnavailable
    case workComplete
    case timeExpired
    case savingAfterExpiration

    var text: String {
        switch self {
        case .foreground:
            return String(localized: "ios_background_foreground")
        case .inactive:
            return String(localized: "ios_background_inactive")
        case .finiteUIKitTime:
            return String(localized: "ios_background_finite_time")
        case .continuedRequested:
            return String(localized: "ios_background_continued_requested")
        case .notificationsEnabled:
            return String(localized: "ios_background_notifications_enabled")
        case .notificationsUnauthorized:
            return String(localized: "ios_background_notifications_unauthorized")
        case .notificationsDisabled:
            return String(localized: "ios_background_notifications_disabled")
        case .continuedActive:
            return String(localized: "ios_background_continued_active")
        case .continuedUnavailable:
            return String(localized: "ios_background_continued_unavailable")
        case .workComplete:
            return String(localized: "ios_background_work_complete")
        case .timeExpired:
            return String(localized: "ios_background_time_expired")
        case .savingAfterExpiration:
            return String(localized: "ios_background_saving")
        }
    }
}

enum RootDisplayName: Equatable {
    case appDocuments
    case supplied(String)

    var text: String {
        switch self {
        case .appDocuments:
            return String(localized: "ios_app_documents")
        case .supplied(let value):
            return value
        }
    }
}

enum RootDisplayDetail: Equatable {
    case onMyDevice
    case qualified
    case probeFailed
    case error(String)

    var text: String {
        switch self {
        case .onMyDevice:
            return String(localized: "ios_on_my_device")
        case .qualified:
            return String(localized: "ios_root_qualified")
        case .probeFailed:
            return String(localized: "ios_root_probe_failed")
        case .error(let detail):
            return detail
        }
    }
}
