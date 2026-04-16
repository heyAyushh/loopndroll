#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_PATH="$ROOT_DIR/scripts/lint-rule-guard/target/release/lint-rule-guard"

if [[ ! -x "$BIN_PATH" ]]; then
  echo "Missing compiled binary at $BIN_PATH" >&2
  echo "Build it first with: cargo build --release --manifest-path scripts/lint-rule-guard/Cargo.toml" >&2
  exit 2
fi

exec "$BIN_PATH" "$@"
