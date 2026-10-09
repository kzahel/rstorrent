# JSTorrent Update Scenarios

First-release checklist, agreed 2026-10-09. Make a good-faith effort to give
users an understandable next step when updates arrive separately. This is a
small rollout and recovery checklist, not a promise of legacy feature parity.

## Release order

- [ ] Freeze and test the exact desktop, Android and extension candidates.
- [ ] Submit store candidates with Chrome Web Store deferred publishing and
  Play managed publishing; verify those controls before submission.
- [ ] Wait for approvals, then make desktop and Android updates available.
- [ ] Verify both backend updates are obtainable, then publish the extension.
- [ ] Check actual existing-install updates and the mixed pairs below. Users
  can remain on different versions after publication; no simultaneous arrival
  is assumed.
- [ ] Name a rollout owner and recovery action. Pause further rollout on data
  loss, competing writers or broken ordinary native startup. An update halt
  does not undo updates already installed.

[Chrome deferred publishing](https://developer.chrome.com/docs/webstore/publish/)
allows up to 30 days after approval. [Play managed publishing](https://support.google.com/googleplay/android-developer/answer/9859654)
holds most approved production changes until manual publication. Internal test
delivery and exceptions must be checked separately. Nothing here authorizes
submission or publication by itself.

## User scenarios

Check a row after a representative run demonstrates its message and recovery.
Source inspection or an injected test alone does not check a store-update row.
Record the tested versions and link a screenshot or receipt in the ignored
finish-line report; avoid adding private device details to this document.

| Check | Scenario | User's recovery story | Current evidence or remaining gap |
| --- | --- | --- | --- |
| [ ] | Old extension + old desktop/Android | Existing behavior continues until either component updates. | Retain one real baseline; no changes to the old release are required. |
| [ ] | New extension + old desktop | Name **JSTorrent Desktop** as needing an update; offer the desktop download/update path, then Retry. Native use remains available meanwhile. | Current desktop error says both components are incompatible, without a download link. Improve guidance; do not claim the protocol identifies the outdated side in every case. |
| [ ] | New extension + old Android | Say **Update the JSTorrent Android app in Google Play**; link its listing, then Retry. Android can be used directly meanwhile. | Specific classification, Play link and bounded no-legacy-pairing tests exist; real store-update check remains. |
| [ ] | Old extension + new desktop/Android | Open the native app directly. Update the **JSTorrent extension**, then reconnect and approve fresh pairing if requested. | Android already explains this. Very old extension errors cannot be rewritten by the new app; cover them in native/help guidance. No intermediate legacy release is required. |
| [ ] | New extension + new backend | Open/connect to the same migrated library; approve pairing when requested. | Controlled evidence exists; verify exact released identities and an existing browser profile. |
| [ ] | Required update is not offered | Explain that delivery may still be rolling out. Use the installed native app and retry later. If the OS is unsupported, explain the requirement and alternatives. | Chromebook help covers no-update/unsupported cases; check the actual store outcome. Never promise an update that the store is not offering. |
| [ ] | App absent, stopped, or service unreachable | Offer the appropriate install link or Open app action. Check browser control and permission; then Retry. | Do not infer “outdated” or “not installed” from a timeout. Existing Android setup/help distinguishes stages; verify desktop missing-host recovery. |
| [ ] | Pairing denied, expired, or changed after update | Open the same app, approve the named request and Retry. Keep the library; do not recommend clearing data or reinstalling as the normal fix. | Existing Android pairing and desktop identity-change paths; final installed check remains. |
| [ ] | Download folder unavailable after update | Select/repair the original folder in native storage settings and approve access. Retain torrents and existing files. | Existing Android folder-help path; finish physical reboot/provider checks. |
| [ ] | Linux desktop incompatible, or user switches Android/Linux | Linux is best effort: explain minimum requirements and CLI/headless alternatives. Android and Linux have separate libraries; switching does not migrate torrents automatically. | Final AppImage requirements and usable headless download/instructions still need verification. DEB/RPM are outside this release. |

## Message and link checks

- [ ] Each actionable message names **Desktop app**, **Android app** or
  **Chrome extension** when known, and includes a relevant link or help action.
  If diagnosis is uncertain, say what to check rather than guessing.
- [ ] Links open the right existing product, and their destination actually
  offers the required release. Opening a listing is not proof of an update.
- [ ] Explain what the user can use meanwhile, and provide manual Retry after
  the requested action. Check one narrow-window screenshot and keyboard access.
- [ ] Unsupported Android versions, managed devices and older Linux systems
  receive a usable explanation without repeated installation loops.
- [ ] Update/recovery instructions preserve existing data and keep one backend
  owner. Stop failed migration visibly; no old-protocol writer fallback.

Known destinations:

- [Android app in Google Play](https://play.google.com/store/apps/details?id=com.jstorrent.app)
- [JSTorrent extension in Chrome Web Store](https://chromewebstore.google.com/detail/dbokmlpefliilbjldladbimlcfgbolhk)
- [JSTorrent website](https://jstorrent.com/) — verify its final desktop download
  destination before wiring new recovery links; the website cutover is pending.
- Bundled Chromebook help: `clients/extension/crostini/setup.html`.

Source review: Android companion client/main and `companion.html`; desktop
companion client/main; English localization catalog; extension popup and
Chromebook setup; Android's `legacy_chromeos_migration_guidance` string.
On 2026-10-09, all 71 existing cases in the eight Android/desktop companion
client, upgrade, recovery, platform, handshake and main suites pass. These
prove bounded connection behavior, not real store delivery or final pixels.
See [full release checklist](jstorrent-cutover-checklist.md) for installed,
store, retained-data and publication gates. iOS and a webview-free tray build
are outside this release.
