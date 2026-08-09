# RSTorrent iOS Storage Probe

This is the bounded Tactical 116 feasibility harness, not an iOS product.
It links the real `rstorrent-engine` file pool into a development-signed iOS
app and keeps payload bytes in Rust while Swift owns document picking,
bookmark restoration, security-scope balancing, and file coordination.

Generate and build the Xcode project:

```bash
source ~/.profile
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cd experiments/ios-storage-probe
xcodegen generate
xcodebuild -project RSTorrentIOSStorageProbe.xcodeproj \
  -scheme RSTorrentIOSStorageProbe \
  -destination 'generic/platform=iOS Simulator' build
```

Physical evidence uses an owned paired device and a controlled same-LAN TCP/
UDP echo endpoint. Stable device identifiers, bookmarks, derived data, signing
state, and probe output are not repository artifacts. The app deletes only its
exact `.rstorrent-ios-storage-probe` workspace under the directory being
tested.
