#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IOS_DIR="${ROOT_DIR}/ios"
DERIVED_DATA_DIR="${ROOT_DIR}/.build/xcode/ios"
CHECK_DIR="${ROOT_DIR}/.build/check-ios"
PROJECT_DIFF_BEFORE="${CHECK_DIR}/project.diff.before"
PROJECT_DIFF_AFTER="${CHECK_DIR}/project.diff.after"
APP_INTENTS_METADATA="${DERIVED_DATA_DIR}/Build/Products/Debug-iphonesimulator/Looper.app/Metadata.appintents/extract.actionsdata"
SIRI_CAPABILITY_REPORT="${ROOT_DIR}/.build/siri-ai-capabilities/report.txt"
REQUIRED_APP_INTENTS=(
  'SearchLooperSessionsIntent'
  'AskContextualCurrentLooperSessionIntent'
  'AskCurrentLooperSessionIntent'
  'AskDefaultLooperSessionIntent'
  'SuggestLooperPromptIntent'
  'SetDefaultLooperSessionIntent'
)
REQUIRED_APP_ENTITY_METADATA=(
  'LooperSessionEntity'
  'Assistant Surface'
  'Project Path'
)
REQUIRED_APP_QUERY_METADATA=(
  'LooperSessionValueQuery'
)
REMOVED_APP_INTENTS=(
  'AskLatestCodexSessionIntent'
)
MAIN_EXECUTION_APP_INTENTS=(
  'AskLooperSessionIntent'
  'SetDefaultLooperSessionIntent'
  'AskDefaultLooperSessionIntent'
  'AskCurrentLooperSessionIntent'
  'AskContextualCurrentLooperSessionIntent'
)

command -v xcodegen >/dev/null 2>&1 || {
  printf 'error: missing required command: xcodegen\n' >&2
  exit 1
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

require_app_intent_metadata() {
  local required_intent

  if [ ! -s "${APP_INTENTS_METADATA}" ]; then
    printf 'error: missing App Intents metadata at %s\n' "${APP_INTENTS_METADATA}" >&2
    exit 1
  fi

  for required_intent in "${REQUIRED_APP_INTENTS[@]}"; do
    if ! strings "${APP_INTENTS_METADATA}" | grep -q "${required_intent}"; then
      printf 'error: App Intents metadata missing %s\n' "${required_intent}" >&2
      exit 1
    fi
  done
}

reject_removed_app_intents() {
  local removed_intent

  for removed_intent in "${REMOVED_APP_INTENTS[@]}"; do
    if strings "${APP_INTENTS_METADATA}" | grep -q "${removed_intent}"; then
      printf 'error: stale App Intents metadata still contains %s\n' "${removed_intent}" >&2
      exit 1
    fi
  done
}

reject_removed_app_intent_sources() {
  local removed_intent

  for removed_intent in "${REMOVED_APP_INTENTS[@]}"; do
    if rg -q "${removed_intent}" "${IOS_DIR}/LooperCompanion"; then
      printf 'error: source still contains removed App Intent %s\n' "${removed_intent}" >&2
      exit 1
    fi
  done
}

require_app_entity_metadata() {
  local required_metadata

  for required_metadata in "${REQUIRED_APP_ENTITY_METADATA[@]}"; do
    if ! strings "${APP_INTENTS_METADATA}" | grep -q "${required_metadata}"; then
      printf 'error: App Intents metadata missing entity metadata %s\n' "${required_metadata}" >&2
      exit 1
    fi
  done
}

require_app_query_metadata() {
  local required_metadata

  for required_metadata in "${REQUIRED_APP_QUERY_METADATA[@]}"; do
    if ! strings "${APP_INTENTS_METADATA}" | grep -q "${required_metadata}"; then
      printf 'error: App Intents metadata missing query metadata %s\n' "${required_metadata}" >&2
      exit 1
    fi
  done
}

require_syncable_entity_source_when_available() {
  if ! grep -q '^present SyncableEntity$' "${SIRI_CAPABILITY_REPORT}"; then
    return
  fi

  if ! rg -q 'LooperSessionEntity: SyncableEntity|extension LooperSessionEntity: SyncableEntity' \
    "${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift"; then
    printf 'error: SyncableEntity is available but LooperSessionEntity is not syncable\n' >&2
    exit 1
  fi

  if grep -q '^present SyncableEntityIdentifier$' "${SIRI_CAPABILITY_REPORT}"; then
    if ! rg -q 'sdkSyncableEntityIdentifier' "${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift" ||
      ! rg -q 'SyncableEntityIdentifier<String, String>' "${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift" ||
      ! rg -q 'stableSyncableEntityIdentifier' "${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift"; then
      printf 'error: SyncableEntityIdentifier is available but LooperSessionEntity does not expose stable syncable identifiers\n' >&2
      exit 1
    fi
  fi
}

require_current_session_resolver_source() {
  local resolver_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperCurrentSessionResolver.swift"
  local support_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift"
  local core_resolver_file="${IOS_DIR}/LooperCompanionCore/Sources/LooperCompanionCore/LooperCurrentSessionResolution.swift"

  if [ ! -s "${resolver_file}" ]; then
    printf 'error: missing deterministic current-session resolver at %s\n' "${resolver_file}" >&2
    exit 1
  fi

  if [ ! -s "${core_resolver_file}" ]; then
    printf 'error: missing tested current-session resolution policy at %s\n' "${core_resolver_file}" >&2
    exit 1
  fi

  if ! rg -q 'LooperCurrentSessionResolution\.resolve' "${resolver_file}"; then
    printf 'error: current-session resolver must use LooperCurrentSessionResolution.resolve\n' >&2
    exit 1
  fi

  if ! awk '
    /func currentSiriSessionEntity\(\)/ { in_current = 1 }
    in_current && /LooperCurrentSessionResolver\(\)\.currentEntity/ { found = 1 }
    in_current && /^    func / && !/currentSiriSessionEntity/ { in_current = 0 }
    END { exit found ? 0 : 1 }
  ' "${support_file}"; then
    printf 'error: currentSiriSessionEntity must route through LooperCurrentSessionResolver\n' >&2
    exit 1
  fi

  if awk '
    /func currentSiriSessionEntity\(\)/ { in_current = 1 }
    in_current && /(defaultSiriSessionEntity|sessionEntities|suggestedEntities|latest|lastUpdated)/ { found = 1 }
    in_current && /^    func / && !/currentSiriSessionEntity/ { in_current = 0 }
    END { exit found ? 0 : 1 }
  ' "${support_file}"; then
    printf 'error: currentSiriSessionEntity contains fallback lookup logic outside the resolver\n' >&2
    exit 1
  fi
}

require_main_execution_targets_when_available() {
  local intents_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriIntents.swift"
  local app_intent

  if ! grep -q '^present IntentExecutionTargets$' "${SIRI_CAPABILITY_REPORT}"; then
    return
  fi

  for app_intent in "${MAIN_EXECUTION_APP_INTENTS[@]}"; do
    if ! awk -v app_intent="${app_intent}" '
      $0 ~ "^struct " app_intent ": AppIntent" {
        in_intent = 1
        brace_depth = 0
        saw_allowed_execution_targets = 0
        found_main_execution_target = 0
      }
      in_intent {
        open_count = gsub(/\{/, "{")
        close_count = gsub(/\}/, "}")
        brace_depth += open_count - close_count
        if (/allowedExecutionTargets/) {
          saw_allowed_execution_targets = 1
        }
        if (saw_allowed_execution_targets && /\.main/) {
          found_main_execution_target = 1
        }
        if (brace_depth == 0) {
          in_intent = 0
          if (found_main_execution_target) {
            found = 1
          }
        }
      }
      END { exit found ? 0 : 1 }
    ' "${intents_file}"; then
      printf 'error: %s must set allowedExecutionTargets to .main when IntentExecutionTargets is available\n' "${app_intent}" >&2
      exit 1
    fi
  done
}

require_siri_search_source_when_available() {
  local intents_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriIntents.swift"
  local support_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift"

  if ! grep -q '^present IntentValueQuery$' "${SIRI_CAPABILITY_REPORT}"; then
    return
  fi

  if ! rg -q 'struct LooperSessionValueQuery: IntentValueQuery' "${support_file}"; then
    printf 'error: IntentValueQuery is available but LooperSessionValueQuery is missing\n' >&2
    exit 1
  fi

  if ! rg -q 'func values\(for input: String\)' "${support_file}"; then
    printf 'error: LooperSessionValueQuery must map String input to session entities\n' >&2
    exit 1
  fi

  if ! rg -q 'struct SearchLooperSessionsIntent: AppIntent' "${intents_file}"; then
    printf 'error: missing SearchLooperSessionsIntent\n' >&2
    exit 1
  fi

  if awk '
    /struct SearchLooperSessionsIntent: AppIntent/ { in_intent = 1; brace_depth = 0 }
    in_intent {
      open_count = gsub(/\{/, "{")
      close_count = gsub(/\}/, "}")
      brace_depth += open_count - close_count
      if (/sendPrompt/) { found = 1 }
      if (brace_depth == 0) { in_intent = 0 }
    }
    END { exit found ? 0 : 1 }
  ' "${intents_file}"; then
    printf 'error: SearchLooperSessionsIntent must not send prompts\n' >&2
    exit 1
  fi
}

require_onscreen_awareness_source_when_available() {
  local models_file="${IOS_DIR}/LooperCompanion/Models/CompanionModels.swift"
  local detail_file="${IOS_DIR}/LooperCompanion/UI/Sessions/SessionDetailScreen.swift"
  local session_row_file="${IOS_DIR}/LooperCompanion/UI/Sessions/SessionRow.swift"
  local search_row_file="${IOS_DIR}/LooperCompanion/UI/Sessions/SessionSearchRows.swift"
  local annotation_file="${IOS_DIR}/LooperCompanion/UI/Sessions/SessionEntityAnnotation.swift"
  local model_file="${IOS_DIR}/LooperCompanion/App/CompanionAppModel.swift"

  if grep -q '^present NSUserActivityAppEntityIdentifier$' "${SIRI_CAPABILITY_REPORT}"; then
    if ! rg -q 'activity\.appEntityIdentifier' "${models_file}"; then
      printf 'error: continuation activity must set appEntityIdentifier when available\n' >&2
      exit 1
    fi

    if ! rg -q 'assistantSurface:' "${detail_file}"; then
      printf 'error: continuation activity must receive the Siri assistant surface\n' >&2
      exit 1
    fi
  fi

  if grep -q '^present SwiftUIAppEntityIdentifier$' "${SIRI_CAPABILITY_REPORT}"; then
    if ! rg -q 'import _AppIntents_SwiftUI' "${annotation_file}"; then
      printf 'error: missing SwiftUI App Entity annotation bridge\n' >&2
      exit 1
    fi

    for annotated_file in "${detail_file}" "${session_row_file}" "${search_row_file}"; do
      if ! rg -q 'looperAppEntityIdentifier' "${annotated_file}"; then
        printf 'error: %s must annotate visible session entities\n' "${annotated_file}" >&2
        exit 1
      fi
    done
  fi

  if ! rg -q 'IntentDonationManager\.shared\.donate' "${model_file}"; then
    printf 'error: Siri interaction donations must use IntentDonationManager\n' >&2
    exit 1
  fi

  if ! rg -q 'OpenLooperSessionIntent' "${model_file}" ||
    ! rg -q 'SetDefaultLooperSessionIntent' "${model_file}"; then
    printf 'error: open-session and set-default Siri interactions must be donated\n' >&2
    exit 1
  fi
}

require_local_context_engine_source_when_available() {
  local support_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift"
  local start_delimiter_count
  local end_delimiter_count

  if grep -q '^present FoundationModelsTokenCount$' "${SIRI_CAPABILITY_REPORT}"; then
    if ! rg -q 'looperTokenBudgetedPrompt' "${support_file}" ||
      ! rg -q 'tokenCount\(for:' "${support_file}" ||
      ! rg -q 'contextSize' "${support_file}"; then
      printf 'error: Foundation Models prompts must use token-budgeted context shaping\n' >&2
      exit 1
    fi
  fi

  if grep -q '^present FoundationModelsHistoryTransform$' "${SIRI_CAPABILITY_REPORT}" &&
    ! rg -q 'historyTransform' "${support_file}"; then
    printf 'error: Foundation Models session must use a rolling history transform when available\n' >&2
    exit 1
  fi

  if grep -q '^present SpotlightSearchTool$' "${SIRI_CAPABILITY_REPORT}"; then
    if ! rg -q 'import _CoreSpotlight_FoundationModels' "${support_file}" ||
      ! rg -q 'SpotlightSearchTool' "${support_file}" ||
      ! rg -q 'usesSpotlightTool: true' "${support_file}"; then
      printf 'error: local context engine must wire the guarded SpotlightSearchTool path\n' >&2
      exit 1
    fi
  fi

  start_delimiter_count="$(rg -c 'untrustedContentStartDelimiter' "${support_file}" || true)"
  end_delimiter_count="$(rg -c 'untrustedContentEndDelimiter' "${support_file}" || true)"
  if [ "${start_delimiter_count}" -lt 4 ] || [ "${end_delimiter_count}" -lt 4 ]; then
    printf 'error: model prompts must wrap session content in untrusted-content delimiters\n' >&2
    exit 1
  fi
}

require_branded_siri_dialog_source() {
  local intents_file="${IOS_DIR}/LooperCompanion/AppIntents/LooperSiriIntents.swift"

  if rg -q 'dialog: "(Sent to |Summarized \(|Opening \\\()|return "(Found |No Looper sessions matched)' "${intents_file}"; then
    printf 'error: Siri dialogs must use explicit Looper session language\n' >&2
    exit 1
  fi

  if ! rg -q 'Looper sent the prompt' "${intents_file}" ||
    ! rg -q 'Looper summarized' "${intents_file}" ||
    ! rg -q 'Looper found' "${intents_file}" ||
    ! rg -q 'Looper suggests' "${intents_file}"; then
    printf 'error: Siri dialogs are missing branded Looper result text\n' >&2
    exit 1
  fi

  if ! rg -Fq 'Ask the current session in \(.applicationName)' "${intents_file}" ||
    ! rg -Fq 'Search sessions in \(.applicationName)' "${intents_file}" ||
    ! rg -Fq 'Set the default session in \(.applicationName)' "${intents_file}"; then
    printf 'error: App Shortcut phrases must put the app name in the natural spoken position\n' >&2
    exit 1
  fi
}

mkdir -p "${CHECK_DIR}"
bash "${ROOT_DIR}/scripts/audit-siri-ai-capabilities.sh"
if [ ! -s "${SIRI_CAPABILITY_REPORT}" ]; then
  printf 'error: missing Siri AI capability report at %s\n' "${SIRI_CAPABILITY_REPORT}" >&2
  exit 1
fi
reject_removed_app_intent_sources
require_syncable_entity_source_when_available
require_current_session_resolver_source
require_main_execution_targets_when_available
require_siri_search_source_when_available
require_onscreen_awareness_source_when_available
require_local_context_engine_source_when_available
require_branded_siri_dialog_source
git -C "${ROOT_DIR}" diff -- ios/LooperCompanion.xcodeproj > "${PROJECT_DIFF_BEFORE}"
xcodegen generate --spec "${IOS_DIR}/project.yml" --project "${IOS_DIR}"
git -C "${ROOT_DIR}" diff -- ios/LooperCompanion.xcodeproj > "${PROJECT_DIFF_AFTER}"
diff -u "${PROJECT_DIFF_BEFORE}" "${PROJECT_DIFF_AFTER}" >/dev/null || {
  printf 'error: ios/LooperCompanion.xcodeproj is not in sync with ios/project.yml\n' >&2
  printf 'run: xcodegen generate --spec ios/project.yml --project ios\n' >&2
  exit 1
}

swift test --package-path "${IOS_DIR}/LooperCompanionCore"

SELECTED_DEVELOPER_DIR="$(select_developer_dir)"
export DEVELOPER_DIR="${SELECTED_DEVELOPER_DIR}"
printf 'Using DEVELOPER_DIR=%s\n' "${DEVELOPER_DIR}"

xcodebuild -quiet \
  -project "${IOS_DIR}/LooperCompanion.xcodeproj" \
  -scheme LooperCompanion \
  -configuration Debug \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "${DERIVED_DATA_DIR}" \
  CODE_SIGNING_ALLOWED=NO \
  ENABLE_DEBUG_DYLIB=NO \
  ARCHS=arm64 \
  ONLY_ACTIVE_ARCH=NO \
  build

require_app_intent_metadata
require_app_entity_metadata
require_app_query_metadata
reject_removed_app_intents
