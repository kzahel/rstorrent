#!/usr/bin/env bash
set -euo pipefail

script_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
client_root=$(cd "${script_root}/.." && pwd)
repo_root=$(cd "${client_root}/../.." && pwd)
task_root=$(mktemp -d "${TMPDIR:-/tmp}/rstorrent-ios-xliff.XXXXXX")

cleanup() {
  if [[ "${task_root}" == *rstorrent-ios-xliff.* ]]; then
    rm -rf "${task_root}"
  fi
}
trap cleanup EXIT

export_root="${task_root}/export"
copied_client="${task_root}/repo/clients/ios"

xcodebuild \
  -quiet \
  -exportLocalizations \
  -project "${client_root}/RSTorrent.xcodeproj" \
  -localizationPath "${export_root}" \
  -exportLanguage en

test -f "${export_root}/en.xcloc/Localized Contents/en.xliff"
mkdir -p "${task_root}/repo/clients"
ditto "${client_root}" "${copied_client}"
ln -s "${repo_root}/target" "${task_root}/repo/target"

xcodebuild \
  -quiet \
  -importLocalizations \
  -project "${copied_client}/RSTorrent.xcodeproj" \
  -localizationPath "${export_root}/en.xcloc"

cmp \
  "${client_root}/App/Localization/Localizable.xcstrings" \
  "${copied_client}/App/Localization/Localizable.xcstrings"
cmp \
  "${client_root}/App/Localization/InfoPlist.xcstrings" \
  "${copied_client}/App/Localization/InfoPlist.xcstrings"

echo "iOS English XLIFF export/import round trip preserved both string catalogs"
