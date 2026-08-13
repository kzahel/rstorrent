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

Search, iCloud and identified File Provider roots, legacy JSTorrent migration,
TestFlight, and App Store publication are outside the current campaign.
