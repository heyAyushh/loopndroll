#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
CLIENT_CORE_MANIFEST = ROOT_DIR / "crates" / "looper-client-core" / "Cargo.toml"
CLIENT_CORE_ROOT = Path("crates/looper-client-core")
RUNTIME_ROOTS = ("crates", "ios", "macos", "swift")
CLIENT_RUNTIME_ROOTS = ("ios", "macos", "swift")
RETIRED_RUNTIME_ROOTS = (Path("swift/LooperRealtime"),)
RAW_CLIENT_CORE_BOUNDARY_FILES = (
    CLIENT_CORE_ROOT / "src" / "client.rs",
    CLIENT_CORE_ROOT / "src" / "local_store.rs",
)

FORBIDDEN_DEPENDENCIES = {
    "agent-control-plane",
    "axum",
    "orb-code",
    "reqwest",
    "rusqlite",
    "tailscale-localapi",
}

CLIENT_CORE_SOURCE_PATTERNS = (
    (
        "legacy event-stream transport",
        re.compile(
            r"\b(SubscribeMobileEvents|SubscribeDesktopEvents|MobileEventStream|"
            r"DesktopEventStreamCoordinator|text/event-stream|EventSource|"
            r"Server-Sent Events|SSE)\b"
        ),
    ),
    (
        "legacy unary command response",
        re.compile(
            r"\b(SetSessionModeResponse|SendSessionPromptResponse|"
            r"SubmitNotificationReplyResponse)\b"
        ),
    ),
    (
        "local truth ownership",
        re.compile(
            r"\b(rusqlite|sqlite|hooks\.json|settings\.json|"
            r"~/.codex|~/.claude|~/.grok|~/.config/devin)\b"
        ),
    ),
)

STRICT_RUNTIME_PATTERNS = (
    (
        "forbidden rewrite-only runtime surface",
        re.compile(
            r"SubscribeMobileEvents|SubscribeDesktopEvents|SubscribeEventsRequest|"
            r"MobileEventStream|CompanionRealtimeController|CompanionRealtimeSync|"
            r"DesktopEventStreamCoordinator|text/event-stream|"
            r"Server-Sent Events|\bSSE\b|\bSse\b|/api/mobile/events|/desktop/events|"
            r"/events/tail|"
            r"/api/mobile/sessions/[^\"']+/(mode|prompt)|"
            r"/desktop/sessions/[^\"']+/(mode|prompt|notification-reply)|"
            r"/desktop/session-prompts|streamMobileEvents|streamDesktopEvents|"
            r"SetSessionModeResponse|SendSessionPromptResponse|"
            r"SubmitNotificationReplyResponse|mobile_event_sse_name"
        ),
    ),
)

CLIENT_RUNTIME_PATTERNS = (
    (
        "client-side reducer ownership",
        re.compile(r"\b[A-Za-z0-9_]*Reducer[A-Za-z0-9_]*\b|\breducer\b"),
    ),
    (
        "raw client-core foreign object",
        re.compile(
            r"\bpublic\s+protocol\s+LooperClientCoreProtocol\b|"
            r"\bopen\s+class\s+LooperClientCore\b|"
            r"\bLooperClientCoreLocalStore\b|"
            r"\bLooperClientCore\s*\("
        ),
    ),
    (
        "retired LooperRealtime production bridge",
        re.compile(
            r"^\s*import\s+LooperRealtime\b|"
            r"\bpackage:\s*LooperRealtime\b|"
            r"LooperRealtime in Frameworks|"
            r"XCLocalSwiftPackageReference \"(?:\.\./)+swift/LooperRealtime\"|"
            r"\bproductName = LooperRealtime\b"
        ),
    ),
)

CLIENT_APP_OWNERSHIP_PATTERNS = (
    (
        "app-owned raw session observe",
        re.compile(r"\bsessionManager\.observe\("),
    ),
    (
        "retired Swift reply-drain ownership",
        re.compile(
            r"notificationReplyMakeClientMutationID|notificationReplyOutboxDrainID|"
            r"stale-drain-finish-skip"
        ),
    ),
    (
        "retired Swift session command owner",
        re.compile(
            r"CompanionSessionCommanding|CompanionSessionMutationCoordinator|"
            r"RealtimeCompanionClientFactory|LooperRealtimeStateMiniSynchronizer"
        ),
    ),
    (
        "retired Swift base URL race owner",
        re.compile(r"\bCompanionBaseURLRacePlan\b"),
    ),
)

SCAN_SUFFIXES = {
    ".c",
    ".cc",
    ".cpp",
    ".h",
    ".hpp",
    ".json",
    ".md",
    ".proto",
    ".rs",
    ".swift",
    ".toml",
    ".yaml",
    ".yml",
}

SKIPPED_COMPONENTS = {
    ".build",
    ".git",
    ".omo",
    "DerivedData",
    "Frameworks",
    "SourcePackages",
    "build",
    "target",
}


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    rule: str
    text: str


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Guard Looper client-core dependency direction and rewrite-only transport boundaries."
    )
    parser.add_argument(
        "--strict-runtime",
        action="store_true",
        help="Also fail on repo-wide no-event-stream/no-unary runtime surfaces after E/F/G integration.",
    )
    args = parser.parse_args()

    findings: list[Finding] = []
    findings.extend(check_client_core_dependencies())
    findings.extend(check_raw_client_core_uniffi_exports())
    findings.extend(
        scan_files(
            roots=(CLIENT_CORE_ROOT,),
            patterns=CLIENT_CORE_SOURCE_PATTERNS,
        )
    )

    if args.strict_runtime:
        findings.extend(check_retired_runtime_roots())
        findings.extend(
            scan_files(
                roots=tuple(Path(root) for root in RUNTIME_ROOTS),
                patterns=STRICT_RUNTIME_PATTERNS,
                include_markdown=False,
            )
        )
        findings.extend(
            scan_files(
                roots=tuple(Path(root) for root in CLIENT_RUNTIME_ROOTS),
                patterns=CLIENT_RUNTIME_PATTERNS,
                include_markdown=False,
            )
        )
        findings.extend(
            scan_files(
                roots=(Path("ios/LooperCompanion"), Path("macos/LooperMenuBar")),
                patterns=CLIENT_APP_OWNERSHIP_PATTERNS,
                include_markdown=False,
            )
        )

    if findings:
        print("client-core boundary check failed:")
        for finding in findings:
            print(f"{finding.path}:{finding.line}: {finding.rule}: {finding.text}")
        return 1

    mode = "strict runtime + client-core" if args.strict_runtime else "client-core"
    print(f"client-core boundary check passed ({mode})")
    return 0


def check_client_core_dependencies() -> list[Finding]:
    if not CLIENT_CORE_MANIFEST.exists():
        return [
            Finding(
                path=CLIENT_CORE_MANIFEST.relative_to(ROOT_DIR),
                line=1,
                rule="missing client-core manifest",
                text="expected crates/looper-client-core/Cargo.toml",
            )
        ]

    metadata = cargo_metadata()
    root_id = metadata["resolve"]["root"]
    packages = {package["id"]: package for package in metadata["packages"]}
    root_package = packages[root_id]
    findings: list[Finding] = []

    for dependency in root_package["dependencies"]:
        dependency_name = dependency["name"]
        if dependency_name in FORBIDDEN_DEPENDENCIES:
            findings.append(
                Finding(
                    path=CLIENT_CORE_MANIFEST.relative_to(ROOT_DIR),
                    line=1,
                    rule="forbidden client-core dependency",
                    text=f"{dependency_name} must not be an inward client-core dependency",
                )
            )

    targets = root_package.get("targets", [])
    lib_targets = [target for target in targets if "lib" in target.get("kind", [])]
    if not lib_targets:
        findings.append(
            Finding(
                path=CLIENT_CORE_MANIFEST.relative_to(ROOT_DIR),
                line=1,
                rule="missing library target",
                text="client core must expose a Rust library",
            )
        )
        return findings

    crate_types = set(lib_targets[0].get("crate_types", []))
    required_crate_types = {"lib", "staticlib", "cdylib"}
    missing_crate_types = sorted(required_crate_types - crate_types)
    if missing_crate_types:
        findings.append(
            Finding(
                path=CLIENT_CORE_MANIFEST.relative_to(ROOT_DIR),
                line=1,
                rule="missing UniFFI crate type",
                text=f"missing crate types: {', '.join(missing_crate_types)}",
            )
        )

    return findings


def check_raw_client_core_uniffi_exports() -> list[Finding]:
    findings: list[Finding] = []
    forbidden_markers = ("uniffi::Object", "#[uniffi::export]", "#[uniffi::constructor]")
    for path in RAW_CLIENT_CORE_BOUNDARY_FILES:
        absolute_path = ROOT_DIR / path
        if not absolute_path.exists():
            findings.append(
                Finding(
                    path=path,
                    line=1,
                    rule="missing raw client-core boundary file",
                    text=f"expected {path}",
                )
            )
            continue

        text = absolute_path.read_text(encoding="utf-8", errors="replace")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if any(marker in line for marker in forbidden_markers):
                findings.append(
                    Finding(
                        path=path,
                        line=line_number,
                        rule="raw client-core UniFFI export",
                        text=(
                            "raw client/store types must stay Rust-internal; "
                            "export only LooperClientCoreSessionRuntime"
                        ),
                    )
                )
    return findings


def check_retired_runtime_roots() -> list[Finding]:
    findings: list[Finding] = []
    for path in tracked_source_files(RETIRED_RUNTIME_ROOTS):
        findings.append(
            Finding(
                path=path.relative_to(ROOT_DIR),
                line=1,
                rule="retired runtime root",
                text="swift/LooperRealtime has been retired; use swift/LooperClientCore",
            )
        )
    return findings


def cargo_metadata() -> dict:
    command = [
        "cargo",
        "metadata",
        "--format-version=1",
        "--manifest-path",
        str(CLIENT_CORE_MANIFEST),
    ]
    completed = subprocess.run(
        command,
        cwd=ROOT_DIR,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return json.loads(completed.stdout)


def scan_files(
    roots: tuple[Path, ...],
    patterns: tuple[tuple[str, re.Pattern[str]], ...],
    include_markdown: bool = True,
) -> list[Finding]:
    findings: list[Finding] = []
    for path in tracked_source_files(roots):
        if not include_markdown and path.suffix == ".md":
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        relative_path = path.relative_to(ROOT_DIR)
        for line_number, line in enumerate(text.splitlines(), start=1):
            for rule, pattern in patterns:
                match = pattern.search(line)
                if match:
                    findings.append(
                        Finding(
                            path=relative_path,
                            line=line_number,
                            rule=rule,
                            text=line.strip(),
                        )
                    )
    return findings


def tracked_source_files(roots: tuple[Path, ...]) -> list[Path]:
    command = ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--"]
    command.extend(str(root) for root in roots)
    completed = subprocess.run(
        command,
        cwd=ROOT_DIR,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    files: list[Path] = []
    for line in completed.stdout.splitlines():
        path = ROOT_DIR / line
        if path.suffix not in SCAN_SUFFIXES or not path.is_file():
            continue
        if SKIPPED_COMPONENTS.intersection(path.relative_to(ROOT_DIR).parts):
            continue
        files.append(path)
    return files


if __name__ == "__main__":
    sys.exit(main())
