#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

DEFAULT_PROCESS="${LOOPER_IOS_PROCESS:-Looper}"
DEFAULT_BUNDLE_ID="${LOOPER_IOS_BUNDLE_ID:-dev.looper.app.ios}"
DEFAULT_SUBSYSTEM="${LOOPER_IOS_OSLOG_SUBSYSTEM:-${DEFAULT_BUNDLE_ID}}"
ARTIFACT_ROOT="${LOOPER_IOS_DIAGNOSTICS_DIR:-${REPO_ROOT}/build/ios-diagnostics}"
DEFAULT_OSLOG_TIMEOUT="20s"
DEFAULT_OSLOG_LEVEL="debug"
DEFAULT_OSLOG_STYLE="ndjson"
DEFAULT_LLDB_PRESET="ui"
DEFAULT_PERF_ITERATIONS="3"
DEFAULT_PERF_TIME_LIMIT="15s"
DEFAULT_PERF_COOLDOWN="2"
DEFAULT_PERF_TEMPLATE="Time Profiler"
DEFAULT_PERF_RUN_NAME="looper-ios"

usage() {
    cat <<'USAGE'
Usage:
  scripts/ios-diagnostics.sh doctor
  scripts/ios-diagnostics.sh oslog [options] [-- extra oslog-live args]
  scripts/ios-diagnostics.sh lldb-trap [options]
  scripts/ios-diagnostics.sh perf-loop --device <simulator-udid> [options]
  scripts/ios-diagnostics.sh ettrace [options]
  scripts/ios-diagnostics.sh capture --device <simulator-udid> [options]

Commands:
  doctor       Check local diagnostic tool availability.
  oslog        Capture Looper iOS OSLog output to build/ios-diagnostics.
  lldb-trap    Emit or run Looper iOS LLDB trap setup.
  perf-loop    Run repeatable xctrace captures through perf-loop.
  ettrace      Run ETTrace against the simulator with optional dSYMs.
  capture      Run oslog-live in parallel with perf-loop for one scenario.

Common defaults:
  process      Looper
  bundle id    dev.looper.app.ios
  subsystem    dev.looper.app.ios
  output root  build/ios-diagnostics

Environment:
  LOOPER_IOS_PROCESS, LOOPER_IOS_BUNDLE_ID, LOOPER_IOS_OSLOG_SUBSYSTEM,
  LOOPER_IOS_DIAGNOSTICS_DIR, LOOPER_IOS_SIMULATOR
USAGE
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

require_tool() {
    local tool="$1"
    command -v "$tool" >/dev/null 2>&1 || fail "missing required tool: ${tool}"
}

optional_tool_status() {
    local tool="$1"
    if command -v "$tool" >/dev/null 2>&1; then
        printf '%-12s %s\n' "$tool" "$(command -v "$tool")"
    else
        printf '%-12s %s\n' "$tool" "missing"
    fi
}

new_run_dir() {
    local kind="$1"
    local stamp
    stamp="$(date -u '+%Y%m%dT%H%M%SZ')"
    local run_dir="${ARTIFACT_ROOT}/${stamp}-${kind}"
    mkdir -p "$run_dir"
    printf '%s\n' "$run_dir"
}

print_artifact() {
    printf 'artifact: %s\n' "$1" >&2
}

simulator_app_executable() {
    local device="$1"
    local bundle_id="$2"
    xcrun simctl appinfo "$device" "$bundle_id" 2>/dev/null |
        awk -F' = ' '/CFBundleExecutable/ { gsub(/[\";]/, "", $2); print $2; exit }'
}

host_pid_for_executable() {
    local executable="$1"
    ps axww -o pid= -o command= |
        while IFS= read -r line; do
            local trimmed="${line#"${line%%[![:space:]]*}"}"
            local pid="${trimmed%%[[:space:]]*}"
            local command="${trimmed#"$pid"}"
            command="${command#"${command%%[![:space:]]*}"}"
            if [[ "$command" == "$executable" || "$command" == "$executable "* ]]; then
                printf '%s\n' "$pid"
                return 0
            fi
        done
}

simulator_app_pid() {
    local device="$1"
    local bundle_id="$2"
    local executable
    executable="$(simulator_app_executable "$device" "$bundle_id")"
    [[ -n "$executable" ]] || return 1
    host_pid_for_executable "$executable"
}

launch_simulator_app() {
    local device="$1"
    local bundle_id="$2"
    xcrun simctl launch "$device" "$bundle_id" |
        awk -F': ' -v bundle_id="$bundle_id" '$1 == bundle_id { print $2; exit }'
}

command_doctor() {
    optional_tool_status oslog-live
    optional_tool_status lldb-trap
    optional_tool_status perf-loop
    optional_tool_status ettrace
    optional_tool_status xctrace
    optional_tool_status xcodebuild
}

command_oslog() {
    require_tool oslog-live

    local process="$DEFAULT_PROCESS"
    local subsystem="$DEFAULT_SUBSYSTEM"
    local category=""
    local timeout="$DEFAULT_OSLOG_TIMEOUT"
    local level="$DEFAULT_OSLOG_LEVEL"
    local style="$DEFAULT_OSLOG_STYLE"
    local contains=""
    local predicate=""
    local output_dir=""
    local source_flag=()
    local payload_flag=()
    local extra_args=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --process) process="$2"; shift 2 ;;
            --subsystem) subsystem="$2"; shift 2 ;;
            --category) category="$2"; shift 2 ;;
            --timeout) timeout="$2"; shift 2 ;;
            --level) level="$2"; shift 2 ;;
            --style) style="$2"; shift 2 ;;
            --contains) contains="$2"; shift 2 ;;
            --predicate) predicate="$2"; shift 2 ;;
            --output-dir) output_dir="$2"; shift 2 ;;
            --source) source_flag=(--source); shift ;;
            --payload) payload_flag=(--payload); shift ;;
            --) shift; extra_args=("$@"); break ;;
            -h|--help) usage; exit 0 ;;
            *) fail "unknown oslog option: $1" ;;
        esac
    done

    local run_dir="${output_dir:-$(new_run_dir oslog)}"
    mkdir -p "$run_dir"
    local output_file="${run_dir}/oslog.${style}"
    local filter_args=(--process "$process" --subsystem "$subsystem" --timeout "$timeout" --level "$level" --style "$style")
    if [[ -n "$category" ]]; then
        filter_args+=(--category "$category")
    fi
    if [[ -n "$contains" ]]; then
        filter_args+=(--contains "$contains")
    fi
    if [[ -n "$predicate" ]]; then
        filter_args+=(--predicate "$predicate")
    fi

    if [[ "${#source_flag[@]}" -gt 0 ]]; then
        filter_args+=("${source_flag[@]}")
    fi
    if [[ "${#payload_flag[@]}" -gt 0 ]]; then
        filter_args+=("${payload_flag[@]}")
    fi
    if [[ "${#extra_args[@]}" -gt 0 ]]; then
        filter_args+=("${extra_args[@]}")
    fi

    print_artifact "$output_file"
    oslog-live "${filter_args[@]}" > "$output_file"
}

command_lldb_trap() {
    require_tool lldb-trap

    local process="$DEFAULT_PROCESS"
    local preset="$DEFAULT_LLDB_PRESET"
    local output_dir=""
    local run_attach="false"
    local wait_flag=(--wait)
    local print_flag=(--print)
    local extra_args=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --process|--attach-name) process="$2"; shift 2 ;;
            --preset) preset="$2"; shift 2 ;;
            --output-dir) output_dir="$2"; shift 2 ;;
            --attach) run_attach="true"; print_flag=(); shift ;;
            --no-wait) wait_flag=(); shift ;;
            --) shift; extra_args=("$@"); break ;;
            -h|--help) usage; exit 0 ;;
            *) extra_args+=("$1"); shift ;;
        esac
    done

    local run_dir="${output_dir:-$(new_run_dir lldb-trap)}"
    mkdir -p "$run_dir"
    local emit_file="${run_dir}/looper-${preset}-traps.lldb"
    local preset_flag="--${preset}"

    local lldb_args=("$preset_flag" --attach-name "$process" --emit "$emit_file")
    if [[ "${#wait_flag[@]}" -gt 0 ]]; then
        lldb_args+=("${wait_flag[@]}")
    fi
    if [[ "${#print_flag[@]}" -gt 0 ]]; then
        lldb_args+=("${print_flag[@]}")
    fi
    if [[ "${#extra_args[@]}" -gt 0 ]]; then
        lldb_args+=("${extra_args[@]}")
    fi

    print_artifact "$emit_file"
    lldb-trap "${lldb_args[@]}"
    if [[ "$run_attach" != "true" ]]; then
        printf 'note: add --attach to run LLDB now; default only emits/prints traps.\n' >&2
    fi
}

command_perf_loop() {
    require_tool perf-loop

    local process="$DEFAULT_PROCESS"
    local bundle_id="$DEFAULT_BUNDLE_ID"
    local iterations="$DEFAULT_PERF_ITERATIONS"
    local time_limit="$DEFAULT_PERF_TIME_LIMIT"
    local cooldown="$DEFAULT_PERF_COOLDOWN"
    local template="$DEFAULT_PERF_TEMPLATE"
    local output_dir=""
    local run_name="$DEFAULT_PERF_RUN_NAME"
    local device="${LOOPER_IOS_SIMULATOR:-}"
    local attach_pid=""
    local launch_app="false"
    local dry_run_flag=()
    local extra_args=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --process|--attach) process="$2"; bundle_id=""; shift 2 ;;
            --bundle-id) bundle_id="$2"; shift 2 ;;
            --pid) attach_pid="$2"; shift 2 ;;
            --launch) launch_app="true"; shift ;;
            --iterations) iterations="$2"; shift 2 ;;
            --time-limit) time_limit="$2"; shift 2 ;;
            --cooldown) cooldown="$2"; shift 2 ;;
            --template) template="$2"; shift 2 ;;
            --output-dir) output_dir="$2"; shift 2 ;;
            --run-name) run_name="$2"; shift 2 ;;
            --device) device="$2"; shift 2 ;;
            --dry-run) dry_run_flag=(--dry-run); shift ;;
            --) shift; extra_args=("$@"); break ;;
            -h|--help) usage; exit 0 ;;
            *) fail "unknown perf-loop option: $1" ;;
        esac
    done

    local run_dir="${output_dir:-$(new_run_dir perf-loop)}"
    mkdir -p "$run_dir"
    local device_args=()
    if [[ -n "$device" ]]; then
        device_args=(--device "$device")
    else
        fail "perf-loop requires --device or LOOPER_IOS_SIMULATOR to avoid attaching the host process"
    fi

    local attach_target="$process"
    if [[ -n "$attach_pid" ]]; then
        attach_target="$attach_pid"
    elif [[ -n "$bundle_id" ]]; then
        attach_target="$(simulator_app_pid "$device" "$bundle_id" || true)"
        if [[ "$launch_app" == "true" ]]; then
            attach_target="$(launch_simulator_app "$device" "$bundle_id")"
            [[ -n "$attach_target" ]] || fail "simctl launch did not return a pid for ${bundle_id} on ${device}"
        fi
        if [[ -z "$attach_target" ]]; then
            fail "no running simulator process for ${bundle_id} on ${device}; rerun with --launch or pass --pid"
        fi
        if ! ps -p "$attach_target" >/dev/null 2>&1; then
            fail "resolved pid ${attach_target} for ${bundle_id} is not running on the host"
        fi
        printf 'resolved simulator app %s on %s to pid %s\n' "$bundle_id" "$device" "$attach_target" >&2
    fi

    local perf_args=(
        --attach "$attach_target"
        --iterations "$iterations"
        --time-limit "$time_limit"
        --cooldown "$cooldown"
        --template "$template"
        --output-dir "$run_dir"
        --run-name "$run_name"
        --export-toc
    )
    if [[ "${#device_args[@]}" -gt 0 ]]; then
        perf_args+=("${device_args[@]}")
    fi
    if [[ "${#dry_run_flag[@]}" -gt 0 ]]; then
        perf_args+=("${dry_run_flag[@]}")
    fi
    if [[ "${#extra_args[@]}" -gt 0 ]]; then
        perf_args+=("${extra_args[@]}")
    fi

    print_artifact "$run_dir"
    perf-loop "${perf_args[@]}"
}

command_ettrace() {
    require_tool ettrace

    local dsyms=""
    local output_dir=""
    local launch_flag=()
    local simulator_flag=(--simulator)
    local verbose_flag=()
    local multi_thread_flag=()
    local dry_run="false"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --dsyms) dsyms="$2"; shift 2 ;;
            --output-dir) output_dir="$2"; shift 2 ;;
            --launch) launch_flag=(--launch); shift ;;
            --device) fail "ettrace wrapper supports simulator capture only; use --simulator" ;;
            --simulator) simulator_flag=(--simulator); shift ;;
            --verbose) verbose_flag=(--verbose); shift ;;
            --multi-thread) multi_thread_flag=(--multi-thread); shift ;;
            --dry-run) dry_run="true"; shift ;;
            -h|--help) usage; exit 0 ;;
            *) fail "unknown ettrace option: $1" ;;
        esac
    done

    local dsyms_args=()
    if [[ -n "$dsyms" ]]; then
        dsyms_args=(--dsyms "$dsyms")
    fi

    local run_dir="${output_dir:-$(new_run_dir ettrace)}"
    mkdir -p "$run_dir"
    print_artifact "$run_dir"

    local ettrace_command=(ettrace)
    if [[ "${#dsyms_args[@]}" -gt 0 ]]; then
        ettrace_command+=("${dsyms_args[@]}")
    fi
    if [[ "${#launch_flag[@]}" -gt 0 ]]; then
        ettrace_command+=("${launch_flag[@]}")
    fi
    if [[ "${#simulator_flag[@]}" -gt 0 ]]; then
        ettrace_command+=("${simulator_flag[@]}")
    fi
    if [[ "${#verbose_flag[@]}" -gt 0 ]]; then
        ettrace_command+=("${verbose_flag[@]}")
    fi
    if [[ "${#multi_thread_flag[@]}" -gt 0 ]]; then
        ettrace_command+=("${multi_thread_flag[@]}")
    fi

    if [[ "$dry_run" == "true" ]]; then
        printf 'cd %q && ' "$run_dir"
        printf '%q ' "${ettrace_command[@]}"
        printf '\n'
        return
    fi

    (
        cd "$run_dir"
        "${ettrace_command[@]}"
    )
}

command_capture() {
    local process="$DEFAULT_PROCESS"
    local subsystem="$DEFAULT_SUBSYSTEM"
    local timeout="$DEFAULT_OSLOG_TIMEOUT"
    local iterations="$DEFAULT_PERF_ITERATIONS"
    local time_limit="$DEFAULT_PERF_TIME_LIMIT"
    local device="${LOOPER_IOS_SIMULATOR:-}"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --process) process="$2"; shift 2 ;;
            --subsystem) subsystem="$2"; shift 2 ;;
            --timeout) timeout="$2"; shift 2 ;;
            --iterations) iterations="$2"; shift 2 ;;
            --time-limit) time_limit="$2"; shift 2 ;;
            --device) device="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) fail "unknown capture option: $1" ;;
        esac
    done

    if [[ -z "$device" ]]; then
        fail "capture requires --device or LOOPER_IOS_SIMULATOR to avoid attaching the host process"
    fi

    local run_dir
    run_dir="$(new_run_dir capture)"
    print_artifact "$run_dir"

    command_oslog \
        --process "$process" \
        --subsystem "$subsystem" \
        --timeout "$timeout" \
        --output-dir "$run_dir" &
    local oslog_pid="$!"

    local perf_args=(--attach "$process" --iterations "$iterations" --time-limit "$time_limit" --output-dir "${run_dir}/perf-loop")
    if [[ -n "$device" ]]; then
        perf_args+=(--device "$device")
    fi
    set +e
    command_perf_loop "${perf_args[@]}"
    local perf_status="$?"
    wait "$oslog_pid"
    local oslog_status="$?"
    set -e

    if [[ "$perf_status" -ne 0 ]]; then
        return "$perf_status"
    fi
    return "$oslog_status"
}

main() {
    if [[ $# -eq 0 ]]; then
        usage
        exit 1
    fi

    local subcommand="$1"
    shift
    case "$subcommand" in
        doctor) command_doctor "$@" ;;
        oslog) command_oslog "$@" ;;
        lldb-trap|trap) command_lldb_trap "$@" ;;
        perf-loop|perf) command_perf_loop "$@" ;;
        ettrace|et-trace) command_ettrace "$@" ;;
        capture) command_capture "$@" ;;
        -h|--help) usage ;;
        *) fail "unknown command: ${subcommand}" ;;
    esac
}

main "$@"
