#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 path/to/application.dmg" >&2
  exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS is required to verify a DMG with Gatekeeper." >&2
  exit 2
fi

dmg_path="$1"
if [[ ! -f "$dmg_path" ]]; then
  echo "DMG not found: $dmg_path" >&2
  exit 2
fi

mount_dir="$(mktemp -d "${TMPDIR:-/tmp}/fika-music-dmg.XXXXXX")"
mounted=false
cleanup() {
  if [[ "$mounted" == true ]]; then
    hdiutil detach "$mount_dir" -quiet >/dev/null 2>&1 ||
      hdiutil detach "$mount_dir" -force -quiet >/dev/null 2>&1 || true
  fi
  rmdir "$mount_dir" 2>/dev/null || true
}
trap cleanup EXIT

hdiutil attach "$dmg_path" -readonly -nobrowse -mountpoint "$mount_dir" >/dev/null
mounted=true

shopt -s nullglob
app_paths=("$mount_dir"/*.app)
shopt -u nullglob
if [[ ${#app_paths[@]} -ne 1 ]]; then
  echo "Expected one app bundle in $dmg_path, found ${#app_paths[@]}." >&2
  exit 1
fi
app_path="${app_paths[0]}"

codesign --verify --deep --strict --verbose=2 "$app_path"
signature_info="$(codesign --display --verbose=4 "$app_path" 2>&1)"

if ! grep -q '^Authority=Developer ID Application:' <<<"$signature_info"; then
  echo "The app is not signed with a Developer ID Application certificate." >&2
  exit 1
fi

if ! grep -Eq '^CodeDirectory .*flags=.*\([^)]*runtime[^)]*\)' <<<"$signature_info"; then
  echo "The app signature does not enable hardened runtime." >&2
  exit 1
fi

xcrun stapler validate "$app_path"
spctl --assess --type execute --verbose=4 "$app_path"

echo "Verified signed and notarized app in $dmg_path"
