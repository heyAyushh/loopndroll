from __future__ import annotations

import re
from pathlib import Path


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

GAME_SURFACE_FORBIDDEN_PATTERNS = (
    (
        "realtime ownership inside game surface",
        re.compile(
            r"\b(LooperClientCore|SessionMini|ClientFrame|ServerFrame|CommandAck|"
            r"StateMiniDelta|client_mutation_id|MobileEventStream|CompanionAppModel|"
            r"CompanionSessionMini|setSessionMode|sendSessionPrompt|"
            r"submitNotificationReply|setAssistantSurface)\b"
        ),
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


def scan_text(
    finding_factory,
    path: Path,
    text: str,
    patterns: tuple[tuple[str, re.Pattern[str]], ...],
) -> list:
    findings: list = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        for rule, pattern in patterns:
            if pattern.search(line):
                findings.append(
                    finding_factory(
                        path=path,
                        line=line_number,
                        rule=rule,
                        text=line.strip(),
                    )
                )
    return findings
