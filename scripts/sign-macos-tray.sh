#!/usr/bin/env bash
# Assina o executável ou bundle com identidade estável. A assinatura ad hoc
# muda a cada build e pode invalidar a autorização de Acessibilidade.
#
# Uso: scripts/sign-macos-tray.sh <executável-ou-app>
# CODESIGN_IDENTITY escolhe o certificado. Por padrão, usa o primeiro válido
# Developer ID Application ou Apple Development. Ad hoc exige escolha explícita
# com CODESIGN_IDENTITY=- e serve somente ao desenvolvimento local.
set -euo pipefail

target=${1:?usage: sign-macos-tray.sh <binary-or-app>}
identifier=com.akitaonrails.ai-usagebar-tray

if [[ -d $target ]]; then
  plist=$target/Contents/Info.plist
  if [[ ! -f $plist ]]; then
    echo "error: bundle is missing Contents/Info.plist: $target" >&2
    exit 1
  fi
  identifier=$(plutil -extract CFBundleIdentifier raw -o - "$plist")
  if [[ ! $identifier =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then
    echo "error: bundle has an invalid CFBundleIdentifier" >&2
    exit 1
  fi
elif [[ ! -f $target ]]; then
  echo "error: signing target does not exist: $target" >&2
  exit 1
fi

identity=${CODESIGN_IDENTITY:-}
if [[ -z $identity ]]; then
  if ! identity=$(security find-identity -v -p codesigning |
    sed -nE 's/^ *[0-9]+\) [0-9A-F]{40} "((Developer ID Application|Apple Development): .*)"$/\1/p' |
    sed -n '1p'); then
    echo "error: could not read code-signing identities; check Keychain access or set CODESIGN_IDENTITY." >&2
    exit 1
  fi
fi

if [[ -z $identity ]]; then
  echo "error: no valid code-signing identity found; set CODESIGN_IDENTITY to a valid certificate." >&2
  echo "For local development only, CODESIGN_IDENTITY=- explicitly selects ad hoc signing." >&2
  exit 1
fi

if [[ $identity == - ]]; then
  echo "warning: explicit ad hoc signing may reset Accessibility permission after rebuilding." >&2
fi

codesign --force --sign "$identity" --identifier "$identifier" "$target"
codesign --verify --strict "$target"
echo "signed $target as $identifier with ${identity/#-/ad-hoc}"
