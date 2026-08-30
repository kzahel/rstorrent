# JSTorrent Beta Extension

This Manifest V3 extension retains the bounded desktop and Crostini bootstrap
surfaces from Tactical
[`166`](../../docs/tactical/166-desktop-native-bootstrap-and-extension-scaffold.md).
It checks for the distinct `com.jstorrent.rstorrent.native` host and can ask
the installed RSTorrent desktop app to open. Tactical
[`167`](../../docs/tactical/167-chromeos-crostini-bundled-web-launcher.md) also
lets the exact local `penguin.linux.test:3030` handoff page wake the worker and
reuse its tab for the backend-served React UI.

Tactical [`194`](../../docs/tactical/194-chromeos-android-extension-control.md)
adds an explicit ChromeOS Android connection. The extension packages the
shared React product application, pairs with the RSTorrent Android foreground
service, and uses only the typed application WebSocket plus the authenticated
SAF folder-picker capability. The engine, profile, payload IO, hashing, and SAF
grants stay in Android. The extension does not run remote code or replace the
current production JSTorrent extension.

The popup is platform-aware. Desktop Chrome shows only the RSTorrent native
bootstrap. ChromeOS shows explicit Android-companion and ChromeOS Linux
controls, with separate-library guidance and a link to the current published
JSTorrent Android app. Unknown platforms show both surfaces as a recovery
fallback. The listing link does not claim that Google Play is enabled or that
the RSTorrent Android preview is installed.

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

The extension detects and opens the locally installed RSTorrent desktop
application and performs the ChromeOS Linux tab handoff. Its regular
permissions are `nativeMessaging` and `storage`; the user may grant only the
optional `http://100.115.92.2/*` ARC host permission from the explicit Android
connect action. It has no content scripts, collects no user data or analytics,
and does not fetch executable code from either backend. Its CSP admits only the
five fixed RSTorrent Android HTTP/WebSocket ports. External messaging remains
manifest-limited to `http://penguin.linux.test/*`, while the worker separately
requires the exact Crostini port, path, message keys, protocol version, and
sender tab.
