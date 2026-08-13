import SwiftUI
import UniformTypeIdentifiers

struct FoundationScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                Section("Engine") {
                    LabeledContent("Status", value: model.engineStatus)
                        .accessibilityIdentifier("engine-status")
                }
                Section("Storage folders") {
                    ForEach(model.roots) { root in
                        VStack(alignment: .leading, spacing: 4) {
                            Label(
                                root.label,
                                systemImage: root.available ? "folder.fill" : "exclamationmark.triangle.fill"
                            )
                            Text(root.detail)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    Button {
                        model.isFolderPickerPresented = true
                    } label: {
                        Label("Choose Download Folder", systemImage: "folder.badge.plus")
                    }
                    .disabled(model.isBusy)
                    .accessibilityIdentifier("choose-folder")
                    Text(model.selectionStatus)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("selected-root-status")
                }
            }
            .navigationTitle("RSTorrent")
            .fileImporter(
                isPresented: $model.isFolderPickerPresented,
                allowedContentTypes: [.folder],
                allowsMultipleSelection: false
            ) { result in
                guard case .success(let urls) = result, let url = urls.first else { return }
                Task { await model.selectFolder(url) }
            }
        }
    }
}
