import SwiftUI
import UIKit

struct AddTorrentSheet: View {
    @Binding var magnetInput: String
    let onAdd: () -> Void
    let onBrowse: () -> Void

    @Environment(\.dismiss) private var dismiss
    @FocusState private var isMagnetFieldFocused: Bool

    private var canAddMagnet: Bool {
        !magnetInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var clipboardText: String? {
        let candidate = UIPasteboard.general.string?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let candidate, !candidate.isEmpty else { return nil }
        return candidate
    }

    var body: some View {
        NavigationStack {
            Form {
                Section(String(localized: "dialog_add_torrent_magnet_label")) {
                    TextField(
                        String(localized: "dialog_add_torrent_magnet_hint"),
                        text: $magnetInput,
                        axis: .vertical
                    )
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($isMagnetFieldFocused)
                    .submitLabel(.done)
                    .onSubmit(submitMagnet)

                    Button(String(localized: "dialog_add_torrent_paste_button")) {
                        magnetInput = clipboardText ?? magnetInput
                    }
                    .disabled(clipboardText == nil)
                }

                Section {
                    Button(String(localized: "dialog_add_torrent_add_button")) {
                        submitMagnet()
                    }
                    .disabled(!canAddMagnet)

                    Button(String(localized: "dialog_add_torrent_browse_button")) {
                        dismiss()
                        onBrowse()
                    }
                }
            }
            .navigationTitle(String(localized: "dialog_add_torrent_title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "dialog_add_torrent_cancel_button")) { dismiss() }
                }
                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()
                    Button(String(localized: "dialog_add_torrent_add_button")) {
                        submitMagnet()
                    }
                    .disabled(!canAddMagnet)
                }
            }
        }
    }

    private func submitMagnet() {
        guard canAddMagnet else { return }
        isMagnetFieldFocused = false
        onAdd()
        dismiss()
    }
}
