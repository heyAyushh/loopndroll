#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CONTROL_PLANE_CRATE="crates/agent-control-plane/Cargo.toml"
DEFAULT_PREFIX="${ROOT_DIR}/build/bin"
LOOPER_EXECUTABLE="looper"
CLI_EXECUTABLE="looper-cli"
SERVER_EXECUTABLE="looper-server"
STALE_EXECUTABLES=("looper-tui" "ratty")

prefix="$DEFAULT_PREFIX"
allow_outside_project=false

fail() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

usage() {
  cat >&2 <<EOF
usage: $0 [--prefix <dir>] [--allow-outside-project]

Installs Looper CLI tools into a directory.
Default prefix: build/bin
EOF
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      [[ -n "${2:-}" ]] || fail "--prefix requires a directory"
      prefix="$2"
      shift 2
      ;;
    --allow-outside-project)
      allow_outside_project=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      fail "unknown argument: $1"
      ;;
  esac
done

case "$prefix" in
  /*) resolved_prefix="$prefix" ;;
  *) resolved_prefix="${ROOT_DIR}/${prefix}" ;;
esac

if [[ "$allow_outside_project" != "true" && "$resolved_prefix" != "$ROOT_DIR"/* ]]; then
  fail "refusing to install outside project root without --allow-outside-project: $resolved_prefix"
fi

require_command cargo
require_command install

cargo build --release --manifest-path "$CONTROL_PLANE_CRATE" \
  --bin "$LOOPER_EXECUTABLE" \
  --bin "$CLI_EXECUTABLE" \
  --bin "$SERVER_EXECUTABLE"

mkdir -p "$resolved_prefix"
install -m 755 "crates/agent-control-plane/target/release/${LOOPER_EXECUTABLE}" "${resolved_prefix}/${LOOPER_EXECUTABLE}"
install -m 755 "crates/agent-control-plane/target/release/${CLI_EXECUTABLE}" "${resolved_prefix}/${CLI_EXECUTABLE}"
install -m 755 "crates/agent-control-plane/target/release/${SERVER_EXECUTABLE}" "${resolved_prefix}/${SERVER_EXECUTABLE}"
for stale_executable in "${STALE_EXECUTABLES[@]}"; do
  stale_path="${resolved_prefix}/${stale_executable}"
  if [[ -e "$stale_path" ]]; then
    rm -f "$stale_path"
  fi
done

printf 'installed=%s\n' "$resolved_prefix"
printf 'try=%s/looper --help\n' "$resolved_prefix"
