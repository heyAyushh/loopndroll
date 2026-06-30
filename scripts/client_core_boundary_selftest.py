#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path


ROOT_DIR = Path(__file__).resolve().parents[1]
CHECKER_PATH = ROOT_DIR / "scripts" / "check-client-core-boundaries.py"
SELF_TEST_ROOT = ROOT_DIR / "build" / "client-core-boundary-selftest"


def main() -> int:
    checker = load_checker()
    failures: list[str] = []
    failures.extend(pattern_self_tests(checker))
    failures.extend(proto_contract_self_tests(checker))

    if failures:
        print("client-core boundary self-test failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("client-core boundary self-test passed")
    return 0


def load_checker():
    spec = importlib.util.spec_from_file_location("client_core_boundary_checker", CHECKER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load checker at {CHECKER_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def pattern_self_tests(checker) -> list[str]:
    samples = (
        (
            "strict runtime transport",
            checker.STRICT_RUNTIME_PATTERNS,
            'let stream = MobileEventStream("/api/mobile/events")',
        ),
        (
            "Swift reducer ownership",
            checker.CLIENT_RUNTIME_PATTERNS,
            "final class SessionReducer { func reducer() {} }",
        ),
        (
            "game-surface realtime ownership",
            checker.GAME_SURFACE_FORBIDDEN_PATTERNS,
            "let ack: CommandAck? = nil",
        ),
    )

    failures: list[str] = []
    for name, patterns, text in samples:
        findings = checker.scan_text(Path(f"{name}.swift"), text, patterns)
        if not findings:
            failures.append(name)
    return failures


def proto_contract_self_tests(checker) -> list[str]:
    SELF_TEST_ROOT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="fixture-",
        dir=SELF_TEST_ROOT,
    ) as temp_dir:
        temp_root = Path(temp_dir)
        proto_path = temp_root / "control_plane.proto"
        frame_limits_path = temp_root / "frame_limits.rs"

        failures: list[str] = []
        write_fixture(proto_path, bad_nested_proto())
        write_fixture(frame_limits_path, bad_limits())
        finding_text = findings_text(
            checker.proto_guard_findings(proto_path, frame_limits_path, checker.ROOT_DIR)
        )
        if "content_json" not in finding_text:
            failures.append("reachable ServerFrame payload message")
        if "transcript_content" not in finding_text:
            failures.append("reachable ClientFrame command payload message")

        write_fixture(proto_path, good_text_chunk_proto())
        write_fixture(frame_limits_path, good_text_chunk_limits())
        allowed_findings = checker.proto_guard_findings(
            proto_path,
            frame_limits_path,
            checker.ROOT_DIR,
        )
        if allowed_findings:
            failures.append("bounded TextChunk allowlist")

    cleanup_empty_self_test_root()
    return failures


def bad_nested_proto() -> str:
    return "\n".join(
        [
            'syntax = "proto3";',
            "message ClientFrame {",
            "  oneof frame {",
            "    Command command = 1;",
            "  }",
            "}",
            "message ServerFrame {",
            "  oneof frame {",
            "    MobileEvent event = 1;",
            "  }",
            "}",
            "message Command {",
            "  oneof command {",
            "    SetScopeRequest set_scope = 1;",
            "  }",
            "}",
            "message SetScopeRequest {",
            "  string transcript_content = 1;",
            "}",
            "message MobileEvent {",
            "  string content_json = 1;",
            "}",
            "",
        ]
    )


def good_text_chunk_proto() -> str:
    return "\n".join(
        [
            'syntax = "proto3";',
            "message ServerFrame {",
            "  oneof frame {",
            "    TextChunk text_chunk = 1;",
            "  }",
            "}",
            "// Ephemeral, bounded live text hint on the Session control stream.",
            "// Full transcript/log/output content belongs to the data plane.",
            "message TextChunk {",
            "  int64 seq = 1;",
            "  string content = 2;",
            "}",
            "",
        ]
    )


def bad_limits() -> str:
    return "\n".join(
        [
            "const SESSION_CONTROL_FRAME_MAX_BYTES: usize = 512 * 1024;",
            "",
        ]
    )


def good_text_chunk_limits() -> str:
    return "\n".join(
        [
            "const SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES: usize = 64 * 1024;",
            "fn ensure_text_chunk_content_size() {}",
            "",
        ]
    )


def write_fixture(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


def findings_text(findings: list) -> str:
    return "\n".join(finding.text for finding in findings)


def cleanup_empty_self_test_root() -> None:
    try:
        SELF_TEST_ROOT.rmdir()
    except OSError:
        pass


if __name__ == "__main__":
    raise SystemExit(main())
