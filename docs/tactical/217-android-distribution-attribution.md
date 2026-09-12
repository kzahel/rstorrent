# Tactical 217: Android Distribution Attribution

Status: **Complete locally and hosted (2026-09-12).**
All 12 jobs pass at the implementation revision recorded below.

Topics: `beta-release-readiness`, `capability-readiness`

## Scope And Stopping Condition

Embed attributable notices for the exact Android Rust and resolved Maven/AAR
runtime graphs in debug and release assets. Verify their presence and content
in APK/AAB outputs before distribution. The current debug APK has six native
libraries across two ABIs and no license/notice entries; its release Maven
graph returns 80 artifact entries, including one repeated identical Core
AAR; there are 79 unique resolved artifacts. Package existence and passing runtime tests
therefore do not establish attribution coverage.

Stop with original notice/declaration provenance, exact artifact/source
metadata, bounded failing checks, a real two-ABI debug APK, release asset
construction without signing credentials, hooked APK/AAB release inspection,
and the applicable hosted Android evidence. No signing, publishing, supported
version declaration, new runtime dependency, application API, UI or engine
behavior is in scope. Legal conclusions and corresponding-source delivery
policy remain release decisions.

## Invariants, Ownership And Bounds

Gradle owns one generated asset directory per build variant. Its resolved
runtime configuration supplies artifact identities and exact files; resolve
POMs and parent POMs through the same configured repositories. Never infer a
license from a package/group name. Preserve supplied root/META-INF license
and notice texts, including inside AAR classes.jar; reject missing or unknown
grants. Distinguish original notices from a manifest-only grant plus standard
license text. Do not include class files merely because their names contain
`License` or `Copyright`.

JNA uses the repository's already-selected Apache-2.0 option. Its bundled
native libffi attribution must also be preserved. The local Rustls AAR binds
to the exact Cargo package and original license files, not a fabricated Maven
declaration. Guava's three license-less child POMs require their declared
parent POM evidence. Record hashes and source locators, never local cache or
build paths, in distributable manifests.

Reuse the existing pinned cargo-about 0.9.2 and original Rust notice policy
for the union of both Android targets. Generated temporary graph paths remain
outside assets. Fail closed on graph, license, checksum, ZIP path/size or
variant drift; cap artifacts, nested archive work, notice texts and output.
Gradle dependency/task ownership orders generation before packaging/signing.
There are no background application tasks or changes to module dependency
direction. No new Gradle license plugin is introduced.

## Source Inspection And Validation

Inspect Gradle 8.11.1's resolved artifact and Maven POM query APIs, the actual
AGP 8.10.1 variant/build task path, exact cached runtime POMs/AARs/JARs and
parent grants, and JNA 5.17.0's original native notices. The initial read-only
Gradle query returns 80 release entries (79 unique artifacts): 75 direct Apache POM declarations,
one dual-license JNA declaration, three inherited Guava declarations, and
the local Rustls component without a Maven license declaration. Existing JNA
and AndroidX Navigation artifact notices do not survive into the APK.

Validate deterministic missing/changed grants, inherited metadata, original
notice preservation, archive bounds and malicious paths, missing/modified
packaged assets, variant separation and graph completeness. Build the actual
Android assets/APK, run JVM/lint checks proportional to Gradle changes, and
validate the hooked release tooling and workflow syntax. Update the owning
readiness topics and commit a substantial validated slice.


## Implementation And Local Evidence

Gradle's public `SourceDirectories.addGeneratedSourceDirectory` API owns each
task's `DirectoryProperty`; generation writes to that assigned location. The
actual APK gate exposed and repaired an initial write to a guessed path that
left generation successful but the package empty. The current APK contains
both notice assets. The generator deduplicates only identical component/file
entries in Gradle's resolved set, not distinct artifacts or license evidence.
POM/parent identities, duplicate/cyclic metadata, unknown grants, native AAR
checksum drift, nested ZIP bounds/paths, missing/modified assets and unexpected
native libraries fail closed. Metadata generation runs before each requested
package rather than reusing stale POM/license inputs.

Source review uses Gradle 8.11.1's `ArtifactResolutionQuery`,
`MavenPomArtifact` and resolved component/file identities. The exact AGP
8.10.1 API was also inspected from its installed API JAR and compared with the
[official generated-assets recipe](https://github.com/android/gradle-recipes/tree/agp-8.10/addGeneratedSourceFolder).
Graphics-path 1.0.1's official release note identifies compiler-flag-only
changes; its release branch endpoint was unavailable (503), so supplemental
Apache notices come from the pinned pre-release mirror history at
`794e3806700833665f48f56f7dd3581642a6057f`. They preserve the original comments
from `Conic.cpp`, `math/TVecHelpers.h`, `math/compiler.h` and `math/vec2.h`, not
implementation source. The actual artifact also declares Apache-2.0. This is
notice provenance, not a claim of reproducing Google's native build.
JNA/libffi source revision and original hashes live in
`distribution/licenses/android-sources.json`; Rustls reuses the existing exact
Cargo manifest-bound supplement. Native AAR hashes must remain exact.

Local validation passes the two-ABI debug APK, credential-free release asset
generation, **113** JVM tests and Android lint. Debug assets cover **83** Maven
artifacts; release assets cover **79**; both include **205** Rust packages.
Final APK inspection accounts for all **six** native libraries. The ten new
negative/integrity tests pass alongside the existing Android release tooling
(**22** Python tests total);
all 16 distribution-review and 12 desktop release-validator tests pass.
Workflow lint, shell syntax and release source checks pass. All five desktop
notice generations still pass, and the existing macOS ARM64 output remains
byte-identical after extracting the shared Rust generator function.

The final local debug notice text is 2,057,148 bytes, SHA-256
`36fe9f028fc8c642902105e910177971b9adcede9c9666d858588e8ab2c54094`;
its manifest is
`f6b132e23a2524b0a39fdfb26ff2bf78f4f9f276eca48b3ec1b412d5ca951ff3`.
Release assets contain 2,055,775 notice bytes, with manifest
`62af1609707f4360074d96ac834392402307d35145d78828cf02c76aafc7e86b`.
Local APK SHA-256:
`0d7cc2a5fea9903053215bc98b5890646380f1169f653157e750ca9c6e77c2a7`.
No release APK/AAB was signed or published.

A read-only OSV query on 2026-09-12 returns no advisory matches for all 79
release Maven coordinates. This is a dated registry query, not universal
security clearance or a substitute for Cargo/npm review. The hosted Android
runtime and package gate subsequently passed as recorded below.

## Hosted Android Qualification

The Android job in run
[`34689662485`](https://github.com/kzahel/rstorrent/actions/runs/34689662485)
passes at `f7e50a753e03896ffbce56648543ec04c1af90be`: build/JVM/lint checks,
22 Python release-tool tests, final APK notice inspection and the owned API 35
x86_64 revision-9 SAF lifecycle. Both notice assets are exported from the
verified APK itself and retained with the structured inspection report.
The hosted manifest is byte-identical to the local one:
`f6b132e23a2524b0a39fdfb26ff2bf78f4f9f276eca48b3ec1b412d5ca951ff3`.
The hosted APK SHA-256 is
`8057196c711d9c289ce90a68de3abfee60bb8afd96a90008a774c3eb5e804049`.
Preconfirmation upload is zero, skipped files remain absent, force recheck
passes, cancellation joins while retaining payload, removal is exact, and
storage handles peak at **3/40**. The owned AVD is removed and cleanup passes.

An additional archive audit found Emoji2's `libs/repackaged.jar` with 49
relocated FlatBuffers classes and the `FLATBUFFERS_1_12_0` API marker. Neither
that embedded JAR nor the original FlatBuffers Java 1.12.0 JAR contains a
standalone license/notice file. The original Maven POM declares Apache 2.0,
whose standard text is already included; its SHA-256 is
`cb226baf546260770f21e8152a6aa88ba15230d739f750df480f2a668d43e0eb`.
A separate dated OSV query for that Java coordinate also returns no matches.
These are inspected archive/metadata facts, not a claim that relocation is a
byte-identical rebuild of upstream. The Emoji2 AAR's full hash already binds
its embedded bytes in the manifest.

Pinned cargo-about 0.9.2 is installed in the local Cargo toolchain so ordinary
Android builds can run the new required hook after scratch cleanup.

The complete workflow finishes successfully at 2026-09-12 11:19:29 UTC:
all 12 jobs pass, including both Linux AppImages, Windows/macOS packaging,
Rust/interop, web, iOS and the extended verification/advisory workflows.
This closes the tactical stopping condition; signed release qualification
and the campaign's GLib/source-maintenance decision remain separate.
