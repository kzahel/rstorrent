# First Desktop Latest Release: 0.2.301

The maintainer authorized the first signed Latest release after the channel
source and Remy product registry were deployed. Main CI
[`36329145890`](https://github.com/kzahel/rstorrent/actions/runs/36329145890)
passed at exact source commit `30523da4b5bef2b3609d8491585a651733557c1f`.
Nightly Desktop run
[`36331465234`](https://github.com/kzahel/rstorrent/actions/runs/36331465234)
selected that commit and numeric package version `0.2.301`. Its source gate,
both signed/notarized macOS jobs, both signed Linux package jobs, Windows
Authenticode job, and sole validation/publication finalizer all passed.

The finalizer published the public prerelease
[`desktop-latest-v0.2.301`](https://github.com/kzahel/rstorrent/releases/tag/desktop-latest-v0.2.301)
with target commit matching the selected source. It is not a GitHub Latest
release and does not replace Stable `desktop-v0.1.4`. The release contains
`latest.json`, twelve package/updater assets, and `SHA256SUMS`. Independent
public reads found all 15 updater platform keys at version `0.2.301` and
13 checksum entries covering every asset except `SHA256SUMS` itself.

The production update service returned HTTP 200, `X-Update-Channel: latest`,
version `0.2.301`, channel confirmation, nonempty signatures, and immutable
URLs from that release for installed `0.1.4` on all five Tauri targets:
macOS arm64/x86_64, Linux arm64/x86_64, and Windows x86_64. The current
`0.2.301` Latest version returned HTTP 204. Explicit Stable and old requests
without a channel still returned signed `0.1.4` metadata with Stable channel
confirmation. `/rstorrent/version?channel=latest` returned `0.2.301`, while
Stable returned `0.1.4`.

Two earlier attempts stayed private. Run
[`36317605866`](https://github.com/kzahel/rstorrent/actions/runs/36317605866)
found that a rehearsal artifact upload also ran during publication and failed
on a colon in a system copyright filename. Commit `19add77a` confined that
upload to rehearsal and narrowed its package paths. Run
[`36323892806`](https://github.com/kzahel/rstorrent/actions/runs/36323892806)
then passed source and four signed package jobs, but Windows evaluated a Bash
environment reference in PowerShell and received an empty package version.
Commit `30523da4` selected Bash for both Latest identity steps. The finalizer
refused publication in both failed attempts; their drafts remain private.

Hosted publication and production route checks are complete. Installed
Stable-to-Latest selection, signed replacement/relaunch, and return-to-Stable
without downgrade remain separate acceptance evidence.
