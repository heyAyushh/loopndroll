#!/usr/bin/env bash
# Generates one drawing plate with Codex's image tool.
#   plates/generate.sh <name> [reference.png ...]
# Reads plates/prompts/<name>.txt, writes plates/<name>.png, logs to plates/prompts/<name>.log.
set -euo pipefail
cd "$(dirname "$0")"
name="$1"; shift
attachments=()
for reference in "$@"; do attachments+=(-i "$reference"); done
{
  cat prompts/_style.txt
  echo
  cat "prompts/${name}.txt"
  echo
  echo "Create ONE landscape image at 1536x1024 with your image generation tool."
  echo "After generating, copy the generated image file to ${name}.png in the current working directory and print its path. Do nothing else."
} | codex exec -m "${CODEX_IMAGE_MODEL:-gpt-5.5}" -s workspace-write --skip-git-repo-check -C "$PWD" ${attachments[@]+"${attachments[@]}"} - > "prompts/${name}.log" 2>&1
test -s "${name}.png" && echo "ok ${name}" || { echo "FAILED ${name}"; exit 1; }
