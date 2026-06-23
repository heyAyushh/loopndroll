#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IOS_DIR="${ROOT_DIR}/ios"
PROOF_DIR="${ROOT_DIR}/.build/siri-runtime"
SURFACE_DIR="${PROOF_DIR}/surface"
SIM_DERIVED_DATA_DIR="${PROOF_DIR}/xcode-simulator"
DEVICE_DERIVED_DATA_DIR="${PROOF_DIR}/xcode-device"
SIM_DEVICES_JSON="${PROOF_DIR}/simulators.json"
CORE_DEVICES_JSON="${PROOF_DIR}/coredevices.json"
CORE_DEVICES_STDERR="${PROOF_DIR}/coredevices.stderr"
DEVICE_DDI_SERVICES_JSON="${PROOF_DIR}/device-ddi-services.json"
DEVICE_DDI_SERVICES_LOG="${PROOF_DIR}/device-ddi-services.log"
DEVICE_INSTALL_JSON="${PROOF_DIR}/device-install.json"
DEVICE_LAUNCH_JSON="${PROOF_DIR}/device-launch.json"
DEVICE_LAUNCH_LOG="${PROOF_DIR}/device-launch.log"
SURFACE_ENVIRONMENT="${SURFACE_DIR}/environment.txt"
SURFACE_INSTALLED_METADATA="${SURFACE_DIR}/installed-metadata.txt"
SIRI_LINKD_INDEXING_LOG="${SURFACE_DIR}/siri-linkd-indexing.log"
SIRI_LINKD_INDEXING_ERR="${SURFACE_DIR}/siri-linkd-indexing.err"
SIRI_LAUNCH_STDOUT="${SURFACE_DIR}/launch-siri.out"
SIRI_LAUNCH_STDERR="${SURFACE_DIR}/launch-siri.err"
SIRI_OPENURL_STDOUT="${SURFACE_DIR}/openurl-siri.out"
SIRI_OPENURL_STDERR="${SURFACE_DIR}/openurl-siri.err"
SIRI_SCREENSHOT="${SURFACE_DIR}/siri-surface.png"
BUNDLE_IDENTIFIER="dev.looper.app.ios"
SIRI_BUNDLE_IDENTIFIER="com.apple.siri"
SIRI_URL="siri://"
SIRI_INDEXING_LOG_LOOKBACK="10m"
SIRI_LAUNCH_TIMEOUT_SECONDS=15
SIMULATOR_INSTALL_TIMEOUT_SECONDS=60
SIMULATOR_APP_LAUNCH_TIMEOUT_SECONDS=20
PREFERRED_SIMULATOR_NAME="Looper Siri iOS27b2"
SIMULATOR_APP="${SIM_DERIVED_DATA_DIR}/Build/Products/Debug-iphonesimulator/Looper.app"
DEVICE_APP="${DEVICE_DERIVED_DATA_DIR}/Build/Products/Debug-iphoneos/Looper.app"
APP_INTENTS_METADATA="Metadata.appintents/extract.actionsdata"
RUN_PHYSICAL_DEVICE_PROOF=1
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

for argument in "$@"; do
  case "${argument}" in
    --simulator-only)
      RUN_PHYSICAL_DEVICE_PROOF=0
      ;;
    *)
      printf 'error: unknown argument: %s\n' "${argument}" >&2
      exit 1
      ;;
  esac
done

require_command() {
  local command_name="$1"

  command -v "${command_name}" >/dev/null 2>&1 || {
    printf 'error: missing required command: %s\n' "${command_name}" >&2
    exit 1
  }
}

run_with_timeout() {
  local timeout_seconds="$2"
  shift 2

  python3 - "${timeout_seconds}" "$@" <<'PY'
import subprocess
import sys

timeout_seconds = float(sys.argv[1])
command = sys.argv[2:]

try:
    completed_process = subprocess.run(command, timeout=timeout_seconds)
except subprocess.TimeoutExpired:
    raise SystemExit(124)

raise SystemExit(completed_process.returncode)
PY
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
  /usr/bin/python3 - "${SIM_DEVICES_JSON}" "${PREFERRED_SIMULATOR_NAME}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

available_devices = []
for runtime_name, runtime_devices in payload.get("devices", {}).items():
    if "iOS-27-0" not in runtime_name and "iOS 27.0" not in runtime_name:
        continue
    for device in runtime_devices:
        if not device.get("isAvailable", False):
            continue
        available_devices.append(device)

available_iphones = []
for device in available_devices:
    if "iPhone" not in device.get("name", ""):
        continue
    available_iphones.append(device)

preferred_name = sys.argv[2]
preferred = next((device for device in available_devices if device.get("name") == preferred_name), None)
booted = next((device for device in available_iphones if device.get("state") == "Booted"), None)
selected = preferred or booted or (available_iphones[0] if available_iphones else None)
if not selected:
    raise SystemExit("error: no available iOS 27 simulator")

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
  DEVELOPER_DIR="${DEVELOPER_DIR}" run_with_timeout \
    "Looper simulator install" \
    "${SIMULATOR_INSTALL_TIMEOUT_SECONDS}" \
    xcrun simctl install "${simulator_udid}" "${SIMULATOR_APP}"
  DEVELOPER_DIR="${DEVELOPER_DIR}" run_with_timeout \
    "Looper simulator launch" \
    "${SIMULATOR_APP_LAUNCH_TIMEOUT_SECONDS}" \
    xcrun simctl launch "${simulator_udid}" "${BUNDLE_IDENTIFIER}" >/dev/null

  installed_app="$(xcrun simctl get_app_container "${simulator_udid}" "${BUNDLE_IDENTIFIER}" app)"
  require_metadata "${installed_app}"
  capture_simulator_siri_surface "${simulator_udid}" "${installed_app}"
  printf 'simulator: installed and launched %s (%s)\n' "${simulator_name}" "${simulator_udid}"
  printf 'simulator: bundle identifier %s\n' "${BUNDLE_IDENTIFIER}"
  printf 'simulator: app path %s\n' "${installed_app}"
}

capture_simulator_siri_surface() {
  local simulator_udid="$1"
  local installed_app="$2"
  local siri_surface_opened=0

  mkdir -p "${SURFACE_DIR}"
  {
    printf 'device=%s\n' "${simulator_udid}"
    printf 'xcode-select=%s\n' "$(xcode-select -p)"
    DEVELOPER_DIR="${DEVELOPER_DIR}" xcodebuild -version
    DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun simctl runtime list | grep 'iOS 27.0' || true
    DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun simctl list devices available | grep "${simulator_udid}" || true
    printf 'looper_app=%s\n' "${installed_app}"
  } >"${SURFACE_ENVIRONMENT}"

  strings "${installed_app}/${APP_INTENTS_METADATA}" >"${SURFACE_INSTALLED_METADATA}"

  if launch_siri_with_timeout "${simulator_udid}"; then
    printf 'surface: Siri launch command completed\n'
    siri_surface_opened=1
  else
    printf 'surface: Siri launch command timed out; trying URL fallback\n'
    if DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun simctl openurl "${simulator_udid}" "${SIRI_URL}" \
      >"${SIRI_OPENURL_STDOUT}" 2>"${SIRI_OPENURL_STDERR}"; then
      siri_surface_opened=1
    fi
  fi

  if [ "${siri_surface_opened}" != "1" ]; then
    printf 'error: unable to open Siri surface in simulator\n' >&2
    return 1
  fi

  DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun simctl io "${simulator_udid}" screenshot "${SIRI_SCREENSHOT}" >/dev/null

  DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun simctl spawn "${simulator_udid}" log show \
    --last "${SIRI_INDEXING_LOG_LOOKBACK}" \
    --style compact \
    --predicate "process == \"linkd\" OR process == \"siriactionsd\" OR eventMessage CONTAINS \"${BUNDLE_IDENTIFIER}\"" \
    >"${SIRI_LINKD_INDEXING_LOG}" 2>"${SIRI_LINKD_INDEXING_ERR}" || true

  if grep -q "${BUNDLE_IDENTIFIER}" "${SIRI_LINKD_INDEXING_LOG}"; then
    printf 'surface: Siri indexing log mentions %s\n' "${BUNDLE_IDENTIFIER}"
  else
    printf 'surface: Siri indexing log captured without bundle mention\n'
  fi

  printf 'surface: Siri app launched as %s\n' "${SIRI_BUNDLE_IDENTIFIER}"
  printf 'surface: artifacts %s\n' "${SURFACE_DIR}"
}

launch_siri_with_timeout() {
  local simulator_udid="$1"

  DEVELOPER_DIR="${DEVELOPER_DIR}" run_with_timeout \
    "Siri simulator launch" \
    "${SIRI_LAUNCH_TIMEOUT_SECONDS}" \
    xcrun simctl launch "${simulator_udid}" "${SIRI_BUNDLE_IDENTIFIER}" \
    >"${SIRI_LAUNCH_STDOUT}" 2>"${SIRI_LAUNCH_STDERR}"
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
require_command python3
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

if [ "${RUN_PHYSICAL_DEVICE_PROOF}" = "0" ]; then
  printf 'physical: skipped (--simulator-only)\n'
elif xcrun -f devicectl >/dev/null 2>&1; then
  xcrun devicectl list devices \
    --search iPhone \
    --timeout 15 \
    --json-output - >"${CORE_DEVICES_JSON}" 2>"${CORE_DEVICES_STDERR}"
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
