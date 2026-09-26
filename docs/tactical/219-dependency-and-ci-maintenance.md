# Tactical 219: Dependency And CI Maintenance

Status: **Active (2026-09-26).** User-authorized maintenance and push.

Topics: `beta-release-readiness`, `capability-readiness`

## Scope And Stopping Condition

Repair the newly reported rustls and website npm advisories, include both npm
lockfiles in scheduled advisory review, retain bounded useful failure evidence,
and establish small reviewed dependency-update PRs. Enable GitHub Dependabot
alerts and security updates, then qualify the exact source in local and hosted
CI. Stop when current audits and relevant product checks pass, automation is
observable, and no release is published by this tactical.

The September 22 storage recovery failure receives bounded triage and better
failure evidence here; it is not an advisory finding. Signed Windows update qualification, a
supported beta declaration, broad major-version migrations, and historical
package remediation are outside this slice.

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
