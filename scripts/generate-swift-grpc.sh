#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE_DIR="${ROOT_DIR}/swift/LooperRealtime"
PROTO_DIR="${ROOT_DIR}/crates/agent-control-plane/proto"
OUTPUT_DIR="${PACKAGE_DIR}/Sources/LooperRealtime/Generated"
PROTO_FILE="${PROTO_DIR}/looper/v1/control_plane.proto"

mkdir -p "${OUTPUT_DIR}"

swift package \
  --package-path "${PACKAGE_DIR}" \
  --allow-writing-to-package-directory \
  generate-grpc-code-from-protos \
  --output-path "${OUTPUT_DIR}" \
  --access-level public \
  --no-servers \
  --file-naming pathToUnderscores \
  --import-path "${PROTO_DIR}" \
  -- "${PROTO_FILE}"
