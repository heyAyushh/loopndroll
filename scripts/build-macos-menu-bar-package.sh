#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

resolve_code_sign_identity() {
  if [[ -n "${LOOPER_MACOS_CODE_SIGN_IDENTITY:-}" ]]; then
    printf '%s\n' "$LOOPER_MACOS_CODE_SIGN_IDENTITY"
    return
  fi

  local apple_development_identity
  apple_development_identity="$(
    security find-identity -v -p codesigning 2>/dev/null |
      awk -F'"' '/Apple Development:/ { print $2; exit }'
  )"
  if [[ -n "$apple_development_identity" ]]; then
    printf '%s\n' "$apple_development_identity"
    return
  fi

  printf '%s\n' "-"
}

resolve_macos_provisioning_profile() {
  if [[ -n "${LOOPER_MACOS_PROVISIONING_PROFILE:-}" ]]; then
    printf '%s\n' "$LOOPER_MACOS_PROVISIONING_PROFILE"
    return
  fi

  local profile
  for profile in "${MACOS_PROVISIONING_PROFILE_DIR}"/*.provisionprofile; do
    [[ -f "$profile" ]] || continue

    local platforms
    platforms="$(security cms -D -i "$profile" 2>/dev/null | plutil -extract Platform json -o - - 2>/dev/null || true)"
    [[ "$platforms" == *OSX* ]] || continue

    local team_identifier
    team_identifier="$(security cms -D -i "$profile" 2>/dev/null | plutil -extract TeamIdentifier.0 raw -o - - 2>/dev/null || true)"
    [[ "$team_identifier" == "$CODE_SIGN_TEAM_ID" ]] || continue

    printf '%s\n' "$profile"
    return
  done
}

APP_NAME="looper"
BUNDLE_ID="dev.looper.app.ios"
LEGACY_BUNDLE_IDS=("dev.looper.app.menubar")
CONTINUATION_ACTIVITY_TYPE="dev.looper.app.continue-session"
CODE_SIGN_TEAM_ID="${LOOPER_MACOS_TEAM_ID:-Z5454ZPPUX}"
CODE_SIGN_IDENTITY="$(resolve_code_sign_identity)"
MACOS_PROVISIONING_PROFILE_DIR="${HOME}/Library/Developer/Xcode/UserData/Provisioning Profiles"
ENABLE_MACOS_ENTITLEMENTS="${LOOPER_MACOS_ENABLE_ENTITLEMENTS:-auto}"
PROVISIONING_PROFILE="$(resolve_macos_provisioning_profile)"
CONTROL_PLANE_CRATE="crates/agent-control-plane/Cargo.toml"
ICON_SOURCE="ios/LooperCompanion/looper.icon/Assets/notification-orb.png"
MENU_BAR_PACKAGE_PATH="macos/LooperMenuBar"
MENU_BAR_EXECUTABLE="LooperMenuBar"
LOOPER_EXECUTABLE="looper"
CLI_EXECUTABLE="looper-cli"
SERVER_EXECUTABLE="looper-server"
STATUS_ICON_NAME="looper-status-icon.png"
INSTALL_PATH="/Applications/${APP_NAME}.app"
STATUS_ICON_SIZE=64
ICON_SIZES=(16 32 128 256 512)
PROCESS_WAIT_ATTEMPTS=10
PROCESS_WAIT_SECONDS=0.2
BACKUP_RETENTION_COUNT=3

fail() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

require_file() {
  [[ -f "$1" ]] || fail "missing required file: $1"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

install_app=false
if [[ "${1:-}" == "--install" ]]; then
  install_app=true
elif [[ "${1:-}" != "" && "${1:-}" != "--no-install" ]]; then
  fail "usage: $0 [--install|--no-install]"
fi

require_command cargo
require_command iconutil
require_command node
require_command plutil
require_command sips
require_command swift
require_file "$ICON_SOURCE"

version="$(node -p "JSON.parse(require('fs').readFileSync('package.json','utf8')).version")"
arch="$(uname -m)"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
package_name="${APP_NAME}-${version}-macos-${arch}-menubar-${stamp}"
package_root="${ROOT_DIR}/build/macos-package/${package_name}"
app_bundle="${package_root}/${APP_NAME}.app"
contents_dir="${app_bundle}/Contents"
macos_dir="${contents_dir}/MacOS"
resources_dir="${contents_dir}/Resources"
plist_path="${contents_dir}/Info.plist"
entitlements_path="${package_root}/${APP_NAME}.entitlements"
embedded_profile_path="${contents_dir}/embedded.provisionprofile"
iconset="${package_root}/${APP_NAME}.iconset"
zip_path="${ROOT_DIR}/artifacts/${package_name}.zip"

swift build -c release --package-path "$MENU_BAR_PACKAGE_PATH"
cargo build --release --manifest-path "$CONTROL_PLANE_CRATE" \
  --bin "$LOOPER_EXECUTABLE" \
  --bin "$CLI_EXECUTABLE" \
  --bin "$SERVER_EXECUTABLE"

mkdir -p "$macos_dir" "$resources_dir" "$iconset" artifacts
cp "${MENU_BAR_PACKAGE_PATH}/.build/release/${MENU_BAR_EXECUTABLE}" "${macos_dir}/${MENU_BAR_EXECUTABLE}"
cp "crates/agent-control-plane/target/release/${LOOPER_EXECUTABLE}" "${macos_dir}/${LOOPER_EXECUTABLE}"
cp "crates/agent-control-plane/target/release/${CLI_EXECUTABLE}" "${macos_dir}/${CLI_EXECUTABLE}"
cp "crates/agent-control-plane/target/release/${SERVER_EXECUTABLE}" "${macos_dir}/${SERVER_EXECUTABLE}"
chmod 755 \
  "${macos_dir}/${MENU_BAR_EXECUTABLE}" \
  "${macos_dir}/${LOOPER_EXECUTABLE}" \
  "${macos_dir}/${CLI_EXECUTABLE}" \
  "${macos_dir}/${SERVER_EXECUTABLE}"

sips -z "$STATUS_ICON_SIZE" "$STATUS_ICON_SIZE" "$ICON_SOURCE" --out "${resources_dir}/${STATUS_ICON_NAME}" >/dev/null
for size in "${ICON_SIZES[@]}"; do
  sips -z "$size" "$size" "$ICON_SOURCE" --out "${iconset}/icon_${size}x${size}.png" >/dev/null
  double_size=$((size * 2))
  sips -z "$double_size" "$double_size" "$ICON_SOURCE" --out "${iconset}/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "${resources_dir}/${APP_NAME}.icns"

plutil -create xml1 "$plist_path"
plutil -insert CFBundleDevelopmentRegion -string en "$plist_path"
plutil -insert CFBundleExecutable -string "$MENU_BAR_EXECUTABLE" "$plist_path"
plutil -insert CFBundleIconFile -string "$APP_NAME" "$plist_path"
plutil -insert CFBundleIdentifier -string "$BUNDLE_ID" "$plist_path"
plutil -insert CFBundleInfoDictionaryVersion -string 6.0 "$plist_path"
plutil -insert CFBundleName -string "$APP_NAME" "$plist_path"
plutil -insert CFBundleDisplayName -string "$APP_NAME" "$plist_path"
plutil -insert CFBundlePackageType -string APPL "$plist_path"
plutil -insert CFBundleShortVersionString -string "$version" "$plist_path"
plutil -insert CFBundleVersion -string "$(git rev-list --count HEAD)" "$plist_path"
plutil -insert LSMinimumSystemVersion -string 14.0 "$plist_path"
plutil -insert NSHighResolutionCapable -bool YES "$plist_path"
plutil -insert NSUserActivityTypes -array "$plist_path"
plutil -insert NSUserActivityTypes.0 -string "$CONTINUATION_ACTIVITY_TYPE" "$plist_path"

codesign_entitlements_args=()
if [[ "$CODE_SIGN_IDENTITY" != "-" && "$ENABLE_MACOS_ENTITLEMENTS" != "0" ]]; then
  if [[ -z "$PROVISIONING_PROFILE" ]]; then
    if [[ "$ENABLE_MACOS_ENTITLEMENTS" == "1" ]]; then
      fail "macOS entitlements requested but no macOS provisioning profile was found"
    fi
  else
    require_file "$PROVISIONING_PROFILE"
    cp "$PROVISIONING_PROFILE" "$embedded_profile_path"
    security cms -D -i "$PROVISIONING_PROFILE" |
      plutil -extract Entitlements xml1 -o "$entitlements_path" -
    codesign_entitlements_args=(--entitlements "$entitlements_path")
  fi
fi

codesign_with_optional_entitlements() {
  local target="$1"
  if [[ "${#codesign_entitlements_args[@]}" -gt 0 ]]; then
    codesign --force --sign "$CODE_SIGN_IDENTITY" "${codesign_entitlements_args[@]}" "$target"
  else
    codesign --force --sign "$CODE_SIGN_IDENTITY" "$target"
  fi
}

prune_install_backups() {
  local backup_prefix="${INSTALL_PATH}.backup-"
  local backups=()
  local backup
  while IFS= read -r backup; do
    backups+=("$backup")
  done < <(find "$(dirname "$INSTALL_PATH")" -maxdepth 1 -type d -name "$(basename "$INSTALL_PATH").backup-*" -print | sort)

  local excess_count=$((${#backups[@]} - BACKUP_RETENTION_COUNT))
  if [[ "$excess_count" -le 0 ]]; then
    return
  fi

  local index
  for ((index = 0; index < excess_count; index++)); do
    backup="${backups[$index]}"
    [[ "$backup" == "$backup_prefix"* ]] || fail "refusing to prune unexpected backup path: $backup"
    rm -rf "$backup"
    printf 'pruned_backup=%s\n' "$backup"
  done
}

launch_installed_app() {
  /usr/bin/open "$INSTALL_PATH"
  for _ in $(seq 1 "$PROCESS_WAIT_ATTEMPTS"); do
    if pgrep -f "${INSTALL_PATH}/Contents/MacOS/${MENU_BAR_EXECUTABLE}" >/dev/null; then
      return
    fi
    sleep "$PROCESS_WAIT_SECONDS"
  done

  fail "installed app did not launch: $INSTALL_PATH"
}

codesign --force --sign "$CODE_SIGN_IDENTITY" "${macos_dir}/${SERVER_EXECUTABLE}"
codesign --force --sign "$CODE_SIGN_IDENTITY" "${macos_dir}/${LOOPER_EXECUTABLE}"
codesign --force --sign "$CODE_SIGN_IDENTITY" "${macos_dir}/${CLI_EXECUTABLE}"
codesign_with_optional_entitlements "${macos_dir}/${MENU_BAR_EXECUTABLE}"
codesign_with_optional_entitlements "$app_bundle"

(cd "$package_root" && ditto -c -k --keepParent "${APP_NAME}.app" "$zip_path")
plutil -lint "${app_bundle}/Contents/Info.plist"
codesign --verify --deep --strict --verbose=2 "$app_bundle"
unzip -t "$zip_path" >/dev/null

if [[ "$install_app" == "true" ]]; then
  osascript -e "tell application id \"${BUNDLE_ID}\" to quit" >/dev/null 2>&1 || true
  for legacy_bundle_id in "${LEGACY_BUNDLE_IDS[@]}"; do
    osascript -e "tell application id \"${legacy_bundle_id}\" to quit" >/dev/null 2>&1 || true
  done
  for _ in $(seq 1 "$PROCESS_WAIT_ATTEMPTS"); do
    if ! pgrep -f "${INSTALL_PATH}/Contents/MacOS/${MENU_BAR_EXECUTABLE}|${INSTALL_PATH}/Contents/MacOS/${SERVER_EXECUTABLE}" >/dev/null; then
      break
    fi
    sleep "$PROCESS_WAIT_SECONDS"
  done
  pkill -f "${INSTALL_PATH}/Contents/MacOS/${MENU_BAR_EXECUTABLE}" >/dev/null 2>&1 || true
  pkill -f "${INSTALL_PATH}/Contents/MacOS/${SERVER_EXECUTABLE}" >/dev/null 2>&1 || true
  if [[ -d "$INSTALL_PATH" ]]; then
    mv "$INSTALL_PATH" "${INSTALL_PATH}.backup-${stamp}"
  fi
  ditto "$app_bundle" "$INSTALL_PATH"
  plutil -lint "${INSTALL_PATH}/Contents/Info.plist"
  codesign --verify --deep --strict --verbose=2 "$INSTALL_PATH"
  prune_install_backups
  launch_installed_app
fi

printf 'app=%s\nzip=%s\n' "$app_bundle" "$zip_path"
