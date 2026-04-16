# lint-rule-guard

Fast Rust CLI for failing CI when source files contain disabled lint rules.

## Build once

```bash
cargo build --release --manifest-path scripts/lint-rule-guard/Cargo.toml
```

## Run the compiled binary

```bash
scripts/run-lint-rule-guard.sh --rule max-lines --rule max-lines-per-function src electrobun.config.ts vite.config.ts
```

Fail on any disable directive at all:

```bash
scripts/run-lint-rule-guard.sh --any-rule src electrobun.config.ts vite.config.ts
```

Exclude folders or paths:

```bash
scripts/run-lint-rule-guard.sh --any-rule --exclude src/generated --exclude fixtures .
```

Force-include a path even if it sits under a default excluded folder:

```bash
scripts/run-lint-rule-guard.sh --any-rule --include node_modules/some-package .
```

Emit JSON for CI ingestion:

```bash
scripts/run-lint-rule-guard.sh --any-rule --format json .
```

## Exit codes

- `0`: no matching disabled-lint directives found
- `1`: matching disabled-lint directives found
- `2`: invalid arguments or runtime error

## Ignore behavior

- Built-in excludes are hardcoded and do not come from `.gitignore`
- Default excludes: `.git`, `.hg`, `.svn`, `node_modules`, `dist`, `build`, `coverage`, `target`
- `--exclude <path>` adds more excluded paths
- `--include <path>` overrides excludes for a specific path or folder prefix
- `--no-default-excludes` disables the built-in exclude list

## Supported directives

- `eslint-disable`
- `eslint-disable-line`
- `eslint-disable-next-line`
- `oxlint-disable`
- `oxlint-disable-line`
- `oxlint-disable-next-line`

Comments that disable all rules, such as `/* eslint-disable */`, are treated as matching every requested rule.
