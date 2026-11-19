#!/bin/bash
# Mac'te Anlık Çeviri'yi kaynak koddan güncel haliyle derleyip Uygulamalar'a kurar.
#
#   ./tools/mac-guncelle.sh            # güncelle (git pull), derle, kur, aç
#   ./tools/mac-guncelle.sh --pull-yok # git pull yapmadan derle ve kur
#
# Yaptıkları:
#   1. Gerekli araçları denetler (git, Swift, CMake, Rust); tauri-cli yoksa kurar.
#   2. main dalını GitHub'dan çeker.
#   3. Açıksa uygulamayı kapatır.
#   4. Uygulamayı derler (.app), /Applications'a kurar.
#   5. Ekran Kaydı iznini sıfırlar: paket imzasız olduğu için macOS her yeni derlemeyi ayrı bir
#      uygulama sayar ve eski izin geçersiz kalır (-3801 hatası).
#   6. Uygulamayı açar. İzni yeniden vermeniz gerekir (ekrandaki adımlar).
#
# Ayarlar ve indirilmiş çeviri modeli (~/Library/Application Support/PS5Ceviri) korunur.

set -euo pipefail

APP_NAME="Anlık Çeviri"
BUNDLE_ID="com.isacetin.anlikceviri"
APP_DEST="/Applications/${APP_NAME}.app"

kirmizi() { printf '\033[31m%s\033[0m\n' "$*"; }
yesil() { printf '\033[32m%s\033[0m\n' "$*"; }
baslik() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
dur() { kirmizi "Hata: $*"; exit 1; }

[[ "$(uname)" == "Darwin" ]] || dur "Bu betik yalnızca macOS içindir."

# Betik nereden çalıştırılırsa çalıştırılsın depo kökünde çalış.
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

baslik "Araçlar denetleniyor"
# Rust kurulu ama bu terminalde PATH'e eklenmemiş olabilir.
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
command -v git >/dev/null || dur "git yok. Kurmak için: xcode-select --install"
command -v swift >/dev/null || dur "Swift yok. Kurmak için: xcode-select --install"
command -v cmake >/dev/null || dur "CMake yok. Kurmak için: brew install cmake"
command -v cargo >/dev/null || dur "Rust yok. Kurmak için: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
if ! cargo tauri --version >/dev/null 2>&1; then
    echo "tauri-cli kuruluyor (bir kez, birkaç dakika sürer)…"
    cargo install tauri-cli --version "^2" --locked
fi
echo "git $(git --version | awk '{print $3}') · $(swift --version 2>&1 | head -1 | sed 's/.*Swift version \([0-9.]*\).*/Swift \1/') · $(cmake --version | head -1) · $(cargo --version)"

if [[ "${1:-}" != "--pull-yok" ]]; then
    baslik "Güncel kod çekiliyor"
    if ! git diff --quiet || ! git diff --cached --quiet; then
        dur "Depoda kaydedilmemiş değişiklik var. Önce 'git stash' ya da 'git checkout .' ile temizleyin."
    fi
    git checkout main
    git pull --ff-only origin main
fi
echo "Sürüm: $(git log --oneline -1)"

baslik "Uygulama kapatılıyor (açıksa)"
osascript -e "tell application \"${APP_NAME}\" to quit" >/dev/null 2>&1 || true
pkill -f "${APP_NAME}.app/Contents/MacOS/" >/dev/null 2>&1 || true
sleep 1

baslik "Derleniyor (ilk seferde 10-20 dk, sonra birkaç dk)"
if [[ ! -f app/build/outputs/apk/release/app-release.apk ]]; then
    echo "Not: TV uygulaması (APK) derlenmemiş; masaüstü uygulaması çalışır, yalnızca sihirbazın"
    echo "     'TV uygulamasını kur' adımı olmaz. TV'de uygulama zaten kuruluysa sorun değil."
fi
(cd desktop/app && CI=true cargo tauri build --bundles app)

# Ad, dosya sisteminde farklı Unicode biçiminde olabilir; klasördeki tek .app alınır.
BUILT="$(find desktop/target/release/bundle/macos -maxdepth 1 -name '*.app' | head -1)"
[[ -n "$BUILT" ]] || dur "Derlenen uygulama bulunamadı (desktop/target/release/bundle/macos)."

baslik "Uygulamalar klasörüne kuruluyor"
rm -rf "$APP_DEST"
ditto "$BUILT" "$APP_DEST"
echo "Kuruldu: $APP_DEST"

baslik "Ekran Kaydı izni sıfırlanıyor"
tccutil reset ScreenCapture "$BUNDLE_ID" || true

baslik "Uygulama açılıyor"
open "$APP_DEST"

yesil "
Kurulum tamam. Şimdi:
  1. Anlık Çeviri'de Başlat'a basın.
  2. İzin sorusu gelince 'Sistem Ayarları'nı Aç' deyin, listede Anlık Çeviri'yi açın.
  3. 'Çık ve Yeniden Aç' deyin (sormazsa uygulamayı ⌘Q ile kapatıp yeniden açın).
  4. Başlat'a basın. Durum'da 'Remote Play' yeşil olmalı.
"
