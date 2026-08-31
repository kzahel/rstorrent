#!/usr/bin/env bash
set -euo pipefail

script_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
client_root=$(cd "${script_root}/.." && pwd)
task_root=$(mktemp -d "${TMPDIR:-/tmp}/rstorrent-ios-localization.XXXXXX")
iphone_id=""
ipad_id=""

cleanup() {
  for device in "${iphone_id}" "${ipad_id}"; do
    if [[ -n "${device}" ]]; then
      xcrun simctl shutdown "${device}" >/dev/null 2>&1 || true
      xcrun simctl delete "${device}" >/dev/null 2>&1 || true
    fi
  done
  if [[ "${task_root}" == *rstorrent-ios-localization.* ]]; then
    rm -rf "${task_root}"
  fi
}
trap cleanup EXIT

runtime=$(
  xcrun simctl list runtimes --json |
    jq -r '[.runtimes[] | select(.isAvailable and (.identifier | contains("iOS")))] | sort_by(.version) | last | .identifier'
)
if [[ -z "${runtime}" || "${runtime}" == "null" ]]; then
  echo "No available iOS simulator runtime" >&2
  exit 1
fi

iphone_type="com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro"
ipad_type="com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB"
suffix="${PPID}-$$"
iphone_id=$(xcrun simctl create "rstorrent-localization-iphone-${suffix}" "${iphone_type}" "${runtime}")
ipad_id=$(xcrun simctl create "rstorrent-localization-ipad-${suffix}" "${ipad_type}" "${runtime}")

run_tests() {
  local device_id=$1
  shift
  xcrun simctl boot "${device_id}"
  xcrun simctl bootstatus "${device_id}" -b
  xcodebuild \
    -quiet \
    -project "${client_root}/RSTorrent.xcodeproj" \
    -scheme RSTorrent \
    -destination "platform=iOS Simulator,id=${device_id}" \
    -derivedDataPath "${task_root}/DerivedData" \
    -resultBundlePath "${task_root}/${device_id}.xcresult" \
    COMPILER_INDEX_STORE_ENABLE=NO \
    test \
    "$@"
  xcrun simctl shutdown "${device_id}"
}

run_tests "${iphone_id}"
run_tests "${ipad_id}" -only-testing:RSTorrentUITests/LocalizationUITests

jq -n \
  --arg runtime "${runtime}" \
  --arg iphone "${iphone_type}" \
  --arg ipad "${ipad_type}" \
  '{ios_localization_matrix: [
    {device: $iphone, runtime: $runtime, scope: "unit + English UI + double-length/RTL pseudo UI", result: "passed"},
    {device: $ipad, runtime: $runtime, scope: "double-length/RTL pseudo UI", result: "passed"}
  ]}'
