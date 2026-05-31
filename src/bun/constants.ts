import type { LoopNotification, LoopPreset, LoopScope, LoopSession } from "../shared/app-rpc";

export const DEFAULT_PROMPT = "Keep working on the task. Do not finish yet.";

export const LOOP_SCOPE_VALUES = ["global", "per-task"] as const satisfies readonly LoopScope[];

export const LOOP_PRESET_VALUES = [
  "infinite",
  "await-reply",
  "completion-checks",
  "max-turns-1",
  "max-turns-2",
  "max-turns-3",
] as const satisfies readonly LoopPreset[];

export const LOOP_SESSION_SOURCE_VALUES = [
  "startup",
  "resume",
  "stop",
] as const satisfies readonly LoopSession["source"][];

export const NOTIFICATION_CHANNEL_VALUES = [
  "slack",
  "telegram",
] as const satisfies readonly LoopNotification["channel"][];
