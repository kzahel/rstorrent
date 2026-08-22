#!/usr/bin/env bash
set -euo pipefail

android_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$android_root/../.." && pwd)"
android_sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
ndk_root="$android_sdk/ndk/27.0.12077973"
generated_root="$android_root/app/build/generated"

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

"$android_root/gradlew" -p "$android_root" clean

cargo ndk \
    -t x86_64 \
    -t arm64-v8a \
    -P 28 \
    -o "$generated_root/jniLibs" \
    build --release -p rstorrent-android --lib

cargo build -p rstorrent-android --lib
case "$(uname -s)" in
    Darwin)
        bindgen_library="$repository_root/target/debug/librstorrent_android.dylib"
        ;;
    Linux)
        bindgen_library="$repository_root/target/debug/librstorrent_android.so"
        ;;
    *)
        echo "Unsupported UniFFI bindgen host: $(uname -s)" >&2
        exit 1
        ;;
esac
if [[ ! -f "$bindgen_library" ]]; then
    echo "UniFFI bindgen library is unavailable at $bindgen_library" >&2
    exit 1
fi
cargo run \
    -p rstorrent-android \
    --features bindgen \
    --bin rstorrent-uniffi-bindgen \
    -- generate \
    --library "$bindgen_library" \
    --crate rstorrent_android \
    --config "$repository_root/crates/rstorrent-android/uniffi.toml" \
    --language kotlin \
    --out-dir "$generated_root/source/uniffi" \
    --no-format
cargo run \
    -p rstorrent-android \
    --features bindgen \
    --bin rstorrent-uniffi-bindgen \
    -- generate \
    --library "$bindgen_library" \
    --crate rstorrent_session \
    --config "$repository_root/crates/rstorrent-session/uniffi.toml" \
    --language kotlin \
    --out-dir "$generated_root/source/uniffi" \
    --no-format

"$android_root/gradlew" -p "$android_root" assembleDebug testDebugUnitTest

apk="$android_root/app/build/outputs/apk/debug/app-debug.apk"
if [[ ! -f "$apk" ]]; then
    echo "Android client APK was not created at $apk" >&2
    exit 1
fi
echo "$apk"
