# Android Release Runbook

RSTorrent Canary uses `com.jstorrent.rstorrent`, independent Android versions,
and `android-vX.Y.Z` tags. Debug builds retain `org.rstorrent.bootstrap` for
existing test harnesses. Kotlin's internal namespace remains unchanged.
The Android Release workflow builds both arm64-v8a and x86_64, runs release
JVM tests and lint, and verifies signed APK/AAB artifacts before publication.
Google Play upload and rollout remain manual.

## Release A Version

Add a nonempty `## [0.1.1]` section to `clients/android/CHANGELOG.md` and
commit it first. From a clean `main` checkout:

```sh
scripts/release-android.sh 0.1.1 --dry-run
scripts/release-android.sh 0.1.1
```

The helper validates the version and changelog, increments Gradle's integer
`versionCode`, sets `versionName`, commits the version change, creates an
annotated tag, and atomically pushes main and the tag. Each release version
must increase; the version code is never derived from CI run numbers. If a
push fails, the local commit/tag remain available for inspection and retry;
do not run the bump helper again blindly.

The tag workflow checks that the tag matches the checked-in version, builds
signed artifacts, then creates a GitHub **prerelease** with:

- `rstorrent-android-X.Y.Z.aab` for Play Console;
- `rstorrent-android-X.Y.Z.apk` for direct installation; and
- `SHA256SUMS` for exact artifact hashes.

Android releases do not take over the repository's desktop latest release.
Publication refuses to overwrite an existing release. Failed builds never
reach the publication job. Minification remains disabled for the first
canary; no mapping file is implied or fabricated.

## Build Without A Tag

Use Actions → Android Release → Run workflow, or:

```sh
gh workflow run android-release.yml --ref main
```

The same signed checks run, but results are retained as an Actions artifact
for 14 days and no GitHub release is created. Download the ZIP from the
completed workflow run's Artifacts section. It contains the AAB and APK.
Artifacts in a public repository are not private; signing keys are never
included in them.

## Signing And Backup

A dedicated RSTorrent upload key is configured in these repository secrets:

- `ANDROID_UPLOAD_KEYSTORE_BASE64`
- `ANDROID_UPLOAD_KEYSTORE_PASSWORD`
- `ANDROID_UPLOAD_KEY_ALIAS`
- `ANDROID_UPLOAD_KEY_PASSWORD`

The maintainer's local backup contains `upload.keystore`, `signing.env`,
`upload-certificate.pem`, and `README.txt`. Back up the entire directory to
secure storage; `signing.env` contains the passwords. Keep it outside Git.
The setup handoff supplies its actual local path. Directory permissions are
0700 and files 0600. CI decodes the key into its temporary directory and
removes it when the signing step exits, including on ordinary failure.

`clients/android/upload-certificate.pem` is the public upload certificate,
not a secret. Artifact validation checks both outputs against this certificate.
A future upload-key rotation must update the local backup, Play enrollment,
CI secrets, and this public certificate together.

For a local signed build, load the backup's `signing.env`, then:

```sh
source ~/.profile
source /path/to/android-signing/signing.env
clients/android/build.sh release
```

Missing signing variables fail before building. There is no debug-key
fallback. Google Play App Signing manages the distribution signing key;
the upload key authenticates uploaded bundles. A directly installed APK may
therefore have a different signing identity from the Play-installed app.

## Upload To Play

Select RSTorrent Canary → Test and release → Testing → Internal testing →
Create new release → App bundles → Upload. Upload the `.aab` and follow Play's
app signing enrollment if this is the first upload. Add release notes and
testers, then use Play's review/rollout controls. Promote the tested artifact
to a wider track when ready.

Console form completion does not establish installed release qualification.
Check fresh install, upgrade, background transfers, file access, and ChromeOS
coexistence on representative devices before promoting the canary.

## Toolchain And Verification

Java 17, Gradle 8.11.1, AGP 8.10.1, SDK 36, build-tools 35.0.0,
NDK 28.2.13676358, Rust 1.97.0, and cargo-ndk 4.1.2 are pinned.
AGP 8.10 supports API 36 with this existing Gradle version:
https://developer.android.com/build/releases/agp-8-10-0-release-notes

The workflow checksum-pins bundletool 1.18.3. Validation checks the package,
versions, SDK, non-debuggable manifest, absence of diagnostic receivers,
upload certificate and signatures, matching native libraries in both
artifacts, both supported ABIs, ELF segment alignment, APK ZIP alignment,
and AAB 16 KiB alignment configuration. These are packaging checks, not a
claim of runtime qualification on a 16 KiB device.

```sh
python3 -m unittest discover -s scripts -p 'test_*android*.py'
scripts/release-android.sh --check --tag android-v0.1.0
python3 scripts/validate-android-release.py --bundletool /path/to/bundletool.jar
```

Update the example tag to the current version. Google documents native
alignment requirements at https://developer.android.com/guide/practices/page-sizes.
