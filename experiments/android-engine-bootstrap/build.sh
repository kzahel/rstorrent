#!/usr/bin/env bash
set -euo pipefail

bootstrap_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$bootstrap_root/../.." && pwd)"
android_sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
ndk_root="$android_sdk/ndk/27.0.12077973"
generated_root="$bootstrap_root/app/build/generated"

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

"$bootstrap_root/gradlew" -p "$bootstrap_root" clean

cargo ndk \
    -t x86_64 \
    -t arm64-v8a \
    -P 28 \
    -o "$generated_root/jniLibs" \
    build --release -p rstorrent-android --lib

cargo build -p rstorrent-android --lib
cargo run \
    -p rstorrent-android \
    --features bindgen \
    --bin rstorrent-uniffi-bindgen \
    -- generate \
    --library "$repository_root/target/debug/librstorrent_android.so" \
    --config "$repository_root/crates/rstorrent-android/uniffi.toml" \
    --language kotlin \
    --out-dir "$generated_root/source/uniffi" \
    --no-format

"$bootstrap_root/gradlew" -p "$bootstrap_root" assembleDebug

apk="$bootstrap_root/app/build/outputs/apk/debug/app-debug.apk"
if [[ ! -f "$apk" ]]; then
    echo "Bootstrap APK was not created at $apk" >&2
    exit 1
fi
echo "$apk"
