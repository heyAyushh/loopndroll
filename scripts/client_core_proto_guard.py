from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path


CONTROL_FRAME_PROTO_ROOT_MESSAGES = ("ClientFrame", "ServerFrame")
CONTROL_FRAME_PAYLOAD_PATTERNS = (
    (
        "data-plane payload in Session control frame",
        re.compile(
            r"\b(?:bytes|string|repeated\s+string)\s+"
            r"(?:content|full_transcript|transcript_json|transcript_content|"
            r"log_output|output_json|attachment|blob|screenshot|"
            r"file_content|content_json)\b"
        ),
    ),
)
TEXT_CHUNK_CONTRACT_MARKERS = (
    "Ephemeral, bounded live text hint",
    "Full transcript/log/output content belongs to the data plane",
    "SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES",
    "ensure_text_chunk_content_size",
)


@dataclass(frozen=True)
class ProtoPayloadFinding:
    path: Path
    line: int
    rule: str
    text: str


@dataclass(frozen=True)
class ProtoMessage:
    name: str
    lines: tuple[tuple[int, str], ...]


def proto_payload_findings(
    proto_path: Path,
    frame_limits_path: Path,
    root_dir: Path,
) -> list[ProtoPayloadFinding]:
    text = proto_path.read_text(encoding="utf-8", errors="replace")
    frame_limit_text = frame_limits_path.read_text(encoding="utf-8", errors="replace")
    messages = parse_proto_messages(text)
    reachable_messages = reachable_proto_messages(
        messages,
        CONTROL_FRAME_PROTO_ROOT_MESSAGES,
    )
    findings: list[ProtoPayloadFinding] = []
    for message_name in sorted(reachable_messages):
        message = messages.get(message_name)
        if message is None:
            continue
        for line_number, line in message.lines:
            findings.extend(
                proto_payload_line_findings(
                    proto_path,
                    root_dir,
                    message_name,
                    line_number,
                    line,
                    text,
                    frame_limit_text,
                )
            )
    return findings


def proto_payload_line_findings(
    proto_path: Path,
    root_dir: Path,
    message_name: str,
    line_number: int,
    line: str,
    proto_text: str,
    frame_limit_text: str,
) -> list[ProtoPayloadFinding]:
    findings: list[ProtoPayloadFinding] = []
    for rule, pattern in CONTROL_FRAME_PAYLOAD_PATTERNS:
        if pattern.search(line):
            if is_allowed_bounded_text_chunk(message_name, line, proto_text, frame_limit_text):
                continue
            findings.append(
                ProtoPayloadFinding(
                    path=path_relative_to_root_or_name(proto_path, root_dir),
                    line=line_number,
                    rule=rule,
                    text=line.strip(),
                )
            )
    return findings


def parse_proto_messages(text: str) -> dict[str, ProtoMessage]:
    current_message: str | None = None
    brace_depth = 0
    message_lines: list[tuple[int, str]] = []
    messages: dict[str, ProtoMessage] = {}

    for line_number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if current_message is None:
            match = re.match(r"message\s+([A-Za-z0-9_]+)\s*\{", stripped)
            if match:
                current_message = match.group(1)
                brace_depth = line.count("{") - line.count("}")
                message_lines = []
                if brace_depth <= 0:
                    messages[current_message] = ProtoMessage(current_message, tuple())
                    current_message = None
            continue

        message_lines.append((line_number, line))
        brace_depth += line.count("{") - line.count("}")
        if brace_depth <= 0:
            messages[current_message] = ProtoMessage(
                current_message,
                tuple(message_lines),
            )
            current_message = None
            message_lines = []

    return messages


def reachable_proto_messages(
    messages: dict[str, ProtoMessage],
    roots: tuple[str, ...],
) -> set[str]:
    reachable: set[str] = set()
    pending = list(roots)
    while pending:
        message_name = pending.pop()
        if message_name in reachable:
            continue
        message = messages.get(message_name)
        if message is None:
            continue
        reachable.add(message_name)
        for _, line in message.lines:
            for referenced_message in referenced_proto_messages(line, messages):
                if referenced_message not in reachable:
                    pending.append(referenced_message)
    return reachable


def referenced_proto_messages(line: str, messages: dict[str, ProtoMessage]) -> set[str]:
    stripped = line.strip()
    if stripped.startswith("//"):
        return set()
    refs: set[str] = set()
    field_match = re.match(
        r"(?:optional\s+|repeated\s+)?([A-Z][A-Za-z0-9_]*)\s+[a-zA-Z_][A-Za-z0-9_]*\s*=",
        stripped,
    )
    if field_match and field_match.group(1) in messages:
        refs.add(field_match.group(1))
    return refs


def is_allowed_bounded_text_chunk(
    message_name: str,
    line: str,
    proto_text: str,
    frame_limit_text: str,
) -> bool:
    if message_name != "TextChunk":
        return False
    if not re.search(r"\bstring\s+content\s*=", line):
        return False
    combined_contract = f"{proto_text}\n{frame_limit_text}"
    return all(marker in combined_contract for marker in TEXT_CHUNK_CONTRACT_MARKERS)


def path_relative_to_root_or_name(path: Path, root_dir: Path) -> Path:
    try:
        return path.relative_to(root_dir)
    except ValueError:
        return Path(path.name)
