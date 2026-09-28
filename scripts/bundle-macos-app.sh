#!/usr/bin/env bash
# Monta e assina target/release/AI Usage.app para macOS Apple Silicon.
#
# Não instala nem abre o aplicativo. O bundle é montado em um diretório
# temporário novo e só substitui o anterior depois de assinado e verificado; o
# anterior vai para a lixeira.
#
# BUNDLE_ID mantém a identidade da cópia em uso: a permissão de Acessibilidade
# e o identificador de assinatura dependem dela. Trocar o valor exige nova
# autorização no macOS.
# CODESIGN_IDENTITY segue as regras de scripts/sign-macos-tray.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

bundle_id=${BUNDLE_ID:-ai-usagebar-tray}
app_name="AI Usage"
executable=ai-usagebar-tray
release_dir=target/release
app="$release_dir/$app_name.app"

if [[ $(uname -s) != Darwin || $(uname -m) != arm64 ]]; then
  echo "error: the app bundle is built only on macOS with Apple Silicon (arm64)." >&2
  exit 1
fi
if [[ ! $bundle_id =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then
  echo "error: invalid BUNDLE_ID: $bundle_id" >&2
  exit 1
fi

version=$(sed -n '/^version = "\(.*\)"/{s//\1/p;q;}' Cargo.toml)
if [[ -z $version ]]; then
  echo "error: could not read the version from Cargo.toml" >&2
  exit 1
fi

cargo build --release --locked --bin "$executable"
binary="$release_dir/$executable"
if [[ $(lipo -archs "$binary") != arm64 ]]; then
  echo "error: $binary is not an arm64-only executable" >&2
  exit 1
fi

staging=$(mktemp -d "${TMPDIR:-/tmp}/ai-usagebar-bundle.XXXXXX")
staged="$staging/$app_name.app"
mkdir -p "$staged/Contents/MacOS"
cp "$binary" "$staged/Contents/MacOS/$executable"
mkdir -p "$staged/Contents/Resources"
cp LICENSE "$staged/Contents/Resources/LICENSE"

plist="$staged/Contents/Info.plist"
plutil -create xml1 "$plist"
plutil -insert CFBundleInfoDictionaryVersion -string 6.0 "$plist"
plutil -insert CFBundleExecutable -string "$executable" "$plist"
plutil -insert CFBundleIdentifier -string "$bundle_id" "$plist"
plutil -insert CFBundleName -string "$app_name" "$plist"
plutil -insert CFBundlePackageType -string APPL "$plist"
plutil -insert CFBundleShortVersionString -string "$version" "$plist"
plutil -insert CFBundleVersion -string "$version" "$plist"
plutil -insert LSUIElement -bool true "$plist"
plutil -lint "$plist" >/dev/null

./scripts/sign-macos-tray.sh "$staged"

if [[ -e $app ]]; then
  if ! command -v trash >/dev/null; then
    echo "error: $app exists and trash is unavailable; the new signed bundle is at $staged" >&2
    exit 1
  fi
  # O bundle anterior é artefato ignorado pelo git: copiar antes da lixeira.
  backup="/tmp/claude-backups/$(date +%Y%m%d_%H%M%S)-app-bundle"
  mkdir -p "$backup"
  cp -Rp "$app" "$backup/"
  echo "backed up the previous bundle to $backup" >&2
  trash "$app"
fi
mv "$staged" "$app"
codesign --verify --strict "$app"
echo "built $app ($version, arm64, $bundle_id)"
