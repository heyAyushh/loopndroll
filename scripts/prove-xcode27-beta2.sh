#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROOF_DIR="${ROOT_DIR}/.build/xcode27-beta2-proof"
RELEASE_NOTES_JSON="${PROOF_DIR}/xcode-27-release-notes.json"
RELEASE_NOTES_MD="${PROOF_DIR}/xcode-27-release-notes.md"
REPORT_PATH="${PROOF_DIR}/report.txt"
DEVICECTL_JSON_STDOUT="${PROOF_DIR}/devicectl-json-stdout.json"
SIMULATORS_JSON="${PROOF_DIR}/simulators.json"
SIMULATORS_SUMMARY="${PROOF_DIR}/ios27-simulators.txt"
SIRI_RUNTIME_LOG="${PROOF_DIR}/prove-ios-siri-runtime.log"
SIRI_RUNTIME_SURFACE_DIR="${ROOT_DIR}/.build/siri-runtime/surface"
SIRI_LINKD_INDEXING_LOG="${SIRI_RUNTIME_SURFACE_DIR}/siri-linkd-indexing.log"
SIRI_SURFACE_SCREENSHOT="${SIRI_RUNTIME_SURFACE_DIR}/siri-surface.png"
SIRI_AI_CAPABILITY_REPORT="${ROOT_DIR}/.build/siri-ai-capabilities/report.txt"
SIRI_AI_CAPABILITY_PROOF="${PROOF_DIR}/siri-ai-capabilities.txt"
DEVICE_HUB_PROOF="${PROOF_DIR}/device-hub.txt"
RELEASE_NOTES_JSON_URL="https://developer.apple.com/tutorials/data/documentation/xcode-release-notes/xcode-27-release-notes.json"
RELEASE_NOTES_MARKDOWN_URL="https://docs.developer.apple.com/tutorials/data/documentation/xcode-release-notes/xcode-27-release-notes.md"
EXPECTED_RELEASE_NOTES_TITLE="Xcode 27 Beta 2 Release Notes"
EXPECTED_XCODE_MAJOR="Xcode 27.0"
EXPECTED_IOS_RUNTIME="iOS 27.0"
RUN_IOS_RUNTIME_PROOF=1
REQUIRE_LLDB_MCP=0

for argument in "$@"; do
  case "${argument}" in
    --skip-ios-runtime-proof)
      RUN_IOS_RUNTIME_PROOF=0
      ;;
    --require-lldb-mcp)
      REQUIRE_LLDB_MCP=1
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

record_pass() {
  printf 'PASS %s\n' "$*" | tee -a "${REPORT_PATH}"
}

record_warn() {
  printf 'WARN %s\n' "$*" | tee -a "${REPORT_PATH}"
}

record_fail() {
  printf 'FAIL %s\n' "$*" | tee -a "${REPORT_PATH}" >&2
  exit 1
}

supports_generic_ios_simulator() {
  local developer_dir="$1"

  DEVELOPER_DIR="${developer_dir}" xcodebuild \
    -project "${ROOT_DIR}/ios/LooperCompanion.xcodeproj" \
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
    supports_generic_ios_simulator '/Applications/Xcode-beta.app/Contents/Developer'; then
    printf '%s\n' '/Applications/Xcode-beta.app/Contents/Developer'
    return
  fi

  printf '%s\n' "${active_developer_dir}"
}

fetch_release_notes() {
  curl -L --max-time 30 -s "${RELEASE_NOTES_JSON_URL}" -o "${RELEASE_NOTES_JSON}"
  curl -L --max-time 30 -s "${RELEASE_NOTES_MARKDOWN_URL}" -o "${RELEASE_NOTES_MD}"
}

verify_release_notes() {
  python3 - "${RELEASE_NOTES_JSON}" "${RELEASE_NOTES_MD}" "${EXPECTED_RELEASE_NOTES_TITLE}" <<'PY'
import json
import pathlib
import sys

json_path = pathlib.Path(sys.argv[1])
markdown_path = pathlib.Path(sys.argv[2])
expected_title = sys.argv[3]

payload = json.loads(json_path.read_text())
actual_title = payload.get("metadata", {}).get("title")
if actual_title != expected_title:
    raise SystemExit(f"release notes title mismatch: {actual_title!r}")

markdown = markdown_path.read_text(errors="replace")
required_headings = [
    "### App Intents",
    "### Core AI",
    "### Debugging",
    "### Device Hub",
    "### devicectl",
    "### Simulator",
    "### Testing",
    "### Xcode",
]
missing = [heading for heading in required_headings if heading not in markdown]
if missing:
    raise SystemExit(f"release notes missing headings: {', '.join(missing)}")

print(actual_title)
PY
  record_pass "official release notes fetched and matched ${EXPECTED_RELEASE_NOTES_TITLE}"
}

record_xcode_environment() {
  local developer_dir="$1"
  local xcode_version
  local runtime_list

  xcode_version="$(DEVELOPER_DIR="${developer_dir}" xcodebuild -version)"
  runtime_list="$(DEVELOPER_DIR="${developer_dir}" xcrun simctl runtime list)"
  {
    printf 'developer_dir=%s\n' "${developer_dir}"
    printf '%s\n' "${xcode_version}"
    printf '%s\n' "${runtime_list}" | grep "${EXPECTED_IOS_RUNTIME}" || true
  } >>"${REPORT_PATH}"

  if ! printf '%s\n' "${xcode_version}" | grep -q "${EXPECTED_XCODE_MAJOR}"; then
    record_fail "selected Xcode is not ${EXPECTED_XCODE_MAJOR}"
  fi

  if ! printf '%s\n' "${runtime_list}" | grep -q "${EXPECTED_IOS_RUNTIME}"; then
    record_fail "missing ${EXPECTED_IOS_RUNTIME} simulator runtime"
  fi

  record_pass "selected Xcode 27 and iOS 27 simulator runtime are available"
}

verify_devicectl_json_stdout() {
  DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun devicectl list devices \
    --timeout 10 \
    --json-output - >"${DEVICECTL_JSON_STDOUT}" 2>"${PROOF_DIR}/devicectl-json-stdout.stderr"

  python3 - "${DEVICECTL_JSON_STDOUT}" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
if payload.get("info", {}).get("outcome") != "success":
    raise SystemExit("devicectl JSON stdout did not report success")
if "devices" not in payload.get("result", {}):
    raise SystemExit("devicectl JSON stdout missing result.devices")
print(len(payload["result"]["devices"]))
PY
  record_pass "devicectl --json-output - produced parseable stdout JSON"
}

verify_lldb_mcp() {
  local lldb_mcp_path

  if lldb_mcp_path="$(DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun --find lldb-mcp 2>/dev/null)" &&
    [ -n "${lldb_mcp_path}" ]; then
    printf 'lldb_mcp=%s\n' "${lldb_mcp_path}" >>"${REPORT_PATH}"
    record_pass "lldb-mcp is available"
    return
  fi

  if [ "${REQUIRE_LLDB_MCP}" = "1" ]; then
    record_fail "lldb-mcp is missing from selected Xcode; install the beta 2 Xcode app before requiring this"
  fi

  record_warn "lldb-mcp is missing from selected Xcode; current local Xcode is not the beta 2 app build"
}

verify_device_hub_app() {
  local device_hub_app="${DEVELOPER_DIR}/../Applications/DeviceHub.app"
  local device_hub_executable="${device_hub_app}/Contents/MacOS/DeviceHub"
  local device_hub_bundle_id=""

  if [ ! -x "${device_hub_executable}" ]; then
    record_fail "Device Hub app is missing from selected Xcode"
  fi

  if [ -x /usr/libexec/PlistBuddy ]; then
    device_hub_bundle_id="$(
      /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
        "${device_hub_app}/Contents/Info.plist" 2>/dev/null || true
    )"
  fi

  {
    printf 'device_hub_app=%s\n' "${device_hub_app}"
    printf 'device_hub_executable=%s\n' "${device_hub_executable}"
    printf 'device_hub_bundle_id=%s\n' "${device_hub_bundle_id}"
  } >"${DEVICE_HUB_PROOF}"

  record_pass "Device Hub app is present in selected Xcode"
}

verify_siri_ai_capability_guard() {
  DEVELOPER_DIR="${DEVELOPER_DIR}" bash "${ROOT_DIR}/scripts/audit-siri-ai-capabilities.sh" >/dev/null
  cp "${SIRI_AI_CAPABILITY_REPORT}" "${SIRI_AI_CAPABILITY_PROOF}"

  if grep -q '^importable FoundationModels$' "${SIRI_AI_CAPABILITY_PROOF}"; then
    record_pass "FoundationModels import guard is backed by the selected SDK"
  else
    record_warn "FoundationModels is unavailable; guarded fallback path remains required"
  fi

  if grep -q '^present SpotlightSearchTool$' "${SIRI_AI_CAPABILITY_PROOF}"; then
    record_pass "SpotlightSearchTool capability is recorded for guarded Siri context"
  else
    record_warn "SpotlightSearchTool is unavailable; guarded no-op path remains required"
  fi
}

verify_simulator_json() {
  DEVELOPER_DIR="${DEVELOPER_DIR}" xcrun simctl list devices available -j >"${SIMULATORS_JSON}"
  python3 - "${SIMULATORS_JSON}" >"${SIMULATORS_SUMMARY}" <<'PY'
import json
import sys

payload = json.load(open(sys.argv[1], encoding="utf-8"))
available_ios_27 = []
for runtime, devices in payload.get("devices", {}).items():
    if "iOS-27-0" not in runtime and "iOS 27.0" not in runtime:
        continue
    for device in devices:
        if device.get("isAvailable") and "iPhone" in device.get("name", ""):
            available_ios_27.append(f"{device['name']}:{device['udid']}:{device.get('state')}")
if not available_ios_27:
    raise SystemExit("no available iPhone simulator on iOS 27")
print("\n".join(available_ios_27))
PY
  cat "${SIMULATORS_SUMMARY}"
  record_pass "simctl JSON reports available iOS 27 iPhone simulators"
}

verify_ios_siri_runtime() {
  if [ "${RUN_IOS_RUNTIME_PROOF}" = "0" ]; then
    record_warn "iOS Siri runtime proof skipped"
    return
  fi

  PATH="/opt/homebrew/bin:${PATH}" DEVELOPER_DIR="${DEVELOPER_DIR}" \
    bash "${ROOT_DIR}/scripts/prove-ios-siri-runtime.sh" --simulator-only \
    >"${SIRI_RUNTIME_LOG}" 2>&1

  if ! grep -q 'proof: complete' "${SIRI_RUNTIME_LOG}"; then
    record_fail "simulator Siri runtime proof did not complete"
  fi

  if ! grep -q 'metadata: simulator App Intents verified' "${SIRI_RUNTIME_LOG}"; then
    record_fail "simulator App Intents metadata was not verified"
  fi

  if ! grep -q 'surface: Siri app launched as com.apple.siri' "${SIRI_RUNTIME_LOG}"; then
    record_fail "simulator Siri app surface was not launched"
  fi

  if [ ! -s "${SIRI_SURFACE_SCREENSHOT}" ]; then
    record_fail "simulator Siri surface screenshot was not captured"
  fi

  if grep -q 'Completed AppShortcut interpolation' "${SIRI_LINKD_INDEXING_LOG}" &&
    grep -q 'FullSetDonation' "${SIRI_LINKD_INDEXING_LOG}"; then
    record_pass "Siri indexing log shows AppShortcut interpolation and donation"
  else
    record_warn "Siri indexing log did not include full AppShortcut interpolation evidence"
  fi

  record_pass "simulator Siri runtime proof completed"
}

mkdir -p "${PROOF_DIR}"
: >"${REPORT_PATH}"
require_command curl
require_command python3
require_command xcodebuild
require_command xcrun
require_command grep
require_command tee

DEVELOPER_DIR="$(select_developer_dir)"
export DEVELOPER_DIR

fetch_release_notes
verify_release_notes
record_xcode_environment "${DEVELOPER_DIR}"
verify_devicectl_json_stdout
verify_lldb_mcp
verify_device_hub_app
verify_siri_ai_capability_guard
verify_simulator_json
verify_ios_siri_runtime

record_pass "xcode27 beta2 proof complete"
