# RSTorrent iOS Storage Probe

This is the bounded Tactical 123 feasibility harness, not an iOS product. It
links the real `rstorrent-engine` file pool into a development-signed iOS app.
Payload bytes remain in Rust while Swift owns root eligibility, persistence,
security-scope balancing, per-operation file coordination, and lifecycle.

Only the app-owned, user-visible Documents root is enabled. The system picker
is retained as a classification control, but a selection cannot create a
bookmark, root record, or Rust payload operation. Picker-backed local storage
remains disabled until a physical run distinguishes a separate **On My
iPhone** directory from an iCloud negative control using public API evidence.

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

Run the standalone eligibility and root-store tests:

```bash
xcodebuild -project RSTorrentIOSStorageProbe.xcodeproj \
  -scheme RSTorrentIOSStorageProbe \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=26.2' \
  -only-testing:RSTorrentIOSStorageProbeTests test
```

The app-owned probe runs on every launch. These environment flags support
bounded lifecycle evidence without adding product behavior:

- `RSTORRENT_PROBE_HOST=loopback` runs direct Rust TCP and UDP loopback;
- `RSTORRENT_PROBE_PREPARE_APP_INTERRUPTION=1` leaves the exact owned partial
  workspace and a generation-fenced recovery fact for a force-close test;
- `RSTORRENT_PROBE_ARM_EXPIRATION=1` arms one ordinary UIKit background task;
  and
- `RSTORRENT_PROBE_SUBMIT_CONTINUED=1` submits one finite iOS 26 continued
  processing task.

Physical evidence uses an owned paired device and a controlled same-LAN TCP/
UDP echo endpoint. Stable device identifiers, bookmarks, derived data, signing
state, and probe output are not repository artifacts. The app deletes only its
exact `.rstorrent-ios-storage-probe` workspace under the directory being
tested. Uninstall the probe after the matrix to remove its app container and
probe-local root registry.
