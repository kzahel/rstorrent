# JSTorrent Beta Extension Scaffold

This is the bounded Manifest V3 bootstrap surface from Tactical
[`166`](../../docs/tactical/166-desktop-native-bootstrap-and-extension-scaffold.md).
It checks for the distinct `com.jstorrent.rstorrent.native` host and can ask the
installed RSTorrent desktop app to open. Tactical
[`167`](../../docs/tactical/167-chromeos-crostini-bundled-web-launcher.md) also
lets the exact local `penguin.linux.test:3030` handoff page wake the worker and
reuse its tab for the backend-served React UI. The extension does not control
torrents, move payload data, run remote code, or replace the current JSTorrent
extension.

The popup is platform-aware. Desktop Chrome shows only the RSTorrent native
bootstrap. ChromeOS shows the exact published JSTorrent Android listing and
the ChromeOS Linux controls, with explicit separate-library guidance. Unknown
platforms show both surfaces as a recovery fallback. The listing link does not
claim that Google Play is enabled or that the Android app is installed.

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

The scaffold detects and opens the locally installed RSTorrent desktop
application and performs the ChromeOS Linux tab handoff. Its only permissions
are `nativeMessaging` and `storage`; session storage remembers one tab ID. It
has no host permissions or content scripts, collects no user data or
analytics, and does not fetch the Crostini backend. External messaging is
manifest-limited to `http://penguin.linux.test/*`, while the worker separately
requires the exact port, path, message keys, protocol version, and sender tab.
