import { type Database } from "bun:sqlite";
import type { LoopPreset, LoopScope, LoopSession } from "../shared/app-rpc";
import { normalizeLoopPreset, normalizeScope } from "./loopndroll-core";

export function buildTelegramSessionListText(sessionsForChat: LoopSession[]) {
  if (sessionsForChat.length === 0) {
    return "No chats are registered to this Telegram destination yet.";
  }

  const lines = sessionsForChat.slice(0, 20).map((session) => {
    const title =
      typeof session.title === "string" && session.title.trim().length > 0
        ? session.title.trim()
        : "Untitled chat";
    return `[${session.sessionRef}] - ${title}`;
  });

  const suffix =
    sessionsForChat.length > lines.length
      ? `\n\nShowing ${lines.length} of ${sessionsForChat.length} chats.`
      : "";

  return `Registered chats:\n${lines.join("\n")}${suffix}`;
}

function getLoopPresetLabel(preset: LoopPreset | null) {
  if (preset === "infinite") {
    return "Infinite";
  }
  if (preset === "await-reply") {
    return "Await Reply";
  }
  if (preset === "completion-checks") {
    return "Completion checks";
  }
  if (preset === "max-turns-1") {
    return "Max Turns 1";
  }
  if (preset === "max-turns-2") {
    return "Max Turns 2";
  }
  if (preset === "max-turns-3") {
    return "Max Turns 3";
  }
  return "Disabled";
}

export function getTelegramStatusSnapshot(db: Database) {
  const row = db
    .query("select scope, global_preset, hooks_auto_registration from settings where id = 1")
    .get() as {
    scope?: unknown;
    global_preset?: unknown;
    hooks_auto_registration?: number | boolean;
  } | null;

  return {
    scope: normalizeScope(row?.scope),
    globalPreset: normalizeLoopPreset(row?.global_preset),
    hooksAutoRegistration:
      typeof row?.hooks_auto_registration === "boolean"
        ? row.hooks_auto_registration
        : Boolean(row?.hooks_auto_registration),
  };
}

export function buildTelegramStatusText(
  settingsSnapshot: {
    scope: LoopScope;
    globalPreset: LoopPreset | null;
    hooksAutoRegistration: boolean;
  },
  sessionsForChat: LoopSession[],
) {
  const visibleSessions = sessionsForChat.filter((session) => !session.archived);
  const lines = [
    "Current status:",
    `Global preset: ${getLoopPresetLabel(settingsSnapshot.globalPreset)}`,
  ];

  if (visibleSessions.length === 0) {
    lines.push("", "Registered chats: none");
    return lines.join("\n");
  }

  lines.push("", "Per-chat presets:");
  for (const session of visibleSessions.slice(0, 20)) {
    const title =
      typeof session.title === "string" && session.title.trim().length > 0
        ? session.title.trim()
        : "Untitled chat";
    const presetLabel =
      session.presetSource === "session"
        ? getLoopPresetLabel(session.preset)
        : session.presetSource === "off"
          ? "Off"
          : "Inherit global";
    lines.push(`[${session.sessionRef}] - ${title}: ${presetLabel}`);
  }

  if (visibleSessions.length > 20) {
    lines.push("", `Showing 20 of ${visibleSessions.length} chats.`);
  }

  return lines.join("\n");
}

export function buildTelegramHelpText() {
  return [
    "Available commands:",
    "/list - List chats registered to this Telegram destination",
    "/status - Show current global mode and per-chat presets",
    "/reply C22 your message - Send a prompt to a specific chat",
    "/mode global infinite - Set the global preset to Infinite",
    "/mode global await - Set the global preset to Await Reply",
    "/mode global checks - Set the global preset to Completion checks",
    "/mode global off - Disable the global preset",
    "/mode C22 infinite - Set chat C22 to Infinite",
    "/mode C22 await - Set chat C22 to Await Reply",
    "/mode C22 off - Stop chat C22",
    "",
    "Reply behavior:",
    "Reply directly to a Telegram notification to target that chat.",
    "Or send plain text without a command to target the latest waiting chat in this Telegram conversation.",
    "",
    "Examples:",
    "/list",
    "/status",
    "/reply C22 fix the failing test",
    "/mode global await",
    "/mode C22 off",
  ].join("\n");
}

export function getModeCommandLabel(preset: LoopPreset | null) {
  if (preset === "infinite") {
    return "Infinite";
  }
  if (preset === "await-reply") {
    return "Await Reply";
  }
  if (preset === "completion-checks") {
    return "Completion checks";
  }
  return "Off";
}
