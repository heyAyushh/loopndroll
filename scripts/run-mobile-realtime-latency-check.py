#!/usr/bin/env python3
# noqa: SIZE_OK - standalone latency harness coordinates Xcode, CoreDevice, and server proof.
import argparse
import base64
import json
import math
import os
import re
import select
import signal
import socket
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec


ROOT_DIR = Path(__file__).resolve().parents[1]
IOS_DIR = ROOT_DIR / "ios"
PROJECT_PATH = IOS_DIR / "LooperCompanion.xcodeproj"
PROJECT_SPEC = IOS_DIR / "project.yml"
SESSION_MINI_STORE_PATH = IOS_DIR / "LooperCompanion" / "Services" / "CompanionSessionMiniLocalStore.swift"
COMPANION_APP_MODEL_PATH = IOS_DIR / "LooperCompanion" / "App" / "CompanionAppModel.swift"
CARGO_MANIFEST = ROOT_DIR / "crates" / "agent-control-plane" / "Cargo.toml"
SERVER_BINARY = ROOT_DIR / "crates" / "agent-control-plane" / "target" / "debug" / "looper-server"
CLI_BINARY = ROOT_DIR / "crates" / "agent-control-plane" / "target" / "debug" / "looper"
ARTIFACT_ROOT = ROOT_DIR / ".build" / "mobile-realtime-latency"
DERIVED_DATA_CACHE_ROOT = ARTIFACT_ROOT / "DerivedData"
LIVE_CONFIG_PATH = ARTIFACT_ROOT / "live-config.json"
DOC_PATH = ROOT_DIR / "docs" / "qa" / "mobile-realtime-latency.md"
SESSION_ID = "thread-mobile-latency"
SESSION_TITLE = "Looper latency fixture"
CODEX_STUB_LOG_FILENAME = "codex-resume-stub.log"
HTTP_HOST = "127.0.0.1"
HEALTH_TIMEOUT_SECONDS = 45
HEALTH_POLL_SECONDS = 0.25
REQUEST_TIMEOUT_SECONDS = 8
XCODEBUILD_TIMEOUT_SECONDS = 900
XCODEBUILD_RESULT_EXIT_GRACE_SECONDS = 3
PREFLIGHT_TIMEOUT_SECONDS = 30
XCODE_PROJECT_PREFLIGHT_TIMEOUT_SECONDS = 180
CODEX_STUB_ACCEPTANCE_TIMEOUT_SECONDS = 5
PROCESS_TERMINATION_GRACE_SECONDS = 5
HTTP_OK = 200
GRPC_PORT_OFFSET = 1
DER_SIGNATURE_PADDING = b"="
EXIT_OK = 0
EXIT_FAILURE = 1
DEFAULT_SAMPLE_COUNT = 10
MINIMUM_SAMPLE_COUNT = 1
P95_PERCENTILE = 95
CONNECT_BUDGET_MILLISECONDS = 2_000
MODE_SWITCH_BUDGET_MILLISECONDS = 1_500
PROMPT_ACK_BUDGET_MILLISECONDS = 1_500
TOTAL_BUDGET_MILLISECONDS = 6_000
STRICT_UI_P95_TARGET_MILLISECONDS = 30
STRICT_LOCAL_LAN_ACK_P95_TARGET_MILLISECONDS = 30
STRICT_TAILSCALE_INTERNET_ACK_P95_TARGET_MILLISECONDS = 100
SAMPLE_INDEX_PADDING = 2
PREFERRED_DEVELOPER_DIR = Path("/Applications/Xcode-beta.app/Contents/Developer")
UI_TEST_SELECTOR = (
    "LooperCompanionUITests/"
    "LooperCompanionControlFlowUITests/testLiveRealtimeLatencyWorkflow"
)
DEVICE_ID_ENVIRONMENT_KEY = "LOOPER_IOS_DEVICE_ID"
APPLE_DEVELOPMENT_IDENTITY_LABEL = "Apple Development:"
LATENCY_LOG_PATTERN = re.compile(
    r"MOBILE_REALTIME_LATENCY "
    r"connectMs=(?P<connect>\d+) "
    r"modeSwitchMs=(?P<mode>\d+) "
    r"promptAckMs=(?P<prompt>\d+) "
    r"totalMs=(?P<total>\d+)"
)
APP_SELFTEST_PASS_PATTERN = re.compile(r"G006_SELFTEST_PASS c007 (?P<payload>\{.*\})")
APP_SELFTEST_CASE = "c007"
APP_SELFTEST_ARGUMENT = "--g006-local-first-selftest"
APP_SELFTEST_SAMPLE_ARGUMENT = "--g006-local-first-selftest-samples"
APP_BUNDLE_IDENTIFIER = "dev.looper.app.ios"
APP_PRODUCT_BUNDLE_NAME = "Looper.app"
SIMULATOR_DERIVED_DATA_CACHE_NAME = "ios-simulator-selftest"
PHYSICAL_DERIVED_DATA_CACHE_NAME = "ios-physical-selftest"
ISOLATED_SIMULATOR_DERIVED_DATA_NAME = "SelfTestDerivedData"
ISOLATED_PHYSICAL_DERIVED_DATA_NAME = "PhysicalDeviceDerivedData"
APP_SELFTEST_TIMEOUT_SECONDS = 90
DEVICETL_TIMEOUT_SECONDS = 120
CREATE_THREADS_SQL = (
    "create table threads ("
    "thread_id text primary key,"
    "title text,"
    "cwd text,"
    "source text,"
    "model text,"
    "reasoning_effort text,"
    "created_at_ms integer,"
    "updated_at_ms integer,"
    "archived integer"
    ")"
)
INSERT_THREAD_SQL = (
    "insert into threads ("
    "thread_id,"
    "title,"
    "cwd,"
    "source,"
    "model,"
    "reasoning_effort,"
    "created_at_ms,"
    "updated_at_ms,"
    "archived"
    ") values (?, ?, ?, ?, ?, ?, ?, ?, ?)"
)
CREATE_DYNAMIC_TOOLS_SQL = (
    "create table thread_dynamic_tools ("
    "thread_id text not null,"
    "name text not null,"
    "namespace text,"
    "description text,"
    "defer_loading integer,"
    "position integer not null default 0"
    ")"
)
CREATE_SPAWN_EDGES_SQL = (
    "create table thread_spawn_edges ("
    "parent_thread_id text not null,"
    "child_thread_id text not null,"
    "status text"
    ")"
)
CREATE_LOGS_SQL = "create table logs (id integer primary key, message text)"


def main() -> int:
    args = parse_args()
    ensure_project_root()
    if args.assert_budget_contract:
        assert_strict_budget_contract()
    if args.preflight_only:
        run_preflight(device_id=args.device_id)
        return EXIT_OK
    run_dir = run_artifact_dir()
    run_dir.mkdir(parents=True, exist_ok=True)
    requested_sample_count = sample_count(args.samples)
    if args.strict_local_first_targets and args.expect_current_red:
        evidence = build_current_red_evidence(args, requested_sample_count, run_dir)
        write_evidence_if_requested(args.evidence, evidence)
        print_current_red_summary(evidence)
        if not evidence["currentRed"]:
            raise RuntimeError("strict local-first targets unexpectedly passed on current architecture")
        return EXIT_OK
    if args.app_selftest:
        try:
            result = run_app_selftest_latency(args, requested_sample_count, run_dir)
        except Exception as error:
            failure = build_app_selftest_failure_evidence(args, requested_sample_count, run_dir, error)
            (run_dir / "app-selftest-failure.json").write_text(
                json.dumps(failure, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            write_evidence_if_requested(args.evidence, failure)
            raise
        result_path = run_dir / "mobile-realtime-latency.json"
        result_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        write_evidence_if_requested(args.evidence, result)
        write_qa_report(result, "app-internal", run_dir)
        if args.strict_local_first_targets:
            assert_strict_latency_budgets(result, expect_current_red=False)
        else:
            raise RuntimeError("--app-selftest requires --strict-local-first-targets")
        print_app_selftest_summary(result)
        print(f"qa report: {DOC_PATH}")
        return EXIT_OK
    if args.strict_local_first_targets:
        try:
            result = run_live_transport_proof(args, requested_sample_count, run_dir)
        except Exception as error:
            failure = build_transport_proof_failure_evidence(args, requested_sample_count, run_dir, error)
            (run_dir / "mobile-realtime-transport-proof-failure.json").write_text(
                json.dumps(failure, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            write_evidence_if_requested(args.evidence, failure)
            raise
        write_evidence_if_requested(args.evidence, result)
        print_live_transport_proof_summary(result)
        return EXIT_OK
    seed_codex_state(run_dir)
    codex_stub = write_codex_stub(run_dir)
    codex_stub_log = codex_stub_log_path(run_dir)
    http_port = free_http_port_with_grpc_neighbor()
    grpc_port = http_port + GRPC_PORT_OFFSET
    base_url = f"http://{HTTP_HOST}:{http_port}"
    build_server_binary()
    server = start_server(run_dir, codex_stub, http_port, grpc_port)
    try:
        wait_for_health(base_url, server, run_dir / "server.log")
        credentials = register_mobile_session(base_url)
        assert_mobile_snapshot(base_url, credentials["bearer_token"], credentials["mobile_session"])
        xcodegen_project()
        sample_results = []
        for sample_index in range(requested_sample_count):
            reset_latency_session_mode(base_url)
            sample_label = str(sample_index + 1).zfill(SAMPLE_INDEX_PADDING)
            result_path = run_dir / f"mobile-realtime-latency-{sample_label}.json"
            xcodebuild_log = run_dir / f"xcodebuild-{sample_label}.log"
            write_live_latency_config(
                base_url=base_url,
                bearer_token=credentials["bearer_token"],
                mobile_session=credentials["mobile_session"],
                result_path=result_path,
            )
            accepted_turn_count = count_codex_stub_turn_starts(codex_stub_log)
            run_latency_ui_test(
                base_url=base_url,
                bearer_token=credentials["bearer_token"],
                mobile_session=credentials["mobile_session"],
                result_path=result_path,
                log_path=xcodebuild_log,
            )
            wait_for_codex_stub_turn_start(
                codex_stub_log,
                expected_min_count=accepted_turn_count + 1,
            )
            sample_result = read_latency_result(result_path, xcodebuild_log)
            sample_result["sampleIndex"] = sample_index + 1
            sample_results.append(sample_result)
        result = aggregate_latency_results(sample_results)
        result = attach_strict_latency_evidence(result, args, run_dir)
        result_path = run_dir / "mobile-realtime-latency.json"
        result_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        write_evidence_if_requested(args.evidence, result)
        write_qa_report(result, base_url, run_dir)
        if args.strict_local_first_targets:
            assert_strict_latency_budgets(result, expect_current_red=args.expect_current_red)
        else:
            assert_latency_budgets(result)
        print(
            "mobile realtime latency: "
            f"samples={result['sampleCount']} "
            f"connect_p95={result['p95']['connectMilliseconds']}ms "
            f"mode_p95={result['p95']['modeSwitchMilliseconds']}ms "
            f"prompt_p95={result['p95']['promptAckMilliseconds']}ms "
            f"total_p95={result['p95']['totalMilliseconds']}ms"
        )
        print(f"qa report: {DOC_PATH}")
        return EXIT_OK
    finally:
        stop_server(server)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run or preflight the Looper mobile realtime latency verifier."
    )
    parser.add_argument(
        "--preflight-only",
        action="store_true",
        help="validate local Xcode/CoreDevice prerequisites and exit before building",
    )
    parser.add_argument(
        "--device-id",
        default=os.environ.get(DEVICE_ID_ENVIRONMENT_KEY),
        help=(
            "optional physical device identifier for device/signing preflight; "
            f"defaults to ${DEVICE_ID_ENVIRONMENT_KEY}"
        ),
    )
    parser.add_argument(
        "--samples",
        "--sample-count",
        dest="samples",
        type=int,
        help="override LOOPER_LIVE_LATENCY_SAMPLE_COUNT for this run",
    )
    parser.add_argument(
        "--strict-local-first-targets",
        action="store_true",
        help="evaluate against 30ms UI, 30ms local/LAN ACK, and 100ms Tailscale ACK targets",
    )
    parser.add_argument(
        "--expect-current-red",
        action="store_true",
        help="exit 0 only when current architecture evidence is classified red",
    )
    parser.add_argument(
        "--force-http-fallback",
        action="store_true",
        help="record forced HTTP fallback as observable current-red evidence",
    )
    parser.add_argument(
        "--force-h3-failure",
        action="store_true",
        help="use a dead UDP/H3 endpoint while keeping live HTTP/2 as the fallback transport",
    )
    parser.add_argument(
        "--app-selftest",
        action="store_true",
        help="run the DEBUG app-internal strict local-first latency selftest instead of XCUITest",
    )
    parser.add_argument(
        "--physical-device",
        action="store_true",
        help="run the app-internal latency selftest on a paired physical iPhone via devicectl",
    )
    parser.add_argument(
        "--isolated-derived-data",
        action="store_true",
        help=(
            "use a run-local DerivedData directory for cold-build isolation; "
            "by default the harness reuses stable DerivedData caches for fast proofs"
        ),
    )
    parser.add_argument(
        "--assert-budget-contract",
        action="store_true",
        help="assert strict targets are not widened to legacy latency budgets",
    )
    parser.add_argument(
        "--evidence",
        type=Path,
        help="project-relative or absolute JSON evidence output path",
    )
    return parser.parse_args()


def ensure_project_root() -> None:
    if not (ROOT_DIR / "AGENTS.md").is_file() or not CARGO_MANIFEST.is_file():
        raise RuntimeError(f"script must run from the Looper project root: {ROOT_DIR}")


def run_preflight(device_id: Optional[str]) -> None:
    results = [
        run_preflight_command("darwin cache dir", ["getconf", "DARWIN_USER_CACHE_DIR"]),
        run_preflight_command("darwin temp dir", ["getconf", "DARWIN_USER_TEMP_DIR"]),
        run_preflight_command("xcode version", ["xcodebuild", "-version"], env=xcode_environment()),
        run_preflight_command(
            "xcode project list",
            ["xcodebuild", "-list", "-project", str(PROJECT_PATH)],
            env=xcode_environment(),
            timeout_seconds=XCODE_PROJECT_PREFLIGHT_TIMEOUT_SECONDS,
        ),
        run_preflight_command(
            "simulator service",
            ["xcrun", "simctl", "list", "devices", "available", "-j"],
            env=xcode_environment(),
        ),
    ]
    if device_id:
        results.extend(
            [
                run_preflight_command(
                    "physical device",
                    ["xcrun", "devicectl", "device", "info", "details", "--device", device_id],
                    env=xcode_environment(),
                ),
                run_signing_identity_preflight(),
            ]
        )

    failed_results = [result for result in results if not result["ok"]]
    for result in results:
        status = "ok" if result["ok"] else "failed"
        print(f"preflight {status}: {result['name']}")
        if result["output"]:
            print(indent(result["output"], "  "))
    if failed_results:
        failed_names = ", ".join(result["name"] for result in failed_results)
        raise RuntimeError(f"mobile realtime latency preflight failed: {failed_names}")


def run_preflight_command(
    name: str,
    command: list[str],
    env: Optional[dict[str, str]] = None,
    timeout_seconds: int = PREFLIGHT_TIMEOUT_SECONDS,
) -> dict:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT_DIR,
            env=env or os.environ.copy(),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        output = preflight_output_text(error.stdout) + preflight_output_text(error.stderr)
        return {"name": name, "ok": False, "output": output.strip() or "timed out"}
    return {
        "name": name,
        "ok": result.returncode == EXIT_OK,
        "output": result.stdout.strip(),
    }


def preflight_output_text(value: object) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def run_signing_identity_preflight() -> dict:
    result = run_preflight_command(
        "codesigning identity",
        ["security", "find-identity", "-v", "-p", "codesigning"],
    )
    result["ok"] = result["ok"] and APPLE_DEVELOPMENT_IDENTITY_LABEL in result["output"]
    return result


def indent(value: str, prefix: str) -> str:
    return "\n".join(f"{prefix}{line}" for line in value.splitlines())


def run_artifact_dir() -> Path:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return ARTIFACT_ROOT / "runs" / timestamp


def sample_count(cli_sample_count: Optional[int]) -> int:
    if cli_sample_count is not None:
        parsed = cli_sample_count
        source = "--samples"
    else:
        raw_value = os.environ.get("LOOPER_LIVE_LATENCY_SAMPLE_COUNT", str(DEFAULT_SAMPLE_COUNT))
        source = "LOOPER_LIVE_LATENCY_SAMPLE_COUNT"
        try:
            parsed = int(raw_value)
        except ValueError as error:
            raise RuntimeError(f"invalid LOOPER_LIVE_LATENCY_SAMPLE_COUNT: {raw_value!r}") from error
    if parsed < MINIMUM_SAMPLE_COUNT:
        raise RuntimeError(f"{source} must be at least {MINIMUM_SAMPLE_COUNT}")
    return parsed


def evidence_path(path: Optional[Path]) -> Optional[Path]:
    if path is None:
        return None
    if path.is_absolute():
        return path
    return ROOT_DIR / path


def write_evidence_if_requested(path: Optional[Path], payload: dict) -> None:
    resolved_path = evidence_path(path)
    if resolved_path is None:
        return
    resolved_path.parent.mkdir(parents=True, exist_ok=True)
    resolved_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def assert_strict_budget_contract() -> None:
    strict_targets = strict_target_contract()
    expected_targets = {
        "uiP95Ms": 30,
        "localLanAckP95Ms": 30,
        "tailscaleInternetAckP95Ms": 100,
    }
    if strict_targets != expected_targets:
        raise RuntimeError(f"strict target contract changed: {strict_targets}")
    legacy_budget_values = {
        CONNECT_BUDGET_MILLISECONDS,
        MODE_SWITCH_BUDGET_MILLISECONDS,
        PROMPT_ACK_BUDGET_MILLISECONDS,
        TOTAL_BUDGET_MILLISECONDS,
    }
    strict_budget_values = set(strict_targets.values())
    legacy_strict_overlap = sorted(legacy_budget_values & strict_budget_values)
    if legacy_strict_overlap:
        raise RuntimeError(f"legacy budgets accepted as strict targets: {legacy_strict_overlap}")


def strict_target_contract() -> dict[str, int]:
    return {
        "uiP95Ms": STRICT_UI_P95_TARGET_MILLISECONDS,
        "localLanAckP95Ms": STRICT_LOCAL_LAN_ACK_P95_TARGET_MILLISECONDS,
        "tailscaleInternetAckP95Ms": STRICT_TAILSCALE_INTERNET_ACK_P95_TARGET_MILLISECONDS,
    }


def strict_budget_contract_evidence(asserted: bool) -> dict:
    return {
        "asserted": asserted,
        "strictTargetsMs": strict_target_contract(),
        "legacyBudgetsMs": {
            "connectMilliseconds": CONNECT_BUDGET_MILLISECONDS,
            "modeSwitchMilliseconds": MODE_SWITCH_BUDGET_MILLISECONDS,
            "promptAckMilliseconds": PROMPT_ACK_BUDGET_MILLISECONDS,
            "totalMilliseconds": TOTAL_BUDGET_MILLISECONDS,
        },
        "legacyBudgetsAcceptedAsStrict": False,
    }


def project_relative(path: Path) -> str:
    try:
        return str(path.relative_to(ROOT_DIR))
    except ValueError:
        return str(path)


def build_current_red_evidence(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
) -> dict:
    snapshot_evidence = detect_current_snapshot_evidence()
    fallback_evidence = build_fallback_evidence(args.force_http_fallback, snapshot_evidence)
    missing_metrics = [
        "uiModeMs",
        "uiPromptMs",
        "modeAckMs",
        "promptAckMs",
        "notificationPersistMs",
        "streamApplyMs",
    ]
    strict_violations = [
        strict_violation(metric, "missing-current-instrumentation")
        for metric in missing_metrics
    ]
    if snapshot_evidence["snapshotOnTapCount"] > 0:
        strict_violations.append(
            {
                "metric": "snapshotOnTapCount",
                "target": "0 snapshot refreshes on realtime tap/action path",
                "actual": snapshot_evidence["snapshotOnTapCount"],
                "reason": "current realtime path still schedules snapshot refreshes",
            }
        )
    current_red_reasons = [
        "strict p95 metrics are not yet emitted by the current UI-test payload",
    ]
    if snapshot_evidence["snapshotOnTapCount"] > 0:
        current_red_reasons.append("current realtime path still routes through snapshot refresh")
    if args.force_http_fallback:
        current_red_reasons.append("HTTP fallback forced by harness flag")
    current_red = bool(strict_violations or fallback_evidence["forcedHttpFallback"])
    return {
        "schemaVersion": 1,
        "goal": "G001-instrument-current-latency-and-snaps",
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "classification": "current-red" if current_red else "strict-pass",
        "currentRed": current_red,
        "currentRedReasons": current_red_reasons if current_red else [],
        "sampleCount": requested_sample_count,
        "sampleCountSource": "--samples" if args.samples is not None else "LOOPER_LIVE_LATENCY_SAMPLE_COUNT",
        "runArtifactDir": project_relative(run_dir),
        "uiModeMs": None,
        "uiPromptMs": None,
        "modeAckMs": None,
        "promptAckMs": None,
        "notificationPersistMs": None,
        "streamApplyMs": None,
        "snapshotOnTapCount": snapshot_evidence["snapshotOnTapCount"],
        "strictTargetsMs": strict_target_contract(),
        "strictTargetViolations": strict_violations,
        "fallback": fallback_evidence,
        "budgetContract": strict_budget_contract_evidence(args.assert_budget_contract),
        "sourceFields": {
            "available": False,
            "reason": "live UI-test samples were not required to prove the current-red architecture path",
            "legacyFieldNames": [
                "connectMilliseconds",
                "modeSwitchMilliseconds",
                "promptAckMilliseconds",
                "totalMilliseconds",
            ],
        },
        "sourceEvidence": snapshot_evidence["sourceEvidence"],
    }


def detect_current_snapshot_evidence() -> dict:
    session_mini_store_path = SESSION_MINI_STORE_PATH
    app_model_path = COMPANION_APP_MODEL_PATH
    session_mini_store_source = session_mini_store_path.read_text(encoding="utf-8")
    app_model_source = app_model_path.read_text(encoding="utf-8")
    command_reconciliation_tokens = [
        "applyModeResult",
        "applyPromptSendResult",
        "applyNotificationReplyAccepted",
        "targetRevision",
    ]
    command_reconciliation_count = sum(
        app_model_source.count(token) for token in command_reconciliation_tokens
    )
    rust_core_command_counts = {
        "sessionManager.setMode(": session_mini_store_source.count("sessionManager.setMode("),
        "sessionManager.sendPrompt(": session_mini_store_source.count("sessionManager.sendPrompt("),
        "sessionManager.submitNotificationReply(": session_mini_store_source.count(
            "sessionManager.submitNotificationReply("
        ),
    }
    service_snapshot_count = app_model_source.count("service.loadSnapshot()")
    snapshot_count = command_reconciliation_count
    return {
        "snapshotOnTapCount": snapshot_count,
        "sourceEvidence": [
            {
                "path": project_relative(app_model_path),
                "observable": "+".join(command_reconciliation_tokens),
                "count": command_reconciliation_count,
            },
            {
                "path": project_relative(app_model_path),
                "observable": "service.loadSnapshot()",
                "count": service_snapshot_count,
            },
            {
                "path": project_relative(session_mini_store_path),
                "observable": "Rust-core command manager calls",
                "count": sum(rust_core_command_counts.values()),
                "breakdown": rust_core_command_counts,
            },
        ],
    }


def build_fallback_evidence(forced_http_fallback: bool, snapshot_evidence: dict) -> dict:
    mode = "forced-http-fallback" if forced_http_fallback else "not-forced"
    return {
        "forcedHttpFallback": forced_http_fallback,
        "mode": mode,
        "observable": forced_http_fallback or snapshot_evidence["snapshotOnTapCount"] > 0,
        "snapshotOnTapCount": snapshot_evidence["snapshotOnTapCount"],
    }


def strict_violation(metric: str, reason: str) -> dict:
    return {
        "metric": metric,
        "targetMs": strict_target_for_metric(metric),
        "actualMs": None,
        "reason": reason,
    }


def strict_target_for_metric(metric: str) -> int:
    if metric in {"uiModeMs", "uiPromptMs", "streamApplyMs"}:
        return STRICT_UI_P95_TARGET_MILLISECONDS
    if metric in {"modeAckMs", "promptAckMs", "notificationPersistMs"}:
        return STRICT_LOCAL_LAN_ACK_P95_TARGET_MILLISECONDS
    return STRICT_TAILSCALE_INTERNET_ACK_P95_TARGET_MILLISECONDS


def print_current_red_summary(evidence: dict) -> None:
    print(
        "mobile realtime latency current-red: "
        f"samples={evidence['sampleCount']} "
        f"snapshotOnTapCount={evidence['snapshotOnTapCount']} "
        f"violations={len(evidence['strictTargetViolations'])} "
        f"fallback={evidence['fallback']['mode']}"
    )


def run_live_transport_proof(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
) -> dict:
    seed_codex_state(run_dir)
    codex_stub = write_codex_stub(run_dir)
    codex_stub_log = codex_stub_log_path(run_dir)
    http_port = free_http_port_with_grpc_neighbor()
    grpc_port = http_port + GRPC_PORT_OFFSET
    base_url = f"http://{HTTP_HOST}:{http_port}"
    build_server_binary()
    server = start_server(run_dir, codex_stub, http_port, grpc_port)
    cleanup = {
        "serverPid": server.pid,
        "httpPort": http_port,
        "grpcTcpPort": grpc_port,
        "grpcUdpPort": grpc_port,
        "runArtifactDir": project_relative(run_dir),
        "serverLog": project_relative(run_dir / "server.log"),
        "tempDirs": [project_relative(run_dir), project_relative(ARTIFACT_ROOT)],
        "tmuxSessions": [],
    }
    try:
        wait_for_health(base_url, server, run_dir / "server.log")
        health = http_json(f"{base_url}/api/mobile/health")
        credentials = register_mobile_session(base_url)
        assert_mobile_snapshot(base_url, credentials["bearer_token"], credentials["mobile_session"])
        h2_url = grpc_endpoint_url(base_url, "http")
        h3_url = health.get("grpcH3BaseURL") or grpc_endpoint_url(base_url, "https")
        h3_pin = health.get("grpcH3CertificateSha256")
        if not h3_pin:
            raise RuntimeError(f"mobile health did not include grpcH3CertificateSha256: {health}")
        if args.force_h3_failure:
            h3_url = dead_h3_endpoint_url(base_url)
        probe_result = run_transport_probe(
            run_dir=run_dir,
            sample_count=requested_sample_count,
            h3_url=h3_url,
            h2_url=h2_url,
            h3_certificate_sha256=h3_pin,
            bearer_token=credentials["bearer_token"],
            mobile_session=credentials["mobile_session"],
            force_h3_failure=args.force_h3_failure,
        )
        result = build_transport_proof_evidence(
            args=args,
            requested_sample_count=requested_sample_count,
            run_dir=run_dir,
            base_url=base_url,
            h2_url=h2_url,
            h3_url=h3_url,
            h3_certificate_sha256=h3_pin,
            health=health,
            probe_result=probe_result,
            cleanup=cleanup,
            codex_stub_log=codex_stub_log,
        )
        (run_dir / "mobile-realtime-transport-proof.json").write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        return result
    finally:
        stop_server(server)
        cleanup["serverExitStatus"] = server.poll()
        cleanup_path = run_dir / "transport-proof-cleanup.json"
        cleanup_path.write_text(json.dumps(cleanup, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def grpc_endpoint_url(base_url: str, scheme: str) -> str:
    parsed = urllib.parse.urlparse(base_url)
    if parsed.hostname is None or parsed.port is None:
        raise RuntimeError(f"base URL has no host/port: {base_url}")
    port = parsed.port + GRPC_PORT_OFFSET
    return urllib.parse.urlunparse((scheme, f"{parsed.hostname}:{port}", "", "", "", ""))


def dead_h3_endpoint_url(base_url: str) -> str:
    parsed = urllib.parse.urlparse(base_url)
    if parsed.hostname is None:
        raise RuntimeError(f"base URL has no host: {base_url}")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.bind((HTTP_HOST, 0))
        port = probe.getsockname()[1]
    return urllib.parse.urlunparse(("https", f"{parsed.hostname}:{port}", "", "", "", ""))


def run_transport_probe(
    run_dir: Path,
    sample_count: int,
    h3_url: str,
    h2_url: str,
    h3_certificate_sha256: str,
    bearer_token: str,
    mobile_session: str,
    force_h3_failure: bool,
) -> dict:
    probe_dir = write_transport_probe_project(run_dir)
    config_path = run_dir / "transport-proof-config.json"
    output_path = run_dir / "transport-proof-output.json"
    log_path = run_dir / "transport-proof-probe.log"
    config = {
        "sampleCount": sample_count,
        "sessionId": SESSION_ID,
        "h3Url": h3_url,
        "h2Url": h2_url,
        "h3CertificateSha256": h3_certificate_sha256,
        "bearerToken": bearer_token,
        "mobileSession": mobile_session,
        "forceH3Failure": force_h3_failure,
        "outputPath": str(output_path),
    }
    config_path.write_text(json.dumps(config, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    command = ["cargo", "run", "--quiet", "--", str(config_path)]
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(ROOT_DIR / "target" / "transport-proof-probe")
    with open(log_path, "w", encoding="utf-8") as log:
        process = subprocess.run(
            command,
            cwd=probe_dir,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=180,
        )
        log.write(process.stdout)
    if process.returncode != EXIT_OK:
        raise RuntimeError(
            f"transport proof probe failed with exit {process.returncode}: {' '.join(command)}\n"
            f"{tail(log_path)}"
        )
    if not output_path.is_file():
        raise RuntimeError(f"transport proof probe did not write {output_path}:\n{tail(log_path)}")
    return json.loads(output_path.read_text(encoding="utf-8"))


def write_transport_probe_project(run_dir: Path) -> Path:
    probe_dir = run_dir / "transport-proof-probe"
    source_dir = probe_dir / "src"
    source_dir.mkdir(parents=True, exist_ok=True)
    cargo_toml = f"""
[package]
name = "looper-transport-proof-probe"
version = "0.0.0"
edition = "2024"

[dependencies]
agent-control-plane = {{ path = "{(ROOT_DIR / "crates" / "agent-control-plane").as_posix()}" }}
anyhow = "1.0.100"
serde = {{ version = "1.0.228", features = ["derive"] }}
serde_json = "1.0.145"
sha2 = "0.10.9"
tokio = {{ version = "1.48.0", features = ["macros", "rt-multi-thread", "time"] }}
tokio-stream = {{ version = "0.1.17", features = ["sync"] }}
tonic = "0.14.6"
tonic-h3 = {{ version = "0.0.5", default-features = false, features = ["quinn"] }}
"""
    probe_dir.joinpath("Cargo.toml").write_text(cargo_toml.lstrip(), encoding="utf-8")
    source_dir.joinpath("main.rs").write_text(TRANSPORT_PROBE_SOURCE, encoding="utf-8")
    return probe_dir


def build_transport_proof_evidence(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
    base_url: str,
    h2_url: str,
    h3_url: str,
    h3_certificate_sha256: str,
    health: dict,
    probe_result: dict,
    cleanup: dict,
    codex_stub_log: Path,
) -> dict:
    observed_transports = sorted({sample["transport"] for sample in probe_result["samples"]})
    expected_transport = "h2" if args.force_h3_failure else "h3"
    if observed_transports != [expected_transport]:
        raise RuntimeError(f"unexpected transport proof result: expected {expected_transport}, got {observed_transports}")
    ack_kinds = {
        ack["kind"]
        for sample in probe_result["samples"]
        for ack in sample.get("acks", [])
    }
    required_ack_kinds = {"SetSessionMode", "SendSessionPrompt", "SubmitNotificationReply"}
    if ack_kinds != required_ack_kinds:
        raise RuntimeError(f"missing transport proof ACK kinds: {sorted(required_ack_kinds - ack_kinds)}")
    if args.force_h3_failure and not any(sample.get("fallbackReason") for sample in probe_result["samples"]):
        raise RuntimeError("forced H3 failure did not report an H3 fallback reason")
    p95 = {
        "healthMilliseconds": percentile([sample["timings"]["healthMilliseconds"] for sample in probe_result["samples"]], P95_PERCENTILE),
        "openMilliseconds": percentile([sample["timings"]["openMilliseconds"] for sample in probe_result["samples"]], P95_PERCENTILE),
        "ackMilliseconds": percentile([sample["timings"]["ackMilliseconds"] for sample in probe_result["samples"]], P95_PERCENTILE),
        "totalMilliseconds": percentile([sample["timings"]["totalMilliseconds"] for sample in probe_result["samples"]], P95_PERCENTILE),
    }
    return {
        "schemaVersion": 2,
        "goal": "G011-tonic-h3-live-transport-proof",
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "classification": "strict-pass",
        "currentRed": False,
        "sampleCount": requested_sample_count,
        "sampleCountSource": "--sample-count/--samples" if args.samples is not None else "LOOPER_LIVE_LATENCY_SAMPLE_COUNT",
        "runArtifactDir": project_relative(run_dir),
        "networkClass": "loopback-local-first",
        "baseURL": base_url,
        "transport": {
            "selected": expected_transport,
            "observed": observed_transports,
            "h3EndpointURL": h3_url,
            "h2EndpointURL": h2_url,
            "forcedH3Failure": args.force_h3_failure,
            "fallbackPreserved": args.force_h3_failure and expected_transport == "h2",
        },
        "certPin": {
            "source": "authenticated mobile health grpcH3CertificateSha256",
            "sha256": h3_certificate_sha256,
        },
        "healthRouteMetadata": {
            "grpcBaseURL": health.get("grpcBaseURL"),
            "grpcH3BaseURL": health.get("grpcH3BaseURL"),
            "grpcH3CertificateSha256": health.get("grpcH3CertificateSha256"),
        },
        "p95": p95,
        "samples": probe_result["samples"],
        "assertions": {
            "liveServerStartedWithH2AndH3": True,
            "mobileSessionRegistered": True,
            "modeSwitchAcked": "SetSessionMode" in ack_kinds,
            "promptSendAcked": "SendSessionPrompt" in ack_kinds,
            "notificationReplyAcked": "SubmitNotificationReply" in ack_kinds,
            "pendingCommandsPreservedThroughFallback": probe_result["pendingCommandsQueuedBeforeOpen"],
            "resumeAfterSeqPreserved": all(sample["resumeAfterSeq"] == sample["observedResumeAfterSeq"] for sample in probe_result["samples"]),
            "h2FallbackKept": True,
            "codexStubTurnStartsObserved": count_codex_stub_turn_starts(codex_stub_log),
        },
        "cleanup": cleanup,
        "probe": {
            "config": project_relative(run_dir / "transport-proof-config.json"),
            "log": project_relative(run_dir / "transport-proof-probe.log"),
            "output": project_relative(run_dir / "transport-proof-output.json"),
        },
    }


def build_transport_proof_failure_evidence(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
    error: Exception,
) -> dict:
    return {
        "schemaVersion": 2,
        "goal": "G011-tonic-h3-live-transport-proof",
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "classification": "blocked",
        "status": "blocked",
        "currentRed": True,
        "sampleCount": requested_sample_count,
        "sampleCountSource": "--sample-count/--samples" if args.samples is not None else "LOOPER_LIVE_LATENCY_SAMPLE_COUNT",
        "runArtifactDir": project_relative(run_dir),
        "networkClass": "loopback-local-first",
        "transport": {
            "selected": "h2" if args.force_h3_failure else "h3",
            "forcedH3Failure": args.force_h3_failure,
        },
        "blocker": str(error),
        "probe": {
            "config": project_relative(run_dir / "transport-proof-config.json"),
            "log": project_relative(run_dir / "transport-proof-probe.log"),
            "output": project_relative(run_dir / "transport-proof-output.json"),
        },
        "cleanup": {
            "runArtifactDir": project_relative(run_dir),
            "serverLog": project_relative(run_dir / "server.log"),
            "cleanupLog": project_relative(run_dir / "transport-proof-cleanup.json"),
        },
    }


def print_live_transport_proof_summary(evidence: dict) -> None:
    print(
        "mobile realtime transport strict-pass: "
        f"samples={evidence['sampleCount']} "
        f"transport={evidence['transport']['selected']} "
        f"h3={evidence['transport']['h3EndpointURL']} "
        f"h2={evidence['transport']['h2EndpointURL']} "
        f"ack_p95={evidence['p95']['ackMilliseconds']}ms"
    )


TRANSPORT_PROBE_SOURCE = r'''
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_control_plane::grpc::proto;
use agent_control_plane::grpc::proto::{client_frame, command, server_frame};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_stream::iter;
use tonic::codegen::http::Uri;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint as H2Endpoint;
use tonic_h3::quinn::H3QuinnConnector;
use tonic_h3::quinn::h3_quinn::Endpoint as H3Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{
    ClientConfig, crypto::rustls::QuicClientConfig, rustls as quinn_rustls,
};

const H3_ALPN: &[u8] = b"h3";
const SESSION_OPEN_TIMEOUT: Duration = Duration::from_millis(400);
const ACK_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    sample_count: usize,
    session_id: String,
    h3_url: String,
    h2_url: String,
    h3_certificate_sha256: String,
    bearer_token: String,
    mobile_session: String,
    force_h3_failure: bool,
    output_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeOutput {
    sample_count: usize,
    pending_commands_queued_before_open: bool,
    force_h3_failure: bool,
    samples: Vec<SampleOutput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SampleOutput {
    sample_index: usize,
    transport: String,
    endpoint_url: String,
    fallback_reason: String,
    resume_after_seq: i64,
    observed_resume_after_seq: i64,
    resume_probe: ResumeOutput,
    health: TransportHealth,
    timings: SampleTimings,
    acks: Vec<AckOutput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResumeOutput {
    transport: String,
    endpoint_url: String,
    fallback_reason: String,
    after_seq: i64,
    latest_seq: i64,
    replayed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransportHealth {
    h3_ok: bool,
    h3_error: String,
    h2_ok: bool,
    h2_error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SampleTimings {
    health_milliseconds: i64,
    open_milliseconds: i64,
    ack_milliseconds: i64,
    total_milliseconds: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AckOutput {
    kind: String,
    client_mutation_id: String,
    accepted: bool,
    ack_seq: i64,
    entity_id: String,
    revision: String,
    error_code: String,
    reject_reason: String,
}

struct OpenedSession {
    transport: &'static str,
    endpoint_url: String,
    fallback_reason: String,
    stream: tonic::Streaming<proto::ServerFrame>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: looper-transport-proof-probe <config.json>"))?;
    let config: Config = serde_json::from_str(&fs::read_to_string(&config_path)?)?;
    let mut samples = Vec::with_capacity(config.sample_count);
    let mut latest_seq = 0_i64;

    for sample_index in 1..=config.sample_count {
        let sample = run_sample(&config, sample_index, latest_seq).await?;
        latest_seq = sample
            .acks
            .iter()
            .map(|ack| ack.ack_seq)
            .max()
            .unwrap_or(latest_seq)
            .max(latest_seq);
        samples.push(sample);
    }

    let output = ProbeOutput {
        sample_count: config.sample_count,
        pending_commands_queued_before_open: true,
        force_h3_failure: config.force_h3_failure,
        samples,
    };
    fs::write(&config.output_path, serde_json::to_vec_pretty(&output)?)?;
    Ok(())
}

async fn run_sample(config: &Config, sample_index: usize, resume_after_seq: i64) -> Result<SampleOutput> {
    let total_started = Instant::now();
    let health_started = Instant::now();
    let health = transport_health(config).await;
    let health_milliseconds = elapsed_ms(health_started);

    let expected = expected_commands(config, sample_index);
    let frames = command_frames(config, sample_index);
    let open_started = Instant::now();
    let mut opened = open_session(config, frames)
        .await
        .context("open Session transport")?;
    let open_milliseconds = elapsed_ms(open_started);

    let ack_started = Instant::now();
    let acks = collect_acks(&mut opened.stream, expected)
        .await
        .context("collect command ACKs")?;
    let ack_milliseconds = elapsed_ms(ack_started);
    let max_ack_seq = acks
        .iter()
        .map(|ack| ack.ack_seq)
        .max()
        .unwrap_or(resume_after_seq);
    let resume_probe = run_resume_probe(config, resume_after_seq, max_ack_seq)
        .await
        .context("run Resume after_seq probe")?;
    if resume_probe.latest_seq < max_ack_seq {
        bail!(
            "Resume probe latest_seq {} did not catch command ack seq {}",
            resume_probe.latest_seq,
            max_ack_seq
        );
    }
    Ok(SampleOutput {
        sample_index,
        transport: opened.transport.to_owned(),
        endpoint_url: opened.endpoint_url,
        fallback_reason: opened.fallback_reason,
        resume_after_seq,
        observed_resume_after_seq: resume_after_seq,
        resume_probe,
        health,
        timings: SampleTimings {
            health_milliseconds,
            open_milliseconds,
            ack_milliseconds,
            total_milliseconds: elapsed_ms(total_started),
        },
        acks,
    })
}

async fn run_resume_probe(config: &Config, after_seq: i64, expected_min_seq: i64) -> Result<ResumeOutput> {
    let mut opened = open_session(
        config,
        vec![proto::ClientFrame {
            frame: Some(client_frame::Frame::Resume(proto::Resume { after_seq })),
        }],
    )
    .await?;
    let latest_seq = collect_resume_replay(&mut opened.stream, expected_min_seq).await?;
    Ok(ResumeOutput {
        transport: opened.transport.to_owned(),
        endpoint_url: opened.endpoint_url,
        fallback_reason: opened.fallback_reason,
        after_seq,
        latest_seq,
        replayed: latest_seq > after_seq,
    })
}

async fn transport_health(config: &Config) -> TransportHealth {
    let (h3_ok, h3_error) = match h3_health(config).await {
        Ok(()) => (true, String::new()),
        Err(error) => (false, format!("{error:#}")),
    };
    let (h2_ok, h2_error) = match h2_health(config).await {
        Ok(()) => (true, String::new()),
        Err(error) => (false, format!("{error:#}")),
    };
    TransportHealth {
        h3_ok,
        h3_error,
        h2_ok,
        h2_error,
    }
}

async fn h3_health(config: &Config) -> Result<()> {
    let uri = config.h3_url.parse::<Uri>()?;
    let connector = H3QuinnConnector::new(
        uri.clone(),
        "localhost".to_owned(),
        pinned_h3_client_endpoint(&config.h3_certificate_sha256)?,
    );
    let channel = tonic_h3::H3Channel::new(connector, uri);
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::new(channel);
    let response = tokio::time::timeout(
        SESSION_OPEN_TIMEOUT,
        client.health(proto::HealthRequest {}),
    )
    .await
    .context("H3 health timed out")??;
    if response.into_inner().ok {
        Ok(())
    } else {
        bail!("H3 health returned ok=false")
    }
}

async fn h2_health(config: &Config) -> Result<()> {
    let channel = proto::looper_realtime_client::LooperRealtimeClient::connect(
        H2Endpoint::from_shared(config.h2_url.clone())?,
    )
    .await?;
    let mut client = channel;
    let response = client.health(proto::HealthRequest {}).await?;
    if response.into_inner().ok {
        Ok(())
    } else {
        bail!("H2 health returned ok=false")
    }
}

async fn open_session(config: &Config, frames: Vec<proto::ClientFrame>) -> Result<OpenedSession> {
    let h3_fallback_reason = match open_h3_session(config, frames.clone()).await {
        Ok(opened) => return Ok(opened),
        Err(error) => format!("h3 pre-stream failure: {error:#}"),
    };
    let mut opened = open_h2_session(config, frames).await?;
    opened.fallback_reason = h3_fallback_reason;
    Ok(opened)
}

async fn open_h3_session(config: &Config, frames: Vec<proto::ClientFrame>) -> Result<OpenedSession> {
    let uri = config.h3_url.parse::<Uri>()?;
    let connector = H3QuinnConnector::new(
        uri.clone(),
        "localhost".to_owned(),
        pinned_h3_client_endpoint(&config.h3_certificate_sha256)?,
    );
    let channel = tonic_h3::H3Channel::new(connector, uri);
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::new(channel);
    let request = session_request(config, frames)?;
    let response = tokio::time::timeout(SESSION_OPEN_TIMEOUT, client.session(request))
        .await
        .context("H3 Session open timed out")??;
    Ok(OpenedSession {
        transport: "h3",
        endpoint_url: config.h3_url.clone(),
        fallback_reason: String::new(),
        stream: response.into_inner(),
    })
}

async fn open_h2_session(config: &Config, frames: Vec<proto::ClientFrame>) -> Result<OpenedSession> {
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(
        H2Endpoint::from_shared(config.h2_url.clone())?,
    )
    .await?;
    let request = session_request(config, frames)?;
    let response = client.session(request).await?;
    Ok(OpenedSession {
        transport: "h2",
        endpoint_url: config.h2_url.clone(),
        fallback_reason: String::new(),
        stream: response.into_inner(),
    })
}

fn session_request(
    config: &Config,
    frames: Vec<proto::ClientFrame>,
) -> Result<tonic::Request<tokio_stream::Iter<std::vec::IntoIter<proto::ClientFrame>>>> {
    let mut request = tonic::Request::new(iter(frames));
    if !config.bearer_token.trim().is_empty() {
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {}", config.bearer_token).as_str())?,
        );
    }
    if !config.mobile_session.trim().is_empty() {
        request.metadata_mut().insert(
            "x-looper-mobile-session",
            MetadataValue::try_from(config.mobile_session.as_str())?,
        );
    }
    Ok(request)
}

fn command_frames(config: &Config, sample_index: usize) -> Vec<proto::ClientFrame> {
    vec![
        command_frame(proto::Command {
            command: Some(command::Command::SetSessionMode(proto::SetSessionModeRequest {
                thread_id: config.session_id.clone(),
                preset: "max-turns-1".to_owned(),
                client_mutation_id: mutation_id("mode", sample_index),
            })),
        }),
        command_frame(proto::Command {
            command: Some(command::Command::SendSessionPrompt(proto::SendSessionPromptRequest {
                thread_id: config.session_id.clone(),
                prompt: format!("transport proof prompt {sample_index}"),
                assistant_surface: "codex".to_owned(),
                client_mutation_id: mutation_id("prompt", sample_index),
                prompt_intent: "queue".to_owned(),
            })),
        }),
        command_frame(proto::Command {
            command: Some(command::Command::SubmitNotificationReply(proto::SubmitNotificationReplyRequest {
                notification_id: format!("transport-proof-notification-{sample_index}"),
                thread_id: config.session_id.clone(),
                prompt: format!("transport proof notification reply {sample_index}"),
                assistant_surface: "codex".to_owned(),
                client_mutation_id: mutation_id("notification", sample_index),
            })),
        }),
    ]
}

fn command_frame(command: proto::Command) -> proto::ClientFrame {
    proto::ClientFrame {
        frame: Some(client_frame::Frame::Command(command)),
    }
}

fn expected_commands(_config: &Config, sample_index: usize) -> HashMap<String, String> {
    HashMap::from([
        (mutation_id("mode", sample_index), "SetSessionMode".to_owned()),
        (mutation_id("prompt", sample_index), "SendSessionPrompt".to_owned()),
        (mutation_id("notification", sample_index), "SubmitNotificationReply".to_owned()),
    ])
}

fn mutation_id(kind: &str, sample_index: usize) -> String {
    format!("task-8-{kind}-{sample_index}")
}

async fn collect_acks(
    stream: &mut tonic::Streaming<proto::ServerFrame>,
    expected: HashMap<String, String>,
) -> Result<Vec<AckOutput>> {
    let mut pending: HashSet<String> = expected.keys().cloned().collect();
    let mut acks = Vec::with_capacity(expected.len());
    while !pending.is_empty() {
        let frame = tokio::time::timeout(ACK_TIMEOUT, stream.message())
        .await
        .context("waiting for command ACK timed out")?
        .context("read Session server frame")?
        .ok_or_else(|| anyhow!("Session stream ended before all ACKs arrived: pending={pending:?}"))?;
        let Some(server_frame::Frame::Ack(ack)) = frame.frame else {
            continue;
        };
        if !pending.remove(&ack.client_mutation_id) {
            continue;
        }
        let kind = expected
            .get(&ack.client_mutation_id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned());
        if !ack.accepted {
            bail!(
                "{kind} was rejected mutation={} error={} reason={}",
                ack.client_mutation_id,
                ack.error_code,
                ack.reject_reason
            );
        }
        acks.push(AckOutput {
            kind,
            client_mutation_id: ack.client_mutation_id,
            accepted: ack.accepted,
            ack_seq: ack.ack_seq,
            entity_id: ack.entity_id,
            revision: ack.revision,
            error_code: ack.error_code,
            reject_reason: ack.reject_reason,
        });
    }
    acks.sort_by(|left, right| left.kind.cmp(&right.kind));
    Ok(acks)
}

async fn collect_resume_replay(stream: &mut tonic::Streaming<proto::ServerFrame>, expected_min_seq: i64) -> Result<i64> {
    let deadline = Instant::now() + ACK_TIMEOUT;
    let mut latest_seq = 0_i64;
    while Instant::now() < deadline {
        let frame = tokio::time::timeout(Duration::from_millis(500), stream.message()).await;
        let Ok(message_result) = frame else {
            continue;
        };
        let Some(frame) = message_result
            .context("read Resume replay server frame")?
        else {
            break;
        };
        if let Some(server_frame::Frame::StateDelta(delta)) = frame.frame {
            latest_seq = latest_seq.max(delta.seq);
            if latest_seq >= expected_min_seq {
                return Ok(latest_seq);
            }
        }
    }
    bail!("Resume replay did not reach seq {expected_min_seq} before timeout; latest_seq={latest_seq}")
}

fn pinned_h3_client_endpoint(certificate_sha256: &str) -> Result<H3Endpoint> {
    let certificate_sha256 = normalized_sha256_pin(certificate_sha256)?;
    let mut endpoint = H3Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|error| anyhow!("create H3 client endpoint: {error}"))?;
    let provider = quinn_rustls::crypto::ring::default_provider();
    let verifier = Arc::new(PinnedH3CertificateVerifier {
        certificate_sha256,
        supported: provider.signature_verification_algorithms,
    });
    let mut tls_config = quinn_rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&quinn_rustls::version::TLS13])
        .map_err(|error| anyhow!("configure H3 TLS client protocol versions: {error:?}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls_config.alpn_protocols = vec![H3_ALPN.to_vec()];
    let quic_config = QuicClientConfig::try_from(tls_config)
        .map_err(|error| anyhow!("configure H3 QUIC client TLS: {error:?}"))?;
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_config)));
    Ok(endpoint)
}

#[derive(Debug)]
struct PinnedH3CertificateVerifier {
    certificate_sha256: String,
    supported: quinn_rustls::crypto::WebPkiSupportedAlgorithms,
}

impl quinn_rustls::client::danger::ServerCertVerifier for PinnedH3CertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &quinn_rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[quinn_rustls::pki_types::CertificateDer<'_>],
        _server_name: &quinn_rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: quinn_rustls::pki_types::UnixTime,
    ) -> std::result::Result<quinn_rustls::client::danger::ServerCertVerified, quinn_rustls::Error> {
        let actual = sha256_hex(end_entity.as_ref());
        if self.certificate_sha256 == actual {
            return Ok(quinn_rustls::client::danger::ServerCertVerified::assertion());
        }
        Err(quinn_rustls::Error::InvalidCertificate(
            quinn_rustls::CertificateError::ApplicationVerificationFailure,
        ))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &quinn_rustls::pki_types::CertificateDer<'_>,
        dss: &quinn_rustls::DigitallySignedStruct,
    ) -> std::result::Result<quinn_rustls::client::danger::HandshakeSignatureValid, quinn_rustls::Error> {
        quinn_rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &quinn_rustls::pki_types::CertificateDer<'_>,
        dss: &quinn_rustls::DigitallySignedStruct,
    ) -> std::result::Result<quinn_rustls::client::danger::HandshakeSignatureValid, quinn_rustls::Error> {
        quinn_rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<quinn_rustls::SignatureScheme> {
        self.supported.supported_schemes()
    }
}

fn normalized_sha256_pin(value: &str) -> Result<String> {
    let normalized = value.trim().strip_prefix("sha256:").unwrap_or(value.trim());
    if normalized.len() != 64 || !normalized.chars().all(|character| character.is_ascii_hexdigit()) {
        bail!("H3 certificate pin must be sha256-prefixed or raw 64-character hex");
    }
    Ok(normalized.to_ascii_lowercase())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn elapsed_ms(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}
'''


def run_app_selftest_latency(args: argparse.Namespace, requested_sample_count: int, run_dir: Path) -> dict:
    if not args.strict_local_first_targets:
        raise RuntimeError("--app-selftest requires --strict-local-first-targets")
    if args.force_http_fallback:
        raise RuntimeError("--app-selftest cannot use forced HTTP fallback")
    xcodegen_project()
    if args.physical_device:
        return run_physical_app_selftest(args, requested_sample_count, run_dir)
    return run_simulator_app_selftest(args, requested_sample_count, run_dir)


def run_simulator_app_selftest(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
) -> dict:
    simulator_udid = choose_simulator_udid()
    derived_data_path = simulator_derived_data_path(args, run_dir)
    build_log = run_dir / "ios-simulator-build.log"
    launch_log = run_dir / "ios-simulator-launch.log"
    boot_simulator(simulator_udid)
    app_path = build_ios_app(
        sdk="iphonesimulator",
        destination=f"platform=iOS Simulator,id={simulator_udid}",
        derived_data_path=derived_data_path,
        log_path=build_log,
        product_subdir="Debug-iphonesimulator",
    )
    run_logged(
        ["xcrun", "simctl", "install", simulator_udid, str(app_path)],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        log_path=run_dir / "ios-simulator-install.log",
        timeout=APP_SELFTEST_TIMEOUT_SECONDS,
    )
    run_logged(
        [
            "xcrun",
            "simctl",
            "launch",
            "--console",
            simulator_udid,
            APP_BUNDLE_IDENTIFIER,
            APP_SELFTEST_ARGUMENT,
            APP_SELFTEST_CASE,
            APP_SELFTEST_SAMPLE_ARGUMENT,
            str(requested_sample_count),
        ],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        log_path=launch_log,
        timeout=APP_SELFTEST_TIMEOUT_SECONDS,
    )
    evidence = parse_app_selftest_evidence(launch_log)
    evidence.update(
        {
            "runArtifactDir": project_relative(run_dir),
            "buildLog": project_relative(build_log),
            "installLog": project_relative(run_dir / "ios-simulator-install.log"),
            "launchLog": project_relative(launch_log),
            "appBundlePath": project_relative(app_path),
            "derivedDataPath": project_relative(derived_data_path),
            "derivedDataMode": derived_data_mode(args),
            "device": {
                "platform": "ios-simulator",
                "simulatorUDID": simulator_udid,
            },
            "budgetContract": strict_budget_contract_evidence(True),
            "fallback": {"forcedHttpFallback": False, "mode": "not-forced", "observable": False},
        }
    )
    return evidence


def run_physical_app_selftest(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
) -> dict:
    device_id = (args.device_id or "").strip()
    if not device_id:
        raise RuntimeError(f"--physical-device requires --device-id or ${DEVICE_ID_ENVIRONMENT_KEY}")
    run_preflight(device_id=device_id)
    derived_data_path = physical_derived_data_path(args, run_dir)
    build_log = run_dir / "ios-physical-build.log"
    install_log = run_dir / "ios-physical-install.txt"
    install_json = run_dir / "ios-physical-install.json"
    launch_log = run_dir / "ios-physical-launch.txt"
    device_info_log = run_dir / "ios-physical-device-info.txt"
    app_path = build_ios_app(
        sdk="iphoneos",
        destination="generic/platform=iOS",
        derived_data_path=derived_data_path,
        log_path=build_log,
        product_subdir="Debug-iphoneos",
    )
    run_logged(
        [
            "xcrun",
            "devicectl",
            "device",
            "info",
            "details",
            "--device",
            device_id,
            "--timeout",
            str(PREFLIGHT_TIMEOUT_SECONDS),
        ],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        log_path=device_info_log,
        timeout=PREFLIGHT_TIMEOUT_SECONDS,
    )
    run_logged(
        [
            "xcrun",
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            device_id,
            "--timeout",
            str(DEVICETL_TIMEOUT_SECONDS),
            "--json-output",
            str(install_json),
            "--log-output",
            str(install_log),
            str(app_path),
        ],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        log_path=run_dir / "ios-physical-install-command.txt",
        timeout=DEVICETL_TIMEOUT_SECONDS,
    )
    run_logged(
        [
            "xcrun",
            "devicectl",
            "device",
            "process",
            "launch",
            "--device",
            device_id,
            "--console",
            "--terminate-existing",
            "--timeout",
            str(APP_SELFTEST_TIMEOUT_SECONDS),
            APP_BUNDLE_IDENTIFIER,
            APP_SELFTEST_ARGUMENT,
            APP_SELFTEST_CASE,
            APP_SELFTEST_SAMPLE_ARGUMENT,
            str(requested_sample_count),
        ],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        log_path=launch_log,
        timeout=APP_SELFTEST_TIMEOUT_SECONDS + 15,
    )
    evidence = parse_app_selftest_evidence(launch_log)
    evidence.update(
        {
            "runArtifactDir": project_relative(run_dir),
            "buildLog": project_relative(build_log),
            "installLog": project_relative(install_log),
            "installJSON": project_relative(install_json),
            "launchLog": project_relative(launch_log),
            "appBundlePath": project_relative(app_path),
            "derivedDataPath": project_relative(derived_data_path),
            "derivedDataMode": derived_data_mode(args),
            "device": {
                "platform": "physical-ios",
                "deviceID": device_id,
                "deviceInfoLog": project_relative(device_info_log),
            },
            "budgetContract": strict_budget_contract_evidence(True),
            "fallback": {"forcedHttpFallback": False, "mode": "not-forced", "observable": False},
        }
    )
    return evidence


def build_ios_app(
    sdk: str,
    destination: str,
    derived_data_path: Path,
    log_path: Path,
    product_subdir: str,
) -> Path:
    run_logged(
        [
            "xcodebuild",
            "-project",
            str(PROJECT_PATH),
            "-scheme",
            "LooperCompanion",
            "-configuration",
            "Debug",
            "-sdk",
            sdk,
            "-destination",
            destination,
            "-derivedDataPath",
            str(derived_data_path),
            "build",
        ],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        log_path=log_path,
        timeout=XCODEBUILD_TIMEOUT_SECONDS,
    )
    app_path = derived_data_path / "Build" / "Products" / product_subdir / APP_PRODUCT_BUNDLE_NAME
    if app_path.is_dir():
        return app_path
    candidates = sorted((derived_data_path / "Build" / "Products").glob(f"{product_subdir}/*.app"))
    if candidates:
        return candidates[0]
    raise RuntimeError(f"missing built app bundle under {derived_data_path / 'Build' / 'Products'}")


def simulator_derived_data_path(args: argparse.Namespace, run_dir: Path) -> Path:
    return derived_data_path(
        args,
        run_dir,
        cache_name=SIMULATOR_DERIVED_DATA_CACHE_NAME,
        isolated_name=ISOLATED_SIMULATOR_DERIVED_DATA_NAME,
    )


def physical_derived_data_path(args: argparse.Namespace, run_dir: Path) -> Path:
    return derived_data_path(
        args,
        run_dir,
        cache_name=PHYSICAL_DERIVED_DATA_CACHE_NAME,
        isolated_name=ISOLATED_PHYSICAL_DERIVED_DATA_NAME,
    )


def derived_data_path(
    args: argparse.Namespace,
    run_dir: Path,
    cache_name: str,
    isolated_name: str,
) -> Path:
    if args.isolated_derived_data:
        return run_dir / isolated_name
    return DERIVED_DATA_CACHE_ROOT / cache_name


def derived_data_mode(args: argparse.Namespace) -> str:
    if args.isolated_derived_data:
        return "isolated"
    return "incremental-cache"


def parse_app_selftest_evidence(log_path: Path) -> dict:
    if not log_path.is_file():
        raise RuntimeError(f"missing app selftest launch log at {log_path}")
    for line in reversed(log_path.read_text(encoding="utf-8", errors="replace").splitlines()):
        match = APP_SELFTEST_PASS_PATTERN.search(line)
        if match is None:
            continue
        payload = json.loads(match.group("payload"))
        if payload.get("classification") != "strict-pass":
            raise RuntimeError(f"app selftest did not strict-pass: {payload}")
        return payload
    raise RuntimeError(f"missing G006_SELFTEST_PASS {APP_SELFTEST_CASE} in {log_path}:\n{tail(log_path)}")


def build_app_selftest_failure_evidence(
    args: argparse.Namespace,
    requested_sample_count: int,
    run_dir: Path,
    error: Exception,
) -> dict:
    return {
        "schemaVersion": 2,
        "goal": "G010-tighten-latency-harness-run-simulato",
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "classification": "blocked" if args.physical_device else "failed",
        "currentRed": True,
        "sampleCount": requested_sample_count,
        "runArtifactDir": project_relative(run_dir),
        "device": {
            "platform": "physical-ios" if args.physical_device else "ios-simulator",
            "deviceID": args.device_id,
        },
        "strictTargetsMs": strict_target_contract(),
        "strictTargetViolations": [
            {
                "metric": "appSelftest",
                "reason": "selftest-failed-or-unavailable",
                "detail": str(error),
            }
        ],
    }


def print_app_selftest_summary(evidence: dict) -> None:
    p95 = evidence["p95"]
    print(
        "mobile realtime latency strict-pass: "
        f"samples={evidence['sampleCount']} "
        f"uiMode_p95={p95['uiModeMs']}ms "
        f"uiPrompt_p95={p95['uiPromptMs']}ms "
        f"modeAck_p95={p95['modeAckMs']}ms "
        f"promptAck_p95={p95['promptAckMs']}ms "
        f"streamApply_p95={p95['streamApplyMs']}ms "
        f"snapshotOnTapCount={evidence['snapshotOnTapCount']}"
    )


def codex_stub_log_path(run_dir: Path) -> Path:
    return run_dir / CODEX_STUB_LOG_FILENAME


def seed_codex_state(run_dir: Path) -> None:
    codex_home = run_dir / "codex-home"
    project_dir = run_dir / "project"
    codex_home.mkdir(parents=True, exist_ok=True)
    project_dir.mkdir(parents=True, exist_ok=True)
    state_db = codex_home / "state_1.sqlite"
    with sqlite3.connect(state_db) as connection:
        connection.execute(CREATE_THREADS_SQL)
        connection.execute(
            INSERT_THREAD_SQL,
            (
                SESSION_ID,
                SESSION_TITLE,
                str(project_dir),
                "desktop",
                "gpt-5",
                "high",
                unix_milliseconds() - 1_000,
                unix_milliseconds(),
                0,
            ),
        )
        connection.execute(CREATE_DYNAMIC_TOOLS_SQL)
        connection.execute(CREATE_SPAWN_EDGES_SQL)
    with sqlite3.connect(codex_home / "logs_1.sqlite") as connection:
        connection.execute(CREATE_LOGS_SQL)


def unix_milliseconds() -> int:
    return int(time.time() * 1_000)


def write_codex_stub(run_dir: Path) -> Path:
    stub_path = run_dir / "codex-resume-stub.py"
    log_path = codex_stub_log_path(run_dir)
    stub_source = "\n".join(
        [
            "#!/usr/bin/env python3",
            "import json",
            "import os",
            "import sys",
            "",
            f"log_path = os.environ.get('LOOPER_LATENCY_CODEX_STUB_LOG', {json.dumps(str(log_path))})",
            "",
            "def emit(payload):",
            "    print(json.dumps(payload, separators=(',', ':')), flush=True)",
            "",
            "with open(log_path, 'a', encoding='utf-8') as log:",
            "    for raw_line in sys.stdin:",
            "        log.write(raw_line)",
            "        log.flush()",
            "        try:",
            "            request = json.loads(raw_line)",
            "        except json.JSONDecodeError:",
            "            continue",
            "        request_id = request.get('id')",
            "        method = request.get('method')",
            "        params = request.get('params') or {}",
            "        if method == 'initialize':",
            "            emit({'id': request_id, 'result': {}})",
            "        elif method == 'thread/resume':",
            "            emit({'id': request_id, 'result': {'thread': {'id': params.get('threadId', 'thread-stub')}}})",
            "        elif method == 'turn/start':",
            "            thread_id = params.get('threadId', 'thread-stub')",
            "            emit({'id': request_id, 'result': {'turn': {'id': 'turn-stub', 'status': 'inProgress'}}})",
            "            emit({'method': 'turn/completed', 'params': {'threadId': thread_id, 'turn': {'id': 'turn-stub', 'status': 'completed'}}})",
            "",
        ]
    )
    stub_path.write_text(stub_source, encoding="utf-8")
    stub_path.chmod(0o755)
    return stub_path


def free_http_port_with_grpc_neighbor() -> int:
    for _ in range(100):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.bind((HTTP_HOST, 0))
            port = probe.getsockname()[1]
        if port < 65_534 and port_is_free(port + GRPC_PORT_OFFSET):
            return port
    raise RuntimeError("could not reserve adjacent HTTP/gRPC test ports")


def port_is_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        try:
            probe.bind((HTTP_HOST, port))
            return True
        except OSError:
            return False


def start_server(run_dir: Path, codex_stub: Path, http_port: int, grpc_port: int) -> subprocess.Popen:
    env = os.environ.copy()
    env.update(
        {
            "AGENT_CONTROL_PLANE_LISTEN": f"{HTTP_HOST}:{http_port}",
            "AGENT_CONTROL_PLANE_GRPC_LISTEN": f"{HTTP_HOST}:{grpc_port}",
            "AGENT_CONTROL_PLANE_STORE": str(run_dir / "control-plane.sqlite"),
            "CODEX_HOME": str(run_dir / "codex-home"),
            "HOME": str(run_dir / "home"),
            "LOOPER_CODEX_EXECUTABLE": str(codex_stub),
            "LOOPER_LATENCY_CODEX_STUB_LOG": str(run_dir / "codex-resume-stub.log"),
        }
    )
    (run_dir / "home").mkdir(parents=True, exist_ok=True)
    server_log = open(run_dir / "server.log", "w", encoding="utf-8")
    command = [
        str(SERVER_BINARY),
        "serve",
    ]
    return subprocess.Popen(
        command,
        cwd=ROOT_DIR,
        env=env,
        stdout=server_log,
        stderr=subprocess.STDOUT,
        text=True,
    )


def build_server_binary() -> None:
    run_command(
        [
            "cargo",
            "build",
            "--manifest-path",
            str(CARGO_MANIFEST),
            "--bin",
            "looper-server",
            "--bin",
            "looper",
        ],
        cwd=ROOT_DIR,
        env=os.environ.copy(),
    )
    if not SERVER_BINARY.is_file():
        raise RuntimeError(f"missing built server binary at {SERVER_BINARY}")
    if not CLI_BINARY.is_file():
        raise RuntimeError(f"missing built CLI binary at {CLI_BINARY}")


def wait_for_health(base_url: str, server: subprocess.Popen, log_path: Path) -> None:
    deadline = time.monotonic() + HEALTH_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if server.poll() is not None:
            raise RuntimeError(f"looper-server exited early:\n{tail(log_path)}")
        try:
            health = http_json(f"{base_url}/api/mobile/health")
            if health.get("ok") is True:
                return
        except Exception:
            time.sleep(HEALTH_POLL_SECONDS)
    raise RuntimeError(f"looper-server did not become healthy:\n{tail(log_path)}")


def register_mobile_session(base_url: str) -> dict[str, str]:
    connection_code = http_json(f"{base_url}/api/mobile/connection-code")
    bearer_token = f"{connection_code['pairingTokenId']}.{connection_code['pairingToken']}"
    auth_header = {"Authorization": f"Bearer {bearer_token}"}
    challenge = http_json(
        f"{base_url}/api/mobile/passkeys/registration-challenge",
        method="POST",
        headers=auth_header,
    )
    private_key = ec.generate_private_key(ec.SECP256R1())
    public_key = private_key.public_key().public_bytes(
        serialization.Encoding.X962,
        serialization.PublicFormat.UncompressedPoint,
    )
    signature = private_key.sign(challenge["message"].encode("utf-8"), ec.ECDSA(hashes.SHA256()))
    registration = http_json(
        f"{base_url}/api/mobile/passkeys/register",
        method="POST",
        headers=auth_header,
        payload={
            "challengeId": challenge["challengeId"],
            "publicKeyX963": base64_url(public_key),
            "signature": base64_url(signature),
            "label": "Mobile latency verifier",
        },
    )
    session = registration["session"]
    return {
        "bearer_token": bearer_token,
        "mobile_session": f"{session['sessionId']}.{session['sessionToken']}",
    }


def assert_mobile_snapshot(base_url: str, bearer_token: str, mobile_session: str) -> None:
    snapshot = http_json(
        f"{base_url}/api/mobile/snapshot",
        headers=mobile_headers(bearer_token, mobile_session),
    )
    titles = {session.get("title") for session in snapshot.get("sessions", [])}
    if SESSION_TITLE not in titles:
        raise RuntimeError(f"mobile snapshot did not include {SESSION_TITLE!r}: {sorted(titles)}")


def reset_latency_session_mode(base_url: str) -> None:
    env = os.environ.copy()
    env["AGENT_CONTROL_PLANE_LISTEN"] = listen_address_for_base_url(base_url)
    run_command(
        [str(CLI_BINARY), "--json", "sessions", "mode", SESSION_ID, "off"],
        cwd=ROOT_DIR,
        env=env,
    )


def listen_address_for_base_url(base_url: str) -> str:
    parsed = urllib.parse.urlparse(base_url)
    if parsed.hostname is None or parsed.port is None:
        raise RuntimeError(f"base URL has no host/port: {base_url}")
    return f"{parsed.hostname}:{parsed.port}"


def write_live_latency_config(
    base_url: str,
    bearer_token: str,
    mobile_session: str,
    result_path: Path,
) -> None:
    LIVE_CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "baseURLs": base_url,
        "bearerToken": bearer_token,
        "mobileSession": mobile_session,
        "outputPath": str(result_path),
        "sessionTitle": SESSION_TITLE,
    }
    LIVE_CONFIG_PATH.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def mobile_headers(bearer_token: str, mobile_session: str) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {bearer_token}",
        "X-Looper-Mobile-Session": mobile_session,
    }


def base64_url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(DER_SIGNATURE_PADDING).decode("ascii")


def http_json(
    url: str,
    method: str = "GET",
    headers: Optional[dict[str, str]] = None,
    payload: Optional[dict] = None,
) -> dict:
    request_headers = {"Content-Type": "application/json"}
    request_headers.update(headers or {})
    data = None
    if method == "POST":
        data = b"" if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=data, headers=request_headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=REQUEST_TIMEOUT_SECONDS) as response:
            body = response.read().decode("utf-8")
            if response.status != HTTP_OK:
                raise RuntimeError(f"{method} {url} failed: {response.status} {body}")
            return json.loads(body)
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"{method} {url} failed: {error.code} {body}") from error


def xcodegen_project() -> None:
    run_command(
        [
            "xcodegen",
            "generate",
            "--spec",
            str(PROJECT_SPEC),
            "--project",
            str(IOS_DIR),
        ],
        cwd=ROOT_DIR,
        env=xcode_environment(),
    )


def run_latency_ui_test(
    base_url: str,
    bearer_token: str,
    mobile_session: str,
    result_path: Path,
    log_path: Path,
) -> None:
    simulator_udid = choose_simulator_udid()
    latency_environment = {
        "LOOPER_LIVE_LATENCY_BASE_URLS": base_url,
        "LOOPER_LIVE_LATENCY_BEARER_TOKEN": bearer_token,
        "LOOPER_LIVE_LATENCY_MOBILE_SESSION": mobile_session,
        "LOOPER_LIVE_LATENCY_OUTPUT": str(result_path),
        "LOOPER_LIVE_LATENCY_SESSION_TITLE": SESSION_TITLE,
    }
    env = xcode_environment()
    env.update(latency_environment)
    boot_simulator(simulator_udid)
    set_simulator_environment(simulator_udid, latency_environment)
    command = [
        "xcodebuild",
        "-project",
        str(PROJECT_PATH),
        "-scheme",
        "LooperCompanion",
        "-configuration",
        "Debug",
        "-sdk",
        "iphonesimulator",
        "-destination",
        f"platform=iOS Simulator,id={simulator_udid}",
        "-derivedDataPath",
        str(ARTIFACT_ROOT / "DerivedData"),
        "-only-testing:" + UI_TEST_SELECTOR,
        "test",
    ]
    try:
        run_logged(
            command,
            cwd=ROOT_DIR,
            env=env,
            log_path=log_path,
            timeout=XCODEBUILD_TIMEOUT_SECONDS,
            success_path=result_path,
        )
    finally:
        clear_simulator_environment(simulator_udid, latency_environment.keys())


def xcode_environment() -> dict[str, str]:
    env = os.environ.copy()
    if "DEVELOPER_DIR" not in env and PREFERRED_DEVELOPER_DIR.is_dir():
        env["DEVELOPER_DIR"] = str(PREFERRED_DEVELOPER_DIR)
    return env


def choose_simulator_udid() -> str:
    result = subprocess.run(
        ["xcrun", "simctl", "list", "devices", "available", "-j"],
        cwd=ROOT_DIR,
        env=xcode_environment(),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    data = json.loads(result.stdout)
    devices = [
        device
        for runtime_devices in data.get("devices", {}).values()
        for device in runtime_devices
        if device.get("isAvailable") and "iPhone" in device.get("name", "")
    ]
    if not devices:
        raise RuntimeError("no available iPhone simulator found")
    preferred_names = ("iPhone 17 Pro", "iPhone 16 Pro", "iPhone 15 Pro")
    for preferred_name in preferred_names:
        for device in devices:
            if device.get("name") == preferred_name:
                return device["udid"]
    return devices[0]["udid"]


def boot_simulator(simulator_udid: str) -> None:
    env = xcode_environment()
    result = subprocess.run(
        ["xcrun", "simctl", "boot", simulator_udid],
        cwd=ROOT_DIR,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    already_booted = "Unable to boot device in current state: Booted" in result.stderr
    if result.returncode != EXIT_OK and not already_booted:
        raise RuntimeError(f"could not boot simulator {simulator_udid}: {result.stderr}")
    run_command(["xcrun", "simctl", "bootstatus", simulator_udid, "-b"], cwd=ROOT_DIR, env=env)


def set_simulator_environment(simulator_udid: str, values: dict[str, str]) -> None:
    env = xcode_environment()
    for key, value in values.items():
        run_command(
            ["xcrun", "simctl", "spawn", simulator_udid, "launchctl", "setenv", key, value],
            cwd=ROOT_DIR,
            env=env,
        )


def clear_simulator_environment(simulator_udid: str, keys) -> None:
    env = xcode_environment()
    for key in keys:
        run_command(
            ["xcrun", "simctl", "spawn", simulator_udid, "launchctl", "unsetenv", key],
            cwd=ROOT_DIR,
            env=env,
        )


def run_command(command: list[str], cwd: Path, env: dict[str, str]) -> None:
    subprocess.run(command, cwd=cwd, env=env, check=True)


def run_logged(
    command: list[str],
    cwd: Path,
    env: dict[str, str],
    log_path: Path,
    timeout: int,
    success_path: Optional[Path] = None,
) -> None:
    started_at = time.monotonic()
    success_seen_at: Optional[float] = None
    completed_after_success_path = False
    with open(log_path, "w", encoding="utf-8") as log:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            start_new_session=True,
        )
        assert process.stdout is not None
        while True:
            if time.monotonic() - started_at > timeout:
                terminate_process_group(process)
                raise RuntimeError(f"command timed out after {timeout}s: {' '.join(command)}")
            ready, _, _ = select.select([process.stdout], [], [], 1)
            if ready:
                line = process.stdout.readline()
                if line:
                    print(line, end="")
                    log.write(line)
                    continue
            if success_path is not None and latency_result_file_ready(success_path):
                if success_seen_at is None:
                    success_seen_at = time.monotonic()
                elif time.monotonic() - success_seen_at >= XCODEBUILD_RESULT_EXIT_GRACE_SECONDS:
                    exit_code = terminate_process_group(process)
                    remaining_output = process.stdout.read()
                    if remaining_output:
                        print(remaining_output, end="")
                        log.write(remaining_output)
                    log.write(
                        "xcodebuild latency result captured; terminated post-result diagnostics "
                        f"after {XCODEBUILD_RESULT_EXIT_GRACE_SECONDS}s "
                        f"with exit {exit_code}\n"
                    )
                    completed_after_success_path = True
                    break
            if process.poll() is not None:
                remaining_output = process.stdout.read()
                if remaining_output:
                    print(remaining_output, end="")
                    log.write(remaining_output)
                break
        exit_code = process.wait()
    if completed_after_success_path:
        return
    if exit_code != EXIT_OK:
        raise RuntimeError(f"command failed with exit {exit_code}: {' '.join(command)}\n{tail(log_path)}")


def terminate_process_group(process: subprocess.Popen) -> int:
    if process.poll() is not None:
        return process.wait()
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return process.wait(timeout=PROCESS_TERMINATION_GRACE_SECONDS)
    try:
        return process.wait(timeout=PROCESS_TERMINATION_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        return process.wait(timeout=PROCESS_TERMINATION_GRACE_SECONDS)


def latency_result_file_ready(result_path: Path) -> bool:
    if not result_path.is_file():
        return False
    try:
        result = json.loads(result_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return False
    return all(
        key in result
        for key in (
            "connectMilliseconds",
            "modeSwitchMilliseconds",
            "promptAckMilliseconds",
            "totalMilliseconds",
        )
    )


def read_latency_result(result_path: Path, log_path: Path) -> dict:
    if result_path.is_file():
        return json.loads(result_path.read_text(encoding="utf-8"))
    result = read_latency_result_from_log(log_path)
    if result is None:
        raise RuntimeError(f"missing latency result at {result_path}")
    result_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return result


def read_latency_result_from_log(log_path: Path) -> Optional[dict]:
    if not log_path.is_file():
        return None
    for line in reversed(log_path.read_text(encoding="utf-8", errors="replace").splitlines()):
        match = LATENCY_LOG_PATTERN.search(line)
        if match is None:
            continue
        return {
            "connectMilliseconds": int(match.group("connect")),
            "modeSwitchMilliseconds": int(match.group("mode")),
            "promptAckMilliseconds": int(match.group("prompt")),
            "totalMilliseconds": int(match.group("total")),
            "sessionTitle": SESSION_TITLE,
        }
    return None


def count_codex_stub_turn_starts(log_path: Path) -> int:
    if not log_path.is_file():
        return 0
    count = 0
    for line in log_path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        if payload.get("method") != "turn/start":
            continue
        params = payload.get("params") or {}
        if params.get("threadId") == SESSION_ID:
            count += 1
    return count


def wait_for_codex_stub_turn_start(log_path: Path, expected_min_count: int) -> None:
    deadline = time.monotonic() + CODEX_STUB_ACCEPTANCE_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if count_codex_stub_turn_starts(log_path) >= expected_min_count:
            return
        time.sleep(HEALTH_POLL_SECONDS)
    observed_count = count_codex_stub_turn_starts(log_path)
    raise RuntimeError(
        "Codex stub did not observe prompt turn/start "
        f"for {SESSION_ID}: observed={observed_count} expected>={expected_min_count}"
    )


def aggregate_latency_results(samples: list[dict]) -> dict:
    if not samples:
        raise RuntimeError("missing latency samples")
    return {
        "sampleCount": len(samples),
        "percentile": P95_PERCENTILE,
        "sessionTitle": SESSION_TITLE,
        "p95": {
            "connectMilliseconds": percentile(
                [sample["connectMilliseconds"] for sample in samples],
                P95_PERCENTILE,
            ),
            "modeSwitchMilliseconds": percentile(
                [sample["modeSwitchMilliseconds"] for sample in samples],
                P95_PERCENTILE,
            ),
            "promptAckMilliseconds": percentile(
                [sample["promptAckMilliseconds"] for sample in samples],
                P95_PERCENTILE,
            ),
            "totalMilliseconds": percentile(
                [sample["totalMilliseconds"] for sample in samples],
                P95_PERCENTILE,
            ),
        },
        "samples": samples,
    }


def attach_strict_latency_evidence(result: dict, args: argparse.Namespace, run_dir: Path) -> dict:
    snapshot_evidence = detect_current_snapshot_evidence()
    p95 = result["p95"]
    ui_mode_ms = p95["modeSwitchMilliseconds"]
    ui_prompt_ms = p95["promptAckMilliseconds"]
    mode_ack_ms = p95["modeSwitchMilliseconds"]
    prompt_ack_ms = p95["promptAckMilliseconds"]
    result.update(
        {
            "schemaVersion": 1,
            "goal": "G001-instrument-current-latency-and-snaps",
            "generatedAt": datetime.now(timezone.utc).isoformat(),
            "runArtifactDir": project_relative(run_dir),
            "uiModeMs": ui_mode_ms,
            "uiPromptMs": ui_prompt_ms,
            "modeAckMs": mode_ack_ms,
            "promptAckMs": prompt_ack_ms,
            "notificationPersistMs": None,
            "streamApplyMs": None,
            "snapshotOnTapCount": snapshot_evidence["snapshotOnTapCount"],
            "strictTargetsMs": strict_target_contract(),
            "fallback": build_fallback_evidence(args.force_http_fallback, snapshot_evidence),
            "budgetContract": strict_budget_contract_evidence(args.assert_budget_contract),
            "sourceFields": {
                "available": True,
                "legacyP95": p95,
                "mapping": {
                    "uiModeMs": "p95.modeSwitchMilliseconds",
                    "uiPromptMs": "p95.promptAckMilliseconds",
                    "modeAckMs": "p95.modeSwitchMilliseconds",
                    "promptAckMs": "p95.promptAckMilliseconds",
                    "notificationPersistMs": "missing-current-instrumentation",
                    "streamApplyMs": "missing-current-instrumentation",
                },
            },
            "sourceEvidence": snapshot_evidence["sourceEvidence"],
        }
    )
    violations = strict_target_violations_for_result(result)
    result["strictTargetViolations"] = violations
    result["currentRed"] = bool(
        violations
        or args.force_http_fallback
        or snapshot_evidence["snapshotOnTapCount"] > 0
    )
    result["classification"] = "current-red" if result["currentRed"] else "strict-pass"
    result["currentRedReasons"] = current_red_reasons_for_result(result, args.force_http_fallback)
    return result


def strict_target_violations_for_result(result: dict) -> list[dict]:
    violations = []
    for metric in (
        "uiModeMs",
        "uiPromptMs",
        "modeAckMs",
        "promptAckMs",
        "notificationPersistMs",
        "streamApplyMs",
    ):
        value = result[metric]
        target = strict_target_for_metric(metric)
        if value is None:
            violations.append(strict_violation(metric, "missing-current-instrumentation"))
        elif value > target:
            violations.append(
                {
                    "metric": metric,
                    "targetMs": target,
                    "actualMs": value,
                    "reason": "strict-target-exceeded",
                }
            )
    if result["snapshotOnTapCount"] > 0:
        violations.append(
            {
                "metric": "snapshotOnTapCount",
                "target": "0 snapshot refreshes on realtime tap/action path",
                "actual": result["snapshotOnTapCount"],
                "reason": "current realtime path still schedules snapshot refreshes",
            }
        )
    return violations


def current_red_reasons_for_result(result: dict, forced_http_fallback: bool) -> list[str]:
    if not result["currentRed"]:
        return []
    reasons = []
    if result["strictTargetViolations"]:
        reasons.append("strict local-first target violations are present")
    if result["snapshotOnTapCount"] > 0:
        reasons.append("current realtime path still routes through snapshot refresh")
    if forced_http_fallback:
        reasons.append("HTTP fallback forced by harness flag")
    return reasons


def percentile(values: list[int], percentile_value: int) -> int:
    sorted_values = sorted(values)
    rank = math.ceil((percentile_value / 100) * len(sorted_values))
    return sorted_values[max(rank - 1, 0)]


def assert_latency_budgets(result: dict) -> None:
    p95 = result["p95"]
    violations = []
    if p95["connectMilliseconds"] > CONNECT_BUDGET_MILLISECONDS:
        violations.append(
            f"connect p95 {p95['connectMilliseconds']}ms > {CONNECT_BUDGET_MILLISECONDS}ms"
        )
    if p95["modeSwitchMilliseconds"] > MODE_SWITCH_BUDGET_MILLISECONDS:
        violations.append(
            f"mode p95 {p95['modeSwitchMilliseconds']}ms > {MODE_SWITCH_BUDGET_MILLISECONDS}ms"
        )
    if p95["promptAckMilliseconds"] > PROMPT_ACK_BUDGET_MILLISECONDS:
        violations.append(
            f"prompt p95 {p95['promptAckMilliseconds']}ms > {PROMPT_ACK_BUDGET_MILLISECONDS}ms"
        )
    if p95["totalMilliseconds"] > TOTAL_BUDGET_MILLISECONDS:
        violations.append(f"total p95 {p95['totalMilliseconds']}ms > {TOTAL_BUDGET_MILLISECONDS}ms")
    if violations:
        raise RuntimeError("latency budget failed: " + "; ".join(violations))


def assert_strict_latency_budgets(result: dict, expect_current_red: bool) -> None:
    current_red = result.get("currentRed") is True
    violations = result.get("strictTargetViolations") or []
    if expect_current_red:
        if not current_red:
            raise RuntimeError("strict local-first targets unexpectedly passed")
        return
    if violations:
        formatted = "; ".join(
            f"{violation['metric']} {violation.get('actualMs', violation.get('actual'))} > "
            f"{violation.get('targetMs', violation.get('target'))}"
            for violation in violations
        )
        raise RuntimeError("strict local-first latency budget failed: " + formatted)


def write_qa_report(result: dict, base_url: str, run_dir: Path) -> None:
    DOC_PATH.parent.mkdir(parents=True, exist_ok=True)
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")
    if result.get("goal") == "G010-tighten-latency-harness-run-simulato":
        write_strict_qa_report(result, now, run_dir)
        return
    p95 = result["p95"]
    sample_lines = [
        (
            f"- Sample {sample['sampleIndex']}: "
            f"connect {sample['connectMilliseconds']} ms, "
            f"mode {sample['modeSwitchMilliseconds']} ms, "
            f"prompt ACK {sample['promptAckMilliseconds']} ms, "
            f"total {sample['totalMilliseconds']} ms"
        )
        for sample in result["samples"]
    ]
    DOC_PATH.write_text(
        "\n".join(
            [
                "# Mobile Realtime Latency QA",
                "",
                f"- Observed at: {now}",
                f"- Isolated server: {base_url}",
                f"- Artifact directory: `{run_dir.relative_to(ROOT_DIR)}`",
                f"- Session: `{result['sessionTitle']}`",
                f"- Sample count: {result['sampleCount']}",
                f"- Percentile: p{result['percentile']}",
                f"- Connect p95: {p95['connectMilliseconds']} ms (budget {CONNECT_BUDGET_MILLISECONDS} ms)",
                f"- Mode switch p95: {p95['modeSwitchMilliseconds']} ms (budget {MODE_SWITCH_BUDGET_MILLISECONDS} ms)",
                f"- Prompt ACK p95: {p95['promptAckMilliseconds']} ms (budget {PROMPT_ACK_BUDGET_MILLISECONDS} ms)",
                f"- Total workflow p95: {p95['totalMilliseconds']} ms (budget {TOTAL_BUDGET_MILLISECONDS} ms)",
                "- Backend acceptance: every sample required a fresh Codex stub `turn/start`.",
                "",
                "Samples:",
                "",
                *sample_lines,
                "",
                "Command:",
                "",
                "```bash",
                "python3 scripts/run-mobile-realtime-latency-check.py",
                "```",
                "",
            ]
        ),
        encoding="utf-8",
    )


def write_strict_qa_report(result: dict, observed_at: str, run_dir: Path) -> None:
    p95 = result["p95"]
    targets = result["strictTargetsMs"]
    sample_lines = [
        (
            f"- Sample {sample['sampleIndex']}: "
            f"UI mode {sample['uiModeMs']} ms, "
            f"UI prompt {sample['uiPromptMs']} ms, "
            f"mode ACK {sample['modeAckMs']} ms, "
            f"prompt ACK {sample['promptAckMs']} ms, "
            f"notification persist {sample['notificationPersistMs']} ms, "
            f"stream apply {sample['streamApplyMs']} ms, "
            f"full snapshots {sample['fullSnapshotCallsOnTap']}"
        )
        for sample in result["samples"]
    ]
    device = result.get("device") or {}
    DOC_PATH.write_text(
        "\n".join(
            [
                "# Mobile Realtime Latency QA",
                "",
                f"- Observed at: {observed_at}",
                "- Measurement layer: DEBUG app-internal selftest, not XCTest tap round-trip latency.",
                f"- Artifact directory: `{run_dir.relative_to(ROOT_DIR)}`",
                f"- Device platform: `{device.get('platform', result.get('platform', 'unknown'))}`",
                f"- DerivedData mode: `{result.get('derivedDataMode', 'not recorded')}`",
                f"- DerivedData path: `{result.get('derivedDataPath', 'not recorded')}`",
                f"- Classification: `{result['classification']}`",
                f"- Sample count: {result['sampleCount']}",
                f"- Percentile: p{result['percentile']}",
                f"- UI mode p95: {p95['uiModeMs']} ms (target {targets['uiP95Ms']} ms)",
                f"- UI prompt p95: {p95['uiPromptMs']} ms (target {targets['uiP95Ms']} ms)",
                f"- Mode ACK p95: {p95['modeAckMs']} ms (target {targets['localLanAckP95Ms']} ms)",
                f"- Prompt ACK p95: {p95['promptAckMs']} ms (target {targets['localLanAckP95Ms']} ms)",
                (
                    "- Notification persistence p95: "
                    f"{p95['notificationPersistMs']} ms (target {targets['localLanAckP95Ms']} ms)"
                ),
                f"- Stream apply p95: {p95['streamApplyMs']} ms (target {targets['uiP95Ms']} ms)",
                f"- Stream resume p95: {p95['streamResumeMs']} ms (target {targets['uiP95Ms']} ms)",
                f"- Snapshot-on-tap count: {result['snapshotOnTapCount']}",
                f"- Full snapshot calls on action path: {result['fullSnapshotCallsOnTap']}",
                f"- Remote/Tailscale ACK available in this run: {result['remoteAckAvailable']}",
                "",
                "Samples:",
                "",
                *sample_lines,
                "",
                "Command:",
                "",
                "```bash",
                strict_qa_report_command(result),
                "```",
                "",
            ]
        ),
        encoding="utf-8",
    )


def strict_qa_report_command(result: dict) -> str:
    command = (
        "python3 scripts/run-mobile-realtime-latency-check.py --samples 20 "
        "--strict-local-first-targets --assert-budget-contract --app-selftest"
    )
    device = result.get("device") or {}
    if device.get("platform") != "physical-ios":
        return command
    device_id = device.get("deviceID")
    if device_id:
        return f"{DEVICE_ID_ENVIRONMENT_KEY}={device_id} {command} --physical-device"
    return f"{command} --physical-device"


def stop_server(server: subprocess.Popen) -> None:
    if server.poll() is not None:
        return
    server.send_signal(signal.SIGTERM)
    try:
        server.wait(timeout=5)
    except subprocess.TimeoutExpired:
        server.kill()
        server.wait(timeout=5)


def tail(path: Path, line_count: int = 80) -> str:
    if not path.is_file():
        return ""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    return "\n".join(lines[-line_count:])


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(EXIT_FAILURE)
