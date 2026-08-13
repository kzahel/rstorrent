# RSTorrent for iOS

This directory owns the first-party SwiftUI client. The Rust application
service runs in-process; Swift owns Apple lifecycle, security-scoped folder
bookmarks, file coordination, and presentation.

Generate bindings and the Xcode project before building:

```bash
source ~/.profile
clients/ios/scripts/generate-project.sh
```

The project deliberately contains no development team, provisioning profile,
device identifier, or Apple account. Pass signing settings to `xcodebuild` or
select a local team in Xcode. The development bundle identifier is
`org.rstorrent.ios.dev`, distinct from JSTorrent.

Create a reproducible unsigned archive without local signing state:

```bash
clients/ios/scripts/archive.sh --unsigned /absolute/path/RSTorrent.xcarchive
```

For a locally signed development archive, supply the team only through the
process environment; never add it to the project:

```bash
RSTORRENT_IOS_DEVELOPMENT_TEAM=... \
  clients/ios/scripts/archive.sh --development \
  /absolute/path/RSTorrent-development.xcarchive
```

The app accepts both `magnet:` URLs and `.torrent` document handoffs. iOS owns
the finite background opportunity: active work may use a UIKit background
assertion and, on iOS 26 or newer, request continued processing. Expiration
stops the in-process service cleanly; durable state resumes on the next launch.
Completion notifications remain an explicit Settings opt-in. No path promises
indefinite background downloading or seeding.

Search, iCloud and identified File Provider roots, legacy JSTorrent migration,
TestFlight, and App Store publication are outside the current campaign.
