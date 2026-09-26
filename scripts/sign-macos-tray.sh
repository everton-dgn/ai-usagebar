#!/usr/bin/env bash
# Sign the macOS tray with a stable identity so its Accessibility permission
# survives rebuilds. macOS keys that permission to the code signature: an
# ad-hoc signature changes with every build and drops the grant, while a
# certificate plus a fixed identifier keeps it.
#
# Usage: scripts/sign-macos-tray.sh <binary>
# CODESIGN_IDENTITY overrides the identity; by default the first valid
# "Developer ID Application" or "Apple Development" one in the keychain.
set -euo pipefail

binary=${1:?usage: sign-macos-tray.sh <binary>}
identifier=com.akitaonrails.ai-usagebar-tray

identity=${CODESIGN_IDENTITY:-}
if [[ -z $identity ]]; then
  identity=$(security find-identity -v -p codesigning |
    sed -nE 's/^ *[0-9]+\) [0-9A-F]{40} "((Developer ID Application|Apple Development): .*)"$/\1/p' |
    head -n 1)
fi

if [[ -z $identity ]]; then
  echo "warning: no code-signing identity found; signing ad-hoc." >&2
  echo "warning: the Accessibility permission will reset on every rebuild." >&2
  identity=-
fi

codesign --force --sign "$identity" --identifier "$identifier" "$binary"
codesign --verify "$binary"
echo "signed $binary as $identifier with ${identity/#-/ad-hoc}"
