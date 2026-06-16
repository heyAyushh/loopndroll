#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IOS_DIR="${ROOT_DIR}/ios"
PROOF_DIR="${ROOT_DIR}/.build/siri-runtime"
SIM_DERIVED_DATA_DIR="${PROOF_DIR}/xcode-simulator"
DEVICE_DERIVED_DATA_DIR="${PROOF_DIR}/xcode-device"
SIM_DEVICES_JSON="${PROOF_DIR}/simulators.json"
CORE_DEVICES_JSON="${PROOF_DIR}/coredevices.json"
DEVICE_DDI_SERVICES_JSON="${PROOF_DIR}/device-ddi-services.json"
DEVICE_DDI_SERVICES_LOG="${PROOF_DIR}/device-ddi-services.log"
DEVICE_INSTALL_JSON="${PROOF_DIR}/device-install.json"
DEVICE_LAUNCH_JSON="${PROOF_DIR}/device-launch.json"
DEVICE_LAUNCH_LOG="${PROOF_DIR}/device-launch.log"
BUNDLE_IDENTIFIER="dev.looper.app.ios"
SIMULATOR_APP="${SIM_DERIVED_DATA_DIR}/Build/Products/Debug-iphonesimulator/Looper.app"
DEVICE_APP="${DEVICE_DERIVED_DATA_DIR}/Build/Products/Debug-iphoneos/Looper.app"
APP_INTENTS_METADATA="Metadata.appintents/extract.actionsdata"
REQUIRED_METADATA=(
  'SearchLooperSessionsIntent'
  'AskContextualCurrentLooperSessionIntent'
  'AskCurrentLooperSessionIntent'
  'AskDefaultLooperSessionIntent'
  'SuggestLooperPromptIntent'
  'SetDefaultLooperSessionIntent'
  'LooperSessionEntity'
  'LooperSessionValueQuery'
  'Assistant Surface'
  'Project Path'
)
REMOVED_METADATA=(
  'AskLatestCodexSessionIntent'
)

require_command() {
  local command_name="$1"

  command -v "${command_name}" >/dev/null 2>&1 || {
    printf 'error: missing required command: %s\n' "${command_name}" >&2
    exit 1
  }
}

supports_generic_ios_simulator() {
  local developer_dir="$1"

  DEVELOPER_DIR="${developer_dir}" xcodebuild \
    -project "${IOS_DIR}/LooperCompanion.xcodeproj" \
    -scheme LooperCompanion \
    -configuration Debug \
    -sdk iphonesimulator \
    -showdestinations 2>/dev/null |
    grep -q 'Any iOS Simulator Device'
}

select_developer_dir() {
  local active_developer_dir
  active_developer_dir="$(xcode-select -p)"

  if supports_generic_ios_simulator "${active_developer_dir}"; then
    printf '%s\n' "${active_developer_dir}"
    return
  fi

  if [ -d '/Applications/Xcode-beta.app' ] &&
    supports_generic_ios_simulator '/Applications/Xcode-beta.app'; then
    printf '%s\n' '/Applications/Xcode-beta.app'
    return
  fi

  printf '%s\n' "${active_developer_dir}"
}

require_metadata() {
  local app_bundle="$1"
  local metadata_path="${app_bundle}/${APP_INTENTS_METADATA}"
  local required_value
  local removed_value

  if [ ! -s "${metadata_path}" ]; then
    printf 'error: missing App Intents metadata at %s\n' "${metadata_path}" >&2
    exit 1
  fi

  for required_value in "${REQUIRED_METADATA[@]}"; do
    if ! strings "${metadata_path}" | grep -q "${required_value}"; then
      printf 'error: App Intents metadata missing %s\n' "${required_value}" >&2
      exit 1
    fi
  done

  for removed_value in "${REMOVED_METADATA[@]}"; do
    if strings "${metadata_path}" | grep -q "${removed_value}"; then
      printf 'error: stale App Intents metadata still contains %s\n' "${removed_value}" >&2
      exit 1
    fi
  done
}

select_simulator() {
  /usr/bin/python3 - "${SIM_DEVICES_JSON}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

available_iphones = []
for runtime_devices in payload.get("devices", {}).values():
    for device in runtime_devices:
        if not device.get("isAvailable", False):
            continue
        if "iPhone" not in device.get("name", ""):
            continue
        available_iphones.append(device)

booted = next((device for device in available_iphones if device.get("state") == "Booted"), None)
selected = booted or (available_iphones[0] if available_iphones else None)
if not selected:
    raise SystemExit("error: no available iPhone simulator")

print(f"{selected['udid']}\t{selected['name']}\t{selected.get('state', '')}")
PY
}

select_physical_iphone() {
  /usr/bin/python3 - "${CORE_DEVICES_JSON}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

unavailable_device = None
for device in payload.get("result", {}).get("devices", []):
    hardware = device.get("hardwareProperties", {})
    properties = device.get("deviceProperties", {})
    connection = device.get("connectionProperties", {})
    if hardware.get("platform") != "iOS":
        continue
    if hardware.get("deviceType") != "iPhone":
        continue
    if hardware.get("reality") != "physical":
        continue

    reasons = []
    if connection.get("pairingState") != "paired":
        reasons.append("not paired")
    if properties.get("developerModeStatus") != "enabled":
        reasons.append("Developer Mode disabled")
    if properties.get("ddiServicesAvailable") is not True:
        reasons.append("developer disk services unavailable")
    if connection.get("tunnelState") != "connected":
        reasons.append("device tunnel unavailable")

    identifier = device.get("identifier")
    name = properties.get("name") or "iPhone"
    if identifier and not reasons:
        print(f"AVAILABLE\t{identifier}\t{name}")
        raise SystemExit(0)

    if unavailable_device is None:
        unavailable_device = (identifier or "", name, "; ".join(reasons) or "not reachable")

if unavailable_device:
    print(f"UNAVAILABLE\t{unavailable_device[0]}\t{unavailable_device[1]}\t{unavailable_device[2]}")

raise SystemExit(0)
PY
}

build_simulator_app() {
  xcodebuild -quiet \
    -project "${IOS_DIR}/LooperCompanion.xcodeproj" \
    -scheme LooperCompanion \
    -configuration Debug \
    -sdk iphonesimulator \
    -destination 'generic/platform=iOS Simulator' \
    -derivedDataPath "${SIM_DERIVED_DATA_DIR}" \
    CODE_SIGNING_ALLOWED=NO \
    ARCHS=arm64 \
    ONLY_ACTIVE_ARCH=NO \
    build
}

install_and_launch_simulator_app() {
  local simulator_selection="$1"
  local simulator_udid
  local simulator_name
  local simulator_state
  local installed_app

  IFS=$'\t' read -r simulator_udid simulator_name simulator_state <<< "${simulator_selection}"

  if [ "${simulator_state}" != "Booted" ]; then
    xcrun simctl boot "${simulator_udid}" || true
  fi
  xcrun simctl bootstatus "${simulator_udid}" -b
  xcrun simctl install "${simulator_udid}" "${SIMULATOR_APP}"
  xcrun simctl launch "${simulator_udid}" "${BUNDLE_IDENTIFIER}" >/dev/null

  installed_app="$(xcrun simctl get_app_container "${simulator_udid}" "${BUNDLE_IDENTIFIER}" app)"
  require_metadata "${installed_app}"
  printf 'simulator: installed and launched %s (%s)\n' "${simulator_name}" "${simulator_udid}"
  printf 'simulator: bundle identifier %s\n' "${BUNDLE_IDENTIFIER}"
  printf 'simulator: app path %s\n' "${installed_app}"
}

build_device_app() {
  local device_identifier="$1"

  xcodebuild -quiet \
    -project "${IOS_DIR}/LooperCompanion.xcodeproj" \
    -scheme LooperCompanion \
    -configuration Debug \
    -sdk iphoneos \
    -destination "platform=iOS,id=${device_identifier}" \
    -derivedDataPath "${DEVICE_DERIVED_DATA_DIR}" \
    -allowProvisioningUpdates \
    build
}

install_and_launch_device_app() {
  local device_identifier="$1"
  local device_name="$2"

  require_metadata "${DEVICE_APP}"
  xcrun devicectl device install app \
    --device "${device_identifier}" \
    --timeout 90 \
    --json-output "${DEVICE_INSTALL_JSON}" \
    "${DEVICE_APP}" >/dev/null
  if ! xcrun devicectl device process launch \
    --device "${device_identifier}" \
    --terminate-existing \
    --timeout 45 \
    --json-output "${DEVICE_LAUNCH_JSON}" \
    "${BUNDLE_IDENTIFIER}" >"${DEVICE_LAUNCH_LOG}" 2>&1; then
    printf 'physical: installed %s (%s), launch unavailable\n' "${device_name}" "${device_identifier}"
    if [ -s "${DEVICE_LAUNCH_LOG}" ]; then
      sed 's/^/physical: launch: /' "${DEVICE_LAUNCH_LOG}"
    fi
    if is_locked_device_launch_blocker; then
      return
    fi
    printf 'error: physical launch failed for an unexpected reason\n' >&2
    return 1
  fi

  printf 'physical: installed and launched %s (%s)\n' "${device_name}" "${device_identifier}"
  printf 'physical: bundle identifier %s\n' "${BUNDLE_IDENTIFIER}"
  printf 'physical: app path %s\n' "${DEVICE_APP}"
}

is_locked_device_launch_blocker() {
  if [ ! -s "${DEVICE_LAUNCH_LOG}" ]; then
    return 1
  fi

  grep -q 'RequestDenied' "${DEVICE_LAUNCH_LOG}" &&
    grep -q 'Locked' "${DEVICE_LAUNCH_LOG}" &&
    grep -q 'device was not, or could not be, unlocked' "${DEVICE_LAUNCH_LOG}"
}

record_unavailable_physical_device() {
  local device_identifier="$1"
  local device_name="$2"
  local unavailable_reason="$3"

  printf 'physical: %s (%s) unavailable: %s\n' \
    "${device_name}" \
    "${device_identifier}" \
    "${unavailable_reason}"
  xcrun devicectl device info ddiServices \
    --device "${device_identifier}" \
    --timeout 30 \
    --json-output "${DEVICE_DDI_SERVICES_JSON}" \
    >"${DEVICE_DDI_SERVICES_LOG}" 2>&1 || true
  if [ -s "${DEVICE_DDI_SERVICES_LOG}" ]; then
    sed 's/^/physical: ddiServices: /' "${DEVICE_DDI_SERVICES_LOG}"
  fi
}

mkdir -p "${PROOF_DIR}"
require_command xcodebuild
require_command xcodegen
require_command xcrun
require_command strings

xcodegen generate --spec "${IOS_DIR}/project.yml" --project "${IOS_DIR}" >/dev/null

export DEVELOPER_DIR
DEVELOPER_DIR="$(select_developer_dir)"
printf 'Using DEVELOPER_DIR=%s\n' "${DEVELOPER_DIR}"

printf 'building simulator app...\n'
build_simulator_app
require_metadata "${SIMULATOR_APP}"
printf 'metadata: simulator App Intents verified\n'

xcrun simctl list devices available -j > "${SIM_DEVICES_JSON}"
install_and_launch_simulator_app "$(select_simulator)"

if xcrun -f devicectl >/dev/null 2>&1; then
  xcrun devicectl list devices \
    --search iPhone \
    --timeout 15 \
    --json-output "${CORE_DEVICES_JSON}" >/dev/null
  physical_selection="$(select_physical_iphone)"
  if [ -n "${physical_selection}" ]; then
    IFS=$'\t' read -r physical_state physical_first physical_second physical_third <<< "${physical_selection}"
    if [ "${physical_state}" = "AVAILABLE" ]; then
      printf 'building physical-device app for %s...\n' "${physical_second}"
      build_device_app "${physical_first}"
      install_and_launch_device_app "${physical_first}" "${physical_second}"
    else
      record_unavailable_physical_device "${physical_first}" "${physical_second}" "${physical_third}"
    fi
  else
    printf 'physical: no iPhone reported by devicectl\n'
  fi
else
  printf 'physical: devicectl unavailable\n'
fi

printf 'proof: complete\n'
