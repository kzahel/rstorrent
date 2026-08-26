# JSTorrent Beta Extension Scaffold

This is the bounded Manifest V3 bootstrap surface from Tactical
[`166`](../../docs/tactical/166-desktop-native-bootstrap-and-extension-scaffold.md).
It checks for the distinct `com.jstorrent.rstorrent.native` host and can ask the
installed RSTorrent desktop app to open. It does not control torrents, access
websites, move payload data, run remote code, or replace the current JSTorrent
extension.

## Validate And Package

```bash
npm test --prefix clients/extension
npm run package --prefix clients/extension
```

The package command validates the reviewed file allowlist and writes
`target/extension/jstorrent-beta-<version>.zip`. The ZIP deliberately excludes
this README, store notes, scripts, dependencies, build output, and secrets.

## Chrome Web Store Identity

The draft store item is `gcgoepclopkgijmclmlheafaglmbjlcc`. Its public key is
pinned in the manifest so store and unpacked builds retain that identity. The
validator independently derives the extension ID from the public key and
rejects any mismatch.

Upload each generated ZIP to that same dashboard item rather than creating a
new one. Loading this directory unpacked must also display the pinned ID.
Publication is not required for the bootstrap checkpoint. Do not commit a
private `.pem` file or store credentials; the manifest contains only the
dashboard's public key.

This follows Chrome's official [manifest key procedure](https://developer.chrome.com/docs/extensions/reference/manifest/key).

## Store Review Boundary

The scaffold's single purpose is to detect and open the locally installed
RSTorrent desktop application. Its only extension permission is
`nativeMessaging`. It has no host permissions or content scripts, makes no
network requests, collects no user data or analytics, and sends only bounded
`hello` or `launch` requests to the locally registered RSTorrent host. The
popup explicitly describes this limited preview behavior.
