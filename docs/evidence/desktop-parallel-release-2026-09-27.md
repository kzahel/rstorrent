# Parallel Desktop Release Pipeline

Commit `80b0446860909eb7a754c1319631fc288127560a` changed the signed
release workflow to build five independent native package legs concurrently.
Each leg verified and staged its own packages, signatures, source/run identity,
and SHA-256 digests. A single collector assembled the 15-key updater manifest
and checked the complete set before any publication step.

Local `actionlint`, 37 focused release tests, and `git diff --check` passed.
Main CI
[`36341427435`](https://github.com/kzahel/rstorrent/actions/runs/36341427435)
passed all jobs, including the Windows unsigned package check.

Credentialed rehearsal
[`36341433008`](https://github.com/kzahel/rstorrent/actions/runs/36341433008)
passed the source gate, all five signed package legs, and the sole collector.
The jobs overlapped on 2026-09-27 (UTC):

| Package leg | Start | Finish |
| --- | --- | --- |
| Windows x86_64 | 18:54:25 | 19:08:05 |
| Linux x86_64 | 18:55:00 | 19:17:13 |
| Linux arm64 | 18:55:37 | 19:12:11 |
| macOS x86_64 | 18:58:09 | 19:13:36 |
| macOS arm64 | 18:58:31 | 19:15:23 |

The package stage spanned 22 minutes 48 seconds from its first start to last
finish; the five job durations total about 85 minutes. Source checks and the
collector are separate from those figures.

The run retained five private signed package artifacts and a private collector
artifact with a 23-asset inventory and 15-key `latest.json`. The collector
reported `isDraft=true` for its synthetic rehearsal inventory. No tag or
GitHub Release was created.

Forced Nightly Desktop run
[`36344554527`](https://github.com/kzahel/rstorrent/actions/runs/36344554527)
selected the same successful main CI commit and version `0.2.401`. Its release
source checks, five signed package legs, and sole collector all passed. The
package jobs ran from 19:41:56 to 19:55:24 UTC, overlapping throughout; the
collector completed at 19:55:54 UTC. It validated the assembled assets, made
one private tagged draft, checked the uploaded GitHub digests, and published
[`desktop-latest-v0.2.401`](https://github.com/kzahel/rstorrent/releases/tag/desktop-latest-v0.2.401)
as a public prerelease at the selected source commit.

An independent public read found `latest.json` at version `0.2.401` with all
15 updater platform keys. The 13 public assets match both `SHA256SUMS` and
GitHub SHA-256 digests. The production update service returned HTTP 200,
`X-Update-Channel: latest`, signed `0.2.401` metadata, and immutable URLs from
this release for macOS arm64/x86_64, Linux arm64/x86_64, and Windows x86_64.
Current `0.2.401` Latest checks returned HTTP 204. Explicit Stable and old
requests without `channel` still returned `0.1.4`; `/version` reported Latest
`0.2.401` and Stable `0.1.4`.

The rehearsal used the same source commit as publication. The later one-line
change to the distribution inventory artifact name gives the two Linux legs
distinct evidence names; those inventories are separate from the release
collector inputs.

Installed channel switching and signed replacement/relaunch remain separate
acceptance evidence.
