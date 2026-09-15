#!/usr/bin/env bash
# macOS codesign + notarization for the alc-ng release binaries.
#
# Called by goreleaser as a universal_binaries post hook:
#   bash scripts/macos-sign.sh "<path to universal binary>"
#
# It runs right after the lipo merge and before goreleaser archives the
# binary, so the distributed tar.gz contains a signed artifact.
#
# Behavior is controlled by two environment variables:
#   MACOS_CODESIGN_IDENTITY  e.g. "Developer ID Application: My Org (TEAMID)"
#   MACOS_NOTARY_PROFILE     keychain profile name created with
#                            `xcrun notarytool store-credentials`
# Both are optional: if MACOS_CODESIGN_IDENTITY is unset the script is a
# no-op (local builds, snapshot runs without secrets). If only the identity
# is set, binaries are signed but not notarized.
#
# Notes:
#   - Notarization uploads a throwaway ZIP containing the binary; submitting
#     a bare Mach-O directly is not supported by notarytool and a tar.gz
#     itself cannot be stapled, so the ticket is never kept locally.
#   - `--options runtime` (hardened runtime) is required by Apple for
#     notarization. This CLI binary needs no entitlements.

set -euo pipefail

bin="${1:?usage: macos-sign.sh <binary-path>}"

if [[ -z "${MACOS_CODESIGN_IDENTITY:-}" ]]; then
  echo "[macos-sign] MACOS_CODESIGN_IDENTITY not set, skipping macOS signing for: ${bin}"
  exit 0
fi

echo "[macos-sign] codesigning: ${bin}"
codesign --force \
  --sign "${MACOS_CODESIGN_IDENTITY}" \
  --options runtime \
  --timestamp \
  "${bin}"

codesign --verify --strict --verbose=2 "${bin}"
echo "[macos-sign] codesign OK: $(codesign -dv "${bin}" 2>&1 | head -1)"

if [[ -z "${MACOS_NOTARY_PROFILE:-}" ]]; then
  echo "[macos-sign] MACOS_NOTARY_PROFILE not set, skipping notarization"
  exit 0
fi

stage="$(mktemp -d)"
trap 'rm -rf "${stage}"' EXIT

zip_name="$(basename "${bin}").zip"
cp "${bin}" "${stage}/"
( cd "${stage}" && zip -q "${zip_name}" "$(basename "${bin}")" )

echo "[macos-sign] submitting for notarization (this may take minutes)..."
xcrun notarytool submit "${stage}/${zip_name}" \
  --keychain-profile "${MACOS_NOTARY_PROFILE}" \
  --wait

echo "[macos-sign] notarization OK"
