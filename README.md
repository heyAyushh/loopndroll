



<h1 align="center">Looper</h1>

<p align="center"><strong>Let Codex run until the task is actually done.</strong></p>
<p align="center"><a href="https://github.com/lnikell/looper/releases/latest/download/stable-macos-arm64-Looper.dmg">Download</a></p>

https://github.com/user-attachments/assets/1deba634-a305-4686-8654-65f889162932

If you've ever had to send dozens of follow-up messages just to keep Codex running, or felt frustrated when it skipped tests, lint, or typecheck even though you clearly asked for them in Agents.md, this might help.

## With Looper, you can:

- keep Codex running until you stop it
- require specific commands at the end of a task, and keep going until they pass
- get progress updates in Telegram or Slack, and even reply to redirect the work or change the mode

## How does it work

Looper plugs into Codex through Codex Hooks.

When a chat starts, Looper registers it and remembers the settings for that task.

When Codex tries to stop, Looper gets a chance to decide what should happen next. Depending on the mode you picked, it can:

- let the chat stop
- send another prompt and keep Codex going
- run your completion checks first, and keep going if they fail
- wait for a reply from Telegram and feed that reply back into the same chat

At the same time, it can send the latest assistant message to Telegram or Slack so you can see progress without sitting in front of Codex the whole time.

**IMPORTANT:** Looper runs fully locally on your machine. It does not send your chats, prompts, or app data to any Looper server. If you connect Telegram or Slack, you are using **your own bot** or **your own webhook**, under your control.

## Modes

You can set a mode globally for all chats, or override it per task.

If no mode is active, Codex stops normally.

- **Infinite**: every time Codex stops, Looper sends the default follow-up prompt and keeps the chat going. You can change that default prompt in Settings, or override it for one task by replying to that task in Telegram.
- **Await Reply**: when Codex stops, Looper waits for your reply in Telegram, then sends that reply back into the same chat.
- **Completion Checks**: when Codex stops, Looper runs your commands like tests, lint, or typecheck. If any command fails, it tells Codex to keep going until they pass.
- **Max Turns 1 / 2 / 3**: Looper keeps Codex going for a fixed number of extra turns, then lets it stop.

This gives you a simple choice: keep pushing automatically, wait for human input, require checks to pass, or allow only a small number of extra turns.

## Use cases

- **Keep pushing on a messy refactor without making me send "keep going" every 5 minutes**
  Use **Infinite** when the work is real, but there is no clean automatic way to evaluate "done" yet. This fits tasks like cross-file refactors, bug hunts, and long review-comment cleanup where the next step depends on what Codex finds.

- **Make sure `pnpm test` passes before marking the task as done**
  Use **Completion Checks** when you want Codex to stop only after the repo is actually green. This is for the common case where the agent says it is done, but tests, lint, or typecheck still fail.

- **Send me the result in Telegram and wait for my decision**
  Use **Await Reply** when Codex reaches a decision point and should wait for you instead of guessing. This works well when you want to review a draft, approve a plan, or redirect the work while you are away from your desk.

## Telegram Setup

### Get a Telegram bot token

1. Open Telegram.
2. Start a chat with [`@BotFather`](https://t.me/BotFather).
3. Send `/newbot`.
4. Follow the prompts to choose a bot name and username.
5. BotFather will send you a bot token. It looks like `123456789:AA...`.
6. In Loop N Roll, go to `Settings` -> `Notifications` -> `Add Notification`.
7. Choose `Telegram`.
8. Paste the bot token into `API Token`.

### Get your Telegram chat to show up in the app

1. Open a direct message with your bot and send any message.
2. Or add the bot to a group and send any message in that group.
3. Go back to Looper.
4. The chat should appear in the `Chat` dropdown.
5. Select it and save the notification.

## Telegram Commands

These commands work in Telegram after your bot is connected:

- `/help` - show the command help
- `/list` - list chats registered to this Telegram destination
- `/status` - show the current global mode and per-chat modes
- `/reply C22 your message` - send a message to one specific chat
- `/mode global infinite` - set the global mode to Infinite
- `/mode global await` - set the global mode to Await Reply
- `/mode global checks` - set the global mode to Completion Checks
- `/mode global off` - turn off the global mode
- `/mode C22 infinite` - set chat `C22` to Infinite
- `/mode C22 await` - set chat `C22` to Await Reply
- `/mode C22 checks` - set chat `C22` to Completion Checks
- `/mode C22 off` - stop chat `C22`

Notes:

- If you reply directly to a Telegram notification, Looper uses that chat automatically.
- If you send plain text without a command, Looper sends it to the latest waiting chat in that Telegram conversation.

## Slack Setup

### Important

This app uses a Slack Incoming Webhook URL.

It does **not** use a Slack bot token.

### Get the Slack webhook URL

1. Go to [Slack Apps](https://api.slack.com/apps).
2. Create a new app, or open an existing app.
3. Open `Incoming Webhooks`.
4. Turn Incoming Webhooks on.
5. Click `Add New Webhook to Workspace`.
6. Pick the channel where you want messages posted.
7. Approve the app.
8. Copy the webhook URL. It looks like `https://hooks.slack.com/services/...`.
9. In Looper, go to `Settings` -> `Notifications` -> `Add Notification`.
10. Choose `Slack`.
11. Paste the webhook URL into `Webhook URL`.

If you were looking for a Slack token: this app does not need one for Slack notifications.

## Development

- `pnpm run install:local` - build product Rust binaries into `build/bin`
- `pnpm run dev` - start the Rust server in development mode
- `pnpm run dev:ios-api` - start the Rust mobile/control-plane API for the iPhone companion app
- `pnpm run dev:tui` - build and run the Rust terminal surface inline
- `pnpm run doctor` - check the terminal backend and local Rust server health
- `pnpm run check` - run Rust, macOS, and iOS checks
- `pnpm run test:menubar` - run the native macOS menu bar package tests
- `pnpm run build:menubar` - package the native macOS menu bar app without installing it
- `pnpm run build` - package the native macOS app without installing it

### Rust control plane

The Rust control plane lives in `crates/agent-control-plane`.

```bash
pnpm run install:local
build/bin/looper serve
build/bin/looper
build/bin/looper sessions
build/bin/looper attach <thread-id>
build/bin/looper send <thread-id> "continue"
build/bin/looper wait <thread-id>
build/bin/looper --table doctor
```

Configuration defaults to `~/Library/Application Support/looper/agent-control-plane.sqlite` for
the local store. Use `AGENT_CONTROL_PLANE_LISTEN`, `AGENT_CONTROL_PLANE_STORE`,
`AGENT_CONTROL_PLANE_MOBILE_BASE_URLS`, or `LOOPER_LEGACY_BUN_DB_PATH` for local overrides.

### Rust TUI

The Rust terminal surface supports keyboard and mouse input. Run `looper` from
your existing terminal. It starts the local server when needed and attaches
inline to the same server state used by the macOS menu bar and iPhone app.
`looper attach <thread-id>` opens the same surface focused on one session.
`looper send <thread-id> <prompt>` queues a prompt without opening the terminal
surface, `looper send active <prompt>` targets every active session, and
`looper wait <thread-id>` waits until the session updates. `looper detach` is a
scriptable no-op that reports the headless server state; closing the terminal
surface never stops `looper-server`.

Connections include iPhone pairings, Codex hooks, local Codex app servers,
Cursor/Superconductor Codex processes, and Devin Desktop itself. Devin support
uses Devin Desktop as the single source for all Devin-hosted ACP agents and
sessions, regardless of provider. Devin-spawned Codex servers are still shown as
Codex runtime children owned by `devin-desktop`. `looper devin bridge` and
`/desktop/devin/acp-bridge` expose sanitized Devin Desktop agent metadata,
including the explicit `looper devin probe [agent-id]` preflight path backed by
`POST /desktop/devin/acp-bridge/probe`. Looper never auto-executes Devin
registry commands and does not control Devin-native stop/continue lifecycle yet.

Run `looper --format table doctor` when terminal launch or packaging looks
wrong. It reports the terminal backend, source/hooks health, and the Rust mobile
API auth contract.

### Menu Bar App

The native menu bar app lives in `macos/LooperMenuBar`. It launches the bundled
Rust control plane, registers Codex hooks on launch, clears live hooks on quit
without disabling the next launch, and shuts down the local server when you quit
unless **Detach Server on Quit** is enabled. The status item is a human-visible
anchor that Looper is present; the menu text shows whether the server is ready,
needs attention, or unavailable. The menu shows the lifecycle state, iPhone API
readiness, active chats, Codex servers, automations, and goals
from `/desktop/snapshot?profile=menu`. It does not open a separate terminal
window; use `looper` from your existing terminal for the control surface.

```bash
pnpm run test:menubar
pnpm run build:menubar
bash scripts/build-macos-menu-bar-package.sh --install
```

## iPhone Companion App

The native iPhone companion app lives in [`ios/`](./ios).

### Generate the Xcode project

```bash
cd ios
xcodegen generate
```

### Run the iPhone companion against the Rust dev API

In one terminal:

```bash
pnpm run dev:ios-api
```

The dev API binds to `0.0.0.0:8765` in this script and advertises the current LAN and Tailscale-style interface URLs. If another local service owns that port, set `AGENT_CONTROL_PLANE_LISTEN` explicitly. Generate a device code from the Mac:

```bash
curl http://127.0.0.1:8765/api/mobile/connection-code
```

Open Settings in the companion app and paste the `code` value. Add explicit public or portless fallback endpoints with `AGENT_CONTROL_PLANE_MOBILE_BASE_URLS`, for example:

```bash
AGENT_CONTROL_PLANE_MOBILE_BASE_URLS="https://looper.example.test,http://100.x.y.z:8765" pnpm run dev:ios-api
```

The app tries every URL from the code in order, so LAN, Tailscale, and a portless reverse proxy can all be bundled into one code. Simulator and device builds both require a paired code or build-time URL.

In another terminal:

```bash
xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build
```

If there is no local Codex thread data yet, the dev API returns an empty real snapshot instead of demo sessions.

## Useful Links

- Telegram BotFather: [https://t.me/BotFather](https://t.me/BotFather)
- Telegram Bot API: [https://core.telegram.org/bots/api](https://core.telegram.org/bots/api)
- Slack apps: [https://api.slack.com/apps](https://api.slack.com/apps)
- Slack incoming webhooks: [https://api.slack.com/messaging/webhooks](https://api.slack.com/messaging/webhooks)
