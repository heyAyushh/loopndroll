#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPORT_DIR="${ROOT_DIR}/.build/siri-ai-capabilities"
REPORT_PATH="${REPORT_DIR}/report.txt"

if [ -z "${DEVELOPER_DIR:-}" ] && [ -d '/Applications/Xcode-beta.app' ]; then
  export DEVELOPER_DIR='/Applications/Xcode-beta.app'
fi

SDKROOT="$(xcrun --sdk iphonesimulator --show-sdk-path)"
APPINTENTS_IFACE="${SDKROOT}/System/Library/Frameworks/AppIntents.framework/Modules/AppIntents.swiftmodule/arm64-apple-ios-simulator.swiftinterface"
SWIFTUI_IFACE="${SDKROOT}/System/Library/Frameworks/SwiftUI.framework/Modules/SwiftUI.swiftmodule/arm64-apple-ios-simulator.swiftinterface"
FOUNDATION_MODELS_IFACE="${SDKROOT}/System/Library/Frameworks/FoundationModels.framework/Modules/FoundationModels.swiftmodule/arm64-apple-ios-simulator.swiftinterface"
CORESPOTLIGHT_FM_IFACE="${SDKROOT}/System/Library/Frameworks/_CoreSpotlight_FoundationModels.framework/Modules/_CoreSpotlight_FoundationModels.swiftmodule/arm64-apple-ios-simulator.swiftinterface"

mkdir -p "${REPORT_DIR}"
: >"${REPORT_PATH}"
printf 'developer_dir %s\n' "${DEVELOPER_DIR:-$(xcode-select -p)}" | tee -a "${REPORT_PATH}"
printf 'sdkroot %s\n' "${SDKROOT}" | tee -a "${REPORT_PATH}"

record_pattern() {
  local label="$1"
  local file="$2"
  local pattern="$3"

  if [ -f "${file}" ] && rg -q "${pattern}" "${file}"; then
    printf 'present %s\n' "${label}" | tee -a "${REPORT_PATH}"
  else
    printf 'missing %s\n' "${label}" | tee -a "${REPORT_PATH}"
  fi
}

record_import() {
  local module="$1"

  if xcrun swift -e "import ${module}; print(\"ok\")" >/dev/null 2>&1; then
    printf 'importable %s\n' "${module}" | tee -a "${REPORT_PATH}"
  else
    printf 'not-importable %s\n' "${module}" | tee -a "${REPORT_PATH}"
  fi
}

record_tree_pattern() {
  local label="$1"
  local directory="$2"
  local pattern="$3"

  if [ -d "${directory}" ] && rg -q "${pattern}" "${directory}"; then
    printf 'present %s\n' "${label}" | tee -a "${REPORT_PATH}"
  else
    printf 'missing %s\n' "${label}" | tee -a "${REPORT_PATH}"
  fi
}

record_pattern SyncableEntity "${APPINTENTS_IFACE}" 'protocol SyncableEntity'
record_pattern SyncableEntityIdentifier "${APPINTENTS_IFACE}" 'struct SyncableEntityIdentifier'
record_pattern IntentValueQuery "${APPINTENTS_IFACE}" 'protocol IntentValueQuery'
record_pattern RelevantEntities "${APPINTENTS_IFACE}" 'struct RelevantEntities'
record_pattern IntentExecutionTargets "${APPINTENTS_IFACE}" 'struct IntentExecutionTargets'
record_pattern LongRunningIntent "${APPINTENTS_IFACE}" 'protocol LongRunningIntent'
record_pattern NSUserActivityAppEntityIdentifier "${APPINTENTS_IFACE}" 'appEntityIdentifier'
record_tree_pattern SwiftUIAppEntityIdentifier "${SDKROOT}/System/Library/Frameworks" 'appEntityIdentifier'
record_pattern FoundationModelsTokenCount "${FOUNDATION_MODELS_IFACE}" 'tokenCount'
record_pattern FoundationModelsHistoryTransform "${FOUNDATION_MODELS_IFACE}" 'historyTransform'
record_pattern SpotlightSearchTool "${CORESPOTLIGHT_FM_IFACE}" 'struct SpotlightSearchTool'

record_import AppIntents
record_import FoundationModels
record_import _CoreSpotlight_FoundationModels
record_import AppIntentsTesting
