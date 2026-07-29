#!/usr/bin/env bash
set -euo pipefail

probe_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
android_sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
ndk_root="$android_sdk/ndk/27.0.12077973"

if [[ ! -d "$android_sdk/platforms/android-35" ]]; then
    echo "Android platform 35 is unavailable under $android_sdk" >&2
    exit 1
fi
if [[ ! -d "$ndk_root" ]]; then
    echo "Android NDK 27.0.12077973 is unavailable under $android_sdk" >&2
    exit 1
fi
for rust_target in x86_64-linux-android aarch64-linux-android; do
    if ! rustup target list --installed | grep -qx "$rust_target"; then
        echo "Install the Rust target with: rustup target add $rust_target" >&2
        exit 1
    fi
done

export ANDROID_HOME="$android_sdk"
export ANDROID_NDK_HOME="$ndk_root"

cargo ndk \
    -t x86_64 \
    -t arm64-v8a \
    -P 28 \
    -o "$probe_root/app/src/main/jniLibs" \
    --manifest-path "$probe_root/native/Cargo.toml" \
    build --release

"$probe_root/gradlew" -p "$probe_root" clean assembleDebug

apk="$probe_root/app/build/outputs/apk/debug/app-debug.apk"
if [[ ! -f "$apk" ]]; then
    echo "Probe APK was not created at $apk" >&2
    exit 1
fi
echo "$apk"
