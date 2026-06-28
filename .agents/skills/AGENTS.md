# AGENTS.md - Repo Skills

## OVERVIEW

`.agents/skills` contains Looper-specific reusable Codex skills. Use these when a workflow is repeatable enough that future sessions should start from known commands, evidence paths, and repo boundaries.

## STRUCTURE

```text
.agents/skills/
├── <skill>/SKILL.md            # required entry point
├── <skill>/agents/openai.yaml  # optional subagent routing, `.yml` also appears
├── <skill>/scripts/            # optional helpers
├── <skill>/references/         # optional focused references
└── <skill>/assets/             # optional reusable assets
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Realtime architecture cuts | `looper-realtime-cutter/SKILL.md` | Session stream, state-mini visibility, endpoint switching, local store truth, and bounded teammode cutters. |
| iOS command lifecycle debugging | `ios-session-sync-debugging/SKILL.md` | Assistant switcher, Siri/default-session, latest-wins, pending command bugs. |
| iOS diagnostics/perf proof | `ios-perf-diagnostics/SKILL.md` | oslog-live, lldb-trap, perf-loop/xctrace, ETTrace, simulator proof artifacts. |
| macOS release flow | `macos-release-looper/SKILL.md` | Menu bar packaging/release guardrails. |
| Commit workflow | `git-commit/SKILL.md` | Repo-local commit expectations. |
| Design/frontend helpers | `emil-design-eng/`, `shadcn/`, `brainstorming/` | Use only when the task matches that surface. |

## CONVENTIONS

- Keep skills operational: triggers, exact commands, evidence artifacts, and stop conditions matter more than prose.
- Prefer repo-local scripts from `scripts/` instead of embedding long one-off command sequences in a skill.
- When a skill becomes first-class for a subsystem, add a pointer from the nearest `AGENTS.md`.
- Keep subagent YAML focused on roles that save time and produce concrete files/evidence.
- For Looper realtime/state bugs, start with `looper-realtime-cutter` before creating new audit docs or running broad gates.
- Treat plugin/cache skill files as external references; do not copy broad generic text into repo skills.

## ANTI-PATTERNS

- Do not put credentials, tokens, local secrets, or personal machine state into skills.
- Do not create a skill for a one-off task that has no repeatable trigger.
- Do not make a skill authorize outside-repo writes; the caller still needs the repo/script rule and explicit target path.
- Do not let skills drift into stale status reports. Keep them as reusable process, not project history.

## COMMANDS

```bash
find .agents/skills -maxdepth 2 -name SKILL.md -print | sort
rg -n "looper-realtime-cutter|ios-session-sync-debugging|ios-perf-diagnostics" AGENTS.md ios/AGENTS.md scripts/AGENTS.md .agents/skills
```
