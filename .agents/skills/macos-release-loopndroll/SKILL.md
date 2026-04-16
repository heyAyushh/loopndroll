---
name: macos-release-loopndroll
description: Build, sign, notarize, and publish the Loopndroll macOS release for this repository. Use when the user asks to bump the app version, produce a signed/notarized macOS build, or publish/update a GitHub release for Loopndroll without re-discovering the project-specific release flow.
---

# Loopndroll macOS Release

This repository uses Electrobun for desktop packaging, even though the repo name contains `tauri`.

## Repository facts

- The release version lives in `package.json`.
- The app build config is `electrobun.config.ts`.
- The macOS release entrypoint is `scripts/release-macos.sh`.
- The default GitHub remote is `origin -> git@github.com:lnikell/loopndroll.git`.
- The stable release command is `bash scripts/release-macos.sh v<version>`.

## Required environment

Before releasing, verify:

- `.env` exists in the repo root or equivalent env vars are exported.
- `ELECTROBUN_DEVELOPER_ID` is set.
- For notarization, prefer App Store Connect API key variables:
  - `ELECTROBUN_APPLEAPIISSUER`
  - `ELECTROBUN_APPLEAPIKEY`
  - `ELECTROBUN_APPLEAPIKEYPATH`
- `gh auth status` succeeds.
- `security find-identity -v -p codesigning` shows the Developer ID Application identity.
- `xcrun notarytool --version` succeeds.

## Standard workflow

1. Read `package.json` and confirm the current version.
2. If the working tree has uncommitted changes, commit and push those changes before changing the version or starting the release. If those changes are not intended for release, stop and confirm with the user instead of releasing them accidentally.
3. If the user asked for a version bump, edit only `package.json` first. If the user asked to release without specifying a version, bump the patch version in `package.json` by `+0.0.1` first.
4. Commit and push the version bump before releasing.
5. Ensure the working tree is clean because `scripts/release-macos.sh` refuses dirty releases unless `ALLOW_DIRTY_RELEASE=true`.
6. Run:

```bash
bash scripts/release-macos.sh v<version>
```

6. After the script finishes, verify:

```bash
codesign --verify --deep --strict --verbose=2 build/stable-macos-arm64/Loopndroll.app
spctl -a -vvv --type exec build/stable-macos-arm64/Loopndroll.app
xcrun stapler validate artifacts/stable-macos-arm64-Loopndroll.dmg
gh release view v<version> --repo lnikell/loopndroll
```

## What the release script already does

Do not re-implement this unless the script is broken. `scripts/release-macos.sh` already:

- loads `.env` automatically if present
- resolves the GitHub repo slug from `origin`
- checks `gh` authentication
- validates notarization environment variables
- tags `v<version>` if needed and pushes the tag
- enables Electrobun code signing and notarization
- runs `pnpm run build:stable`
- signs and notarizes the app and DMG
- publishes or updates the GitHub release assets

## Operational guidance

- Prefer releasing before making unrelated repo edits. The script expects a clean tree.
- If there are uncommitted changes in the repo, commit and push them before the version bump and release. Do not ship unreviewed local work by accident; if the changes should not be part of the release, stop and ask the user.
- If the user does not specify a version, default to the next patch release by bumping `package.json` from the current version to `+0.0.1` before running the release.
- If you need to add repo changes such as documentation or a skill, do that after the release and commit them separately.
- If the release already exists for the requested tag, the script will upload the fresh artifacts with `--clobber`.
- When reporting completion, include the GitHub release URL and confirm the notarization checks passed.
