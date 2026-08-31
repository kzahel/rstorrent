import SwiftUI
import UniformTypeIdentifiers

struct FoundationScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                Section(String(localized: "ios_foundation_engine")) {
                    LabeledContent(
                        String(localized: "ios_runtime_status_label"),
                        value: model.engineStatus.text
                    )
                        .accessibilityIdentifier("engine-status")
                }
                Section(String(localized: "ios_foundation_storage_folders")) {
                    ForEach(model.roots) { root in
                        VStack(alignment: .leading, spacing: 4) {
                            Label(
                                root.label,
                                systemImage: root.available ? "folder.fill" : "exclamationmark.triangle.fill"
                            )
                            Text(root.detailText)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    Button {
                        model.isFolderPickerPresented = true
                    } label: {
                        Label(
                            String(localized: "settings_download_folder_choose_button"),
                            systemImage: "folder.badge.plus"
                        )
                    }
                    .disabled(model.isBusy)
                    .accessibilityIdentifier("choose-folder")
                    Text(model.selectionStatus.text)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("selected-root-status")
                }
            }
            .navigationTitle(String(localized: "app_name"))
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
