#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -f ".env" ]]; then
  set -a
  # Load local release credentials and defaults for macOS packaging.
  source ".env"
  set +a
fi

fail() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

resolve_repo_slug() {
  if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
    printf '%s\n' "$GITHUB_REPOSITORY"
    return
  fi

  local remote_url
  remote_url="$(git remote get-url origin 2>/dev/null || true)"

  case "$remote_url" in
    git@github.com:*.git)
      printf '%s\n' "${remote_url#git@github.com:}" | sed 's/\.git$//'
      ;;
    https://github.com/*)
      printf '%s\n' "${remote_url#https://github.com/}" | sed 's/\.git$//'
      ;;
    *)
      fail "could not determine GitHub repository. Set GITHUB_REPOSITORY=owner/repo or add an origin remote"
      ;;
  esac
}

ensure_notarization_env() {
  [[ -n "${ELECTROBUN_DEVELOPER_ID:-}" ]] || fail "ELECTROBUN_DEVELOPER_ID is required"

  if [[ -n "${ELECTROBUN_APPLEAPIISSUER:-}" && -n "${ELECTROBUN_APPLEAPIKEY:-}" && -n "${ELECTROBUN_APPLEAPIKEYPATH:-}" ]]; then
    [[ -f "${ELECTROBUN_APPLEAPIKEYPATH}" ]] || fail "ELECTROBUN_APPLEAPIKEYPATH does not exist: ${ELECTROBUN_APPLEAPIKEYPATH}"
    return
  fi

  if [[ -n "${ELECTROBUN_APPLEID:-}" && -n "${ELECTROBUN_APPLEIDPASS:-}" && -n "${ELECTROBUN_TEAMID:-}" ]]; then
    return
  fi

  fail "provide App Store Connect API key envs or Apple ID notarization envs"
}

PACKAGE_VERSION="$(node -p "JSON.parse(require('fs').readFileSync('package.json','utf8')).version")"
TAG="${1:-v${PACKAGE_VERSION}}"
REPO_SLUG="$(resolve_repo_slug)"
RELEASE_TITLE="${RELEASE_TITLE:-$TAG}"
RELEASE_BASE_URL_DEFAULT="https://github.com/${REPO_SLUG}/releases/latest/download"

require_command git
require_command gh
require_command pnpm

gh auth status >/dev/null 2>&1 || fail "gh is not authenticated. Run: gh auth login"
ensure_notarization_env

if [[ "${ALLOW_DIRTY_RELEASE:-false}" != "true" ]] && [[ -n "$(git status --porcelain)" ]]; then
  fail "working tree is dirty. Commit or stash changes first, or set ALLOW_DIRTY_RELEASE=true"
fi

if [[ "${RUN_CHECKS:-false}" == "true" ]]; then
  pnpm run check
fi

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
  git tag "$TAG"
fi

if ! git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  git push origin "$TAG"
fi

export RELEASE_BASE_URL="${RELEASE_BASE_URL:-$RELEASE_BASE_URL_DEFAULT}"
export ELECTROBUN_ENABLE_CODESIGN=true
export ELECTROBUN_ENABLE_NOTARIZE=true

pnpm run build:stable

compgen -G "artifacts/*" >/dev/null || fail "no release artifacts were generated"

if gh release view "$TAG" --repo "$REPO_SLUG" >/dev/null 2>&1; then
  gh release upload "$TAG" artifacts/* --repo "$REPO_SLUG" --clobber
else
  if [[ -n "${RELEASE_NOTES_FILE:-}" ]]; then
    gh release create "$TAG" artifacts/* \
      --repo "$REPO_SLUG" \
      --title "$RELEASE_TITLE" \
      --notes-file "$RELEASE_NOTES_FILE"
  else
    gh release create "$TAG" artifacts/* \
      --repo "$REPO_SLUG" \
      --title "$RELEASE_TITLE" \
      --generate-notes
  fi
fi

printf 'released %s to %s\n' "$TAG" "$REPO_SLUG"
