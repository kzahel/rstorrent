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

## First Chrome Web Store Upload

The initial manifest intentionally has no `key` because the beta extension
does not have a store identity yet.

1. In the Chrome Developer Dashboard, choose **Add new item** and upload the
   generated ZIP as a draft. Publication is not required for this checkpoint.
2. Record the dashboard **Item ID**.
3. On the item's **Package** tab, choose **View public key**. Copy only the
   base64 text between the public-key markers and remove its newlines.
4. Return the Item ID and single-line public key to this repository's
   maintainer workflow. Do not commit a private `.pem` file or store
   credentials.
5. The follow-up change adds that public value as manifest `key`, derives and
   verifies the same 32-character ID for unpacked development, and adds only
   its exact `chrome-extension://<item-id>/` origin to the native-host manifest.

This follows Chrome's official [manifest key procedure](https://developer.chrome.com/docs/extensions/reference/manifest/key).

## Store Review Boundary

The scaffold's single purpose is to detect and open the locally installed
RSTorrent desktop application. Its only extension permission is
`nativeMessaging`. It has no host permissions or content scripts, makes no
network requests, collects no user data or analytics, and sends only bounded
`hello` or `launch` requests to the locally registered RSTorrent host. The
popup explicitly describes this limited preview behavior.
