# Tactical 219: Dependency And CI Maintenance

Status: **Complete (2026-09-26).** Dependency, review automation and hosted
qualification landed on `main`; no release was published.

Topics: `beta-release-readiness`, `capability-readiness`

## Scope And Stopping Condition

Repair the newly reported rustls and website npm advisories, include both npm
lockfiles in scheduled advisory review, retain bounded useful failure evidence,
and establish small reviewed dependency-update PRs. Enable GitHub Dependabot
alerts and security updates, then qualify the exact source in local and hosted
CI. Stop when current audits and relevant product checks pass, automation is
observable, and no release is published by this tactical.

The September 22 storage recovery failure receives bounded triage and better
failure evidence here; it is not an advisory finding. Signed Windows update
qualification, a supported beta declaration, broad major-version migrations,
and historical package remediation are outside this slice.

## Invariants And Direction

- Keep the vendored GLib source proof, exact warning inventory, expiry and
  tagged-release fail-closed policy. Do not suppress a new advisory.
- Update the existing compatible rustls line to at least the upstream patched
  0.23.45; keep every application and Android graph on one reviewed resolution.
- Update the website's pinned Astro graph and confirm the lockfile audit. An
  advisory's package presence does not alone prove a reachable website exploit.
- Produce a bounded summary artifact before failing review on findings, without
  uploading raw reports, local paths, credentials or a full dependency graph.
- Monitor Cargo, web npm, website npm, Android Gradle and GitHub Actions with
  limited weekly PRs. Major upgrades and automatic merging remain manual.
  Preserve SHA-pinned Actions and the repository's deliberate direct-main
  incubation policy.
- The audit job owns synchronous collection and review only. No runtime task,
  application boundary or protocol state changes. Platform/engine layers keep
  their existing dependency direction.

## Validation

Run both npm audits, the exact Cargo collector/reviewer, focused review-tool
failure tests, website check/build, web type/unit/build, Rust fmt/clippy/tests,
and proportional Android and desktop checks. Validate workflow syntax and
Dependabot configuration against current GitHub documentation. After push,
inspect ordinary CI, website and scheduled/manual advisory results. Record
actual versions, findings and any remaining blockers before closure.

## Local Checkpoint, 2026-09-26

`cargo update -p rustls --precise 0.23.45` resolves the moderate
RUSTSEC-2026-0285 finding and updates its compatible `rustls-webpki` and
AWS-LC dependencies. The workspace minimum is now 0.23.45. The upstream
rustls advisory affects 0.23.13 through 0.23.44. This single lockfile
resolution serves engine, remote and desktop graphs; `cargo tree -i` identified
the relevant direct and transitive consumers before the update.

The website's pinned Astro moves from 7.1.6 to 7.3.5. Its refreshed lockfile
resolves all seven prior npm package findings, including the upstream AVIF
processing advisory and the remaining transitive findings. Web npm retains
zero. No claim is made that the static website exposed attacker-controlled
AVIF processing.

The reviewer now checks both npm reports, includes all three lockfile hashes
on success and writes a bounded finding summary before failing a review. A
synthetic Cargo finding was checked through the actual CLI: it exited 1 and
wrote its advisory/package/version summary. The GLib source and warning
inventory stayed enforced; the exact local review reports zero vulnerabilities
and `release_ready=true`.

Local gates pass: 17 distribution-review tests; `npm ci`, Astro check/build;
web typecheck, 386 unit tests and production/CSP build; workspace formatting,
warning-denying Clippy and all workspace tests; actionlint; YAML parse and
five-ecosystem Dependabot inventory. Hosted CI and GitHub security settings
remain to be qualified/applied at this checkpoint.

The September 22 weekly storage run reported 9,699,328 uploaded bytes after
post-commit restart against 6,291,456 expected. Five consecutive local
post-commit cases pass, and fresh Linux hosted verification run `36231952351`
passes the complete application/topology/checkpoint matrix at the unchanged
source revision. The cause of the one failure remains unproven. The exact
upload assertion stays strict; a future failure will also report durable,
valid and missing piece counts and excess upload bytes. No engine state or
protocol claim changes on this evidence.

## Hosted And Automation Checkpoint, 2026-09-26

Source commit `62fc537a` is on `main`. Website run `36232402740` builds and
deploys successfully. Manual advisory run `36232441484` collects all three
lockfile reports and passes with `release_ready=True`. GitHub's vulnerability
alerts GET returns 204, and automated security fixes report enabled and
unpaused. The first version-update scans created Cargo, web, website, Android
and Actions PRs, proving all five ecosystem paths are recognized. No PR is
auto-merged.

GitHub's alert list now contains one medium GLib 0.18.5 alert for
GHSA-wrw7-89jp-8q8g. The registry version remains 0.18.5, while Tactical
`218` verifies the exact locally patched GLib source and preserves the
original advisory in the Cargo audit projection. GitHub's version-based alert
does not verify that local backport. Keep it visible for a future upstream
version or source-remediation decision; do not dismiss it as if the registry
package had been upgraded. The exact three-lockfile review still reports
zero vulnerabilities against the verified source and seven reviewed warnings.

The first scan also showed that two routine PRs per ecosystem start a large
cross-platform queue. Website TypeScript 7 PR `#1` fails `npm ci` because
`@astrojs/check 0.9.10` requires TypeScript 5 or 6. The follow-up config
reduces the routine limit to one PR per ecosystem and allows only patch/minor
version updates; GitHub documents `allow.update-types` as version-only, so
security updates remain eligible. Major migrations stay separately reviewed.
Four first-scan major PRs (`#1`, `#5`, `#7`, `#9`) were closed without merging;
five routine PRs (`#2`, `#3`, `#4`, `#6`, `#8`) were open at that checkpoint.
The web patch group in `#4` includes Tauri CLI 2.11.5; its Linux ARM64
packaging check stops at the deliberate exact-version guard in
`prepare-appimage-notices.py`. Review the AppImage notice hook against that
CLI before accepting the group. This gate alone does not establish a Tauri
regression.
The first-scan PR check runs were cancelled to free hosted capacity for main
qualification; rerun the relevant checks before considering any of those PRs.
Ordinary cross-platform CI and the exact-source storage matrix are still in
flight at this checkpoint.

A later Dependabot refresh replaced the first Cargo/web patch PRs with
oversized groups: `#10` held 14 website changes, `#11` held 44 web changes,
and `#12` held 100 Cargo changes. These were closed without merging. Commit
`7b9ec29c` removes broad patch grouping while retaining one open routine PR
per ecosystem and the patch/minor version-update allow rule. New individual
proposals need their own CI confirmation before review. All five refreshed
Dependabot scans pass. The observed open set is one individual proposal in
each ecosystem: Cargo `#3`, web `#13`, website `#14`, Android `#8`, and
Actions `#6`. The new PRs' broad CI runs were cancelled to prioritize main;
rerun those checks before merging.

The first main CI run `36232402842` exposed a gateway test race during
workspace tests: the HTTP view-set shutdown test accepted a normal 200 update
from its unthrottled live torrent-list subscription before the shutdown close
could wake the poll with 410. The test now uses the supported 60-second
delivery interval, longer than its 20-second wait, so ordinary patches stay
pending while it exercises gateway shutdown. This changes only test setup;
the view-set and gateway runtime behavior remain unchanged. The focused case
passes 20/20 local repetitions, the full gateway suite passes 45/45, and
workspace formatting passes. The later main run `36235256275` qualifies this
correction.

The next main run `36234599395` reached the workflow/release-tools job and
found an existing GLib backport test still calling `audit.review` with two
reports after the reviewer gained a website report argument. All four calls
now pass a zero-finding website fixture. Local GLib source verification,
10 backport tests, 17 distribution-review tests, desktop release validation,
23 desktop tool tests, and both three-case release-manifest suites pass.
The Crostini and headless shell integrity suites require Linux and remain
covered by the hosted release-tools job. Main run `36235256275` qualifies the
corrected test call.

The exact-source storage verification run `36232442627` passes its complete
Linux matrix after the dependency and audit changes. The corrected
release-tools job passes in main CI run `36235256275`, including the Linux-only
bootstrap integrity suites.

## Completion And Next Boundary

Main CI run `36235256275` passes all ten required jobs at `39b4a127`:
Rust/loopback, web, extension, release tools, Android, iOS, and desktop on
Windows, macOS, Linux x86_64 and Linux ARM64. The separately dispatched
website, three-lockfile advisory, and exact-source storage runs pass at
`62fc537a`, before the later test/config/documentation corrections. Those
later commits did not change the audited dependency graphs or storage runtime.
The final source run `36239731206` again passes all ten required jobs at
`e1536fa8`, after the ungrouped Dependabot config and both CI test fixes.

The original dependency and hosted qualification gates are met. Routine
Dependabot PRs remain unmerged for review; rerun their checks before merging.
Any individual Tauri CLI update must pass the AppImage notice-hook review
identified by closed group `#4`. GitHub's one GLib alert remains visible
alongside the verified local backport. A future weekly storage result may
help diagnose the isolated September 22 failure, but its cause is not claimed
solved by this slice.

The configuration-only main run `36238069794` passed nine required jobs and
failed the Rust job on a session integration test that assumed a probed
preferred port stayed free until application startup. Its automatic listener
legitimately selected the next port, with TCP and UDP still coordinated.
Commit `e1536fa8` removes only that preferred-port assertion; it retains the
live TCP/UDP pairing, DHT source, and TCP acceptance checks. The focused test
passes 20 local repetitions, the session library passes 346 tests with two
ignored, and workspace formatting passes. Exact-source hosted main CI run
`36239731206` passes all ten required jobs.
