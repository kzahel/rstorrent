import SwiftUI
import UniformTypeIdentifiers

struct ProbeView: View {
    @ObservedObject var model: ProbeModel

    var body: some View {
        NavigationStack {
            List {
                Section("Storage") {
                    status("App-owned", value: model.appOwned, id: "app-owned-status")
                    Button("Run App-Owned Probe") { model.runAppOwned() }
                        .accessibilityIdentifier("run-app-owned")
                    status("Root registry", value: model.rootState, id: "root-state-status")
                    status("Root resources", value: model.resources, id: "resources-status")
                    status("Selected root", value: model.selected, id: "selected-status")
                    status("Eligibility", value: model.eligibility, id: "eligibility-status")
                    Button("Classify System-Picked Folder") { model.chooseFolder() }
                        .accessibilityIdentifier("choose-folder")
                    if ProbeRootPolicy.selectedRootRegistrationEnabled {
                        Button("Retry Selected Root") { model.retrySelected() }
                            .accessibilityIdentifier("retry-selected")
                    }
                    Button("Open App Settings") { model.openAppSettings() }
                        .accessibilityIdentifier("open-app-settings")
                    Text("Only app-owned Documents is enabled. Picker results are classified without bookmarks or writes.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                Section("Networking") {
                    status("Direct Rust TCP/UDP", value: model.network, id: "network-status")
                }
                Section("Lifecycle") {
                    status("Evidence", value: model.lifecycle, id: "lifecycle-status")
                    Button("Start Continued Task") { model.submitContinuedProbe() }
                        .accessibilityIdentifier("start-continued")
                    Button("Arm Ordinary Expiration") { model.armOrdinaryExpirationProbe() }
                        .accessibilityIdentifier("arm-expiration")
                    Button("Prepare App-Owned Force-Close") {
                        model.prepareAppOwnedInterruption()
                    }
                    .accessibilityIdentifier("prepare-app-interruption")
                    if ProbeRootPolicy.selectedRootRegistrationEnabled {
                        Button("Prepare Selected Force-Close") {
                            model.prepareSelectedInterruption()
                        }
                        .accessibilityIdentifier("prepare-selected-interruption")
                    }
                }
            }
            .navigationTitle("RSTorrent Probe")
        }
        .sheet(isPresented: $model.presentsPicker) {
            FolderPicker(
                selected: model.selectedFolder,
                cancelled: model.cancelPicker
            )
        }
    }

    private func status(_ title: String, value: String, id: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.headline)
            Text(value)
                .font(.caption.monospaced())
                .textSelection(.enabled)
                .accessibilityIdentifier(id)
        }
    }
}

private struct FolderPicker: UIViewControllerRepresentable {
    let selected: (URL) -> Void
    let cancelled: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(selected: selected, cancelled: cancelled)
    }

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.folder])
        picker.allowsMultipleSelection = false
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}

    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let selected: (URL) -> Void
        let cancelled: () -> Void

        init(selected: @escaping (URL) -> Void, cancelled: @escaping () -> Void) {
            self.selected = selected
            self.cancelled = cancelled
        }

        func documentPicker(
            _ controller: UIDocumentPickerViewController,
            didPickDocumentsAt urls: [URL]
        ) {
            guard urls.count == 1, let url = urls.first else { return cancelled() }
            selected(url)
        }

        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
            cancelled()
        }
    }
}
