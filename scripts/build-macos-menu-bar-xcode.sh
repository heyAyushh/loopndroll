#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_DIR="${ROOT_DIR}/macos/LooperMenuBar"
PROJECT_PATH="${PROJECT_DIR}/LooperMenuBar.xcodeproj"
DERIVED_DATA="${LOOPER_MACOS_XCODE_DERIVED_DATA:-/tmp/looper-macos-menubar-xcode}"
SCHEME="LooperMenuBar"
CONFIGURATION="${LOOPER_MACOS_XCODE_CONFIGURATION:-Release}"
APP_NAME="looper"
BUILT_APP_NAME="LooperMenuBar"
INSTALL_PATH="/Applications/${APP_NAME}.app"
PROCESS_WAIT_ATTEMPTS=10
PROCESS_WAIT_SECONDS=0.2

install_app=false
if [[ "${1:-}" == "--install" ]]; then
  install_app=true
elif [[ "${1:-}" != "" && "${1:-}" != "--no-install" ]]; then
  printf 'usage: %s [--install|--no-install]\n' "$0" >&2
  exit 1
fi

command -v xcodegen >/dev/null 2>&1 || {
  printf 'error: missing required command: xcodegen\n' >&2
  exit 1
}

launch_installed_app() {
  /usr/bin/open -g "$INSTALL_PATH"
  for _ in $(seq 1 "$PROCESS_WAIT_ATTEMPTS"); do
    if pgrep -f "${INSTALL_PATH}/Contents/MacOS/LooperMenuBar" >/dev/null; then
      return
    fi
    sleep "$PROCESS_WAIT_SECONDS"
  done

  printf 'error: installed app did not launch: %s\n' "$INSTALL_PATH" >&2
  exit 1
}

xcodegen generate --spec "${PROJECT_DIR}/project.yml" --project "${PROJECT_DIR}"

xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme "$SCHEME" \
  -configuration "$CONFIGURATION" \
  -destination 'platform=macOS' \
  -derivedDataPath "$DERIVED_DATA" \
  -allowProvisioningUpdates \
  -allowProvisioningDeviceRegistration \
  build

app_path="${DERIVED_DATA}/Build/Products/${CONFIGURATION}/${BUILT_APP_NAME}.app"
[[ -d "$app_path" ]] || {
  printf 'error: built app not found: %s\n' "$app_path" >&2
  exit 1
}

if [[ "$install_app" == "true" ]]; then
  osascript -e 'tell application id "dev.looper.app.menubar" to quit' >/dev/null 2>&1 || true
  osascript -e 'tell application id "dev.looper.app.ios" to quit' >/dev/null 2>&1 || true
  for _ in $(seq 1 "$PROCESS_WAIT_ATTEMPTS"); do
    if ! pgrep -f "${INSTALL_PATH}/Contents/MacOS/LooperMenuBar|${INSTALL_PATH}/Contents/MacOS/looper-server" >/dev/null; then
      break
    fi
    sleep "$PROCESS_WAIT_SECONDS"
  done
  pkill -f "${INSTALL_PATH}/Contents/MacOS/LooperMenuBar" >/dev/null 2>&1 || true
  pkill -f "${INSTALL_PATH}/Contents/MacOS/looper-server" >/dev/null 2>&1 || true
  if [[ -d "$INSTALL_PATH" ]]; then
    stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    mv "$INSTALL_PATH" "${INSTALL_PATH}.backup-${stamp}"
  fi
  ditto "$app_path" "$INSTALL_PATH"
  codesign --verify --deep --strict --verbose=2 "$INSTALL_PATH"
  launch_installed_app
fi

printf 'app=%s\n' "$app_path"
