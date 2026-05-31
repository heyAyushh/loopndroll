import { type Database } from "bun:sqlite";
import type { LoopPreset } from "../shared/app-rpc";
import { getLoopndrollDatabase } from "./db/client";
import {
  TELEGRAM_BRIDGE_POLL_INTERVAL_MS,
  type LoopndrollPaths,
  appendHookDebugLog,
  isPersistentPromptPreset,
  isPromptOnlyArtifact,
  normalizeLoopPreset,
  nowIsoString,
  optOutExistingInactiveSessionsFromGlobalPreset,
  resolveSessionPresetState,
  getLoopndrollPaths,
} from "./loopndroll-core";
import {
  buildTelegramHelpText,
  buildTelegramSessionListText,
  buildTelegramStatusText,
  getModeCommandLabel,
  getTelegramStatusSnapshot,
} from "./telegram-bridge-text";
import {
  collectTelegramChatsFromUpdates,
  fetchTelegramUpdates,
  sendTelegramBridgeMessage,
  type TelegramInboundMessage,
  type TelegramUpdate,
  upsertKnownTelegramChats,
} from "./telegram-utils";

function getTelegramBridgeBotTokens(db: Database) {
  const rows = db
    .query(
      `select distinct bot_token
      from notifications
      where channel = 'telegram'
        and bot_token is not null
        and trim(bot_token) != ''`,
    )
    .all() as Array<{
    bot_token: string;
  }>;

  return rows.map((row) => row.bot_token?.trim() ?? "").filter((botToken) => botToken.length > 0);
}

function getTelegramUpdateCursor(db: Database, botToken: string) {
  const row = db
    .query("select last_update_id from telegram_update_cursors where bot_token = ?")
    .get(botToken) as { last_update_id?: number } | null;

  return typeof row?.last_update_id === "number" ? row.last_update_id : null;
}

function setTelegramUpdateCursor(db: Database, botToken: string, lastUpdateId: number) {
  const updatedAt = nowIsoString();
  db.query(
    `insert into telegram_update_cursors (bot_token, last_update_id, updated_at)
      values (?, ?, ?)
      on conflict(bot_token) do update set
        last_update_id = excluded.last_update_id,
        updated_at = excluded.updated_at`,
  ).run(botToken, lastUpdateId, updatedAt);
}

function isAuthorizedTelegramBridgeChat(db: Database, botToken: string, chatId: string) {
  const row = db
    .query(
      `select 1
      from notifications
      where channel = 'telegram'
        and bot_token = ?
        and chat_id = ?
      limit 1`,
    )
    .get(botToken, chatId);

  return Boolean(row);
}

function getTelegramCommandName(text: string) {
  const token = text.trim().split(/\s+/, 1)[0] ?? "";
  const match = /^\/([a-z0-9_]+)(?:@\w+)?$/i.exec(token);
  return match?.[1]?.toLowerCase() ?? null;
}

function listRegisteredTelegramSessions(db: Database, botToken: string, chatId: string) {
  const settingsRow = db.query("select global_preset from settings where id = 1").get() as {
    global_preset?: unknown;
  } | null;
  const globalPreset = normalizeLoopPreset(settingsRow?.global_preset);
  const rows = db
    .query(
      `select distinct
        s.session_id,
        s.session_ref,
        s.title,
        s.transcript_path,
        s.last_assistant_message,
        s.last_seen_at,
        s.active_since,
        s.preset,
        s.preset_overridden
      from sessions s
      inner join session_notifications sn on sn.session_id = s.session_id
      inner join notifications n on n.id = sn.notification_id
      where n.channel = 'telegram'
        and n.bot_token = ?
        and n.chat_id = ?
      order by s.last_seen_at desc, s.first_seen_at desc`,
    )
    .all(botToken, chatId) as Array<{
    session_id: string;
    session_ref: string;
    title: string | null;
    transcript_path: string | null;
    last_assistant_message: string | null;
    last_seen_at: string;
    active_since: string | null;
    preset: LoopPreset | null;
    preset_overridden: number | boolean | null;
  }>;

  return rows
    .map((row) => {
      const presetState = resolveSessionPresetState(
        row.preset,
        row.preset_overridden,
        globalPreset,
      );

      return {
        sessionId: row.session_id,
        sessionRef: row.session_ref,
        source: "stop" as const,
        cwd: null,
        notificationIds: [],
        archived: false,
        firstSeenAt: row.last_seen_at,
        lastSeenAt: row.last_seen_at,
        activeSince: row.active_since,
        stopCount: 0,
        preset: presetState.preset,
        presetSource: presetState.presetSource,
        effectivePreset: presetState.effectivePreset,
        completionCheckId: null,
        completionCheckWaitForReply: false,
        effectiveCompletionCheckId: null,
        effectiveCompletionCheckWaitForReply: false,
        title: row.title,
        transcriptPath: row.transcript_path,
        lastAssistantMessage: row.last_assistant_message,
      };
    })
    .filter((session) => !isPromptOnlyArtifact(session));
}

function getEffectivePresetForSession(db: Database, sessionId: string) {
  const row = db
    .query(
      `select
        s.preset as session_preset,
        s.preset_overridden as preset_overridden,
        s.archived as session_archived,
        st.global_preset as global_preset
      from sessions s
      left join settings st on st.id = 1
      where s.session_id = ?
      limit 1`,
    )
    .get(sessionId) as {
    session_preset?: unknown;
    preset_overridden?: unknown;
    session_archived?: unknown;
    global_preset?: unknown;
  } | null;

  if (row?.session_archived) {
    return null;
  }

  return resolveSessionPresetState(row?.session_preset, row?.preset_overridden, row?.global_preset)
    .effectivePreset;
}

function findTelegramReplySessionId(
  db: Database,
  botToken: string,
  chatId: string,
  replyToMessageId: number,
) {
  const row = db
    .query(
      `select session_id
      from telegram_delivery_receipts
      where bot_token = ?
        and chat_id = ?
        and telegram_message_id = ?
      order by created_at desc
      limit 1`,
    )
    .get(botToken, chatId, replyToMessageId) as { session_id?: string } | null;

  return typeof row?.session_id === "string" && row.session_id.length > 0 ? row.session_id : null;
}

function findLatestAwaitingTelegramSessionId(db: Database, botToken: string, chatId: string) {
  const row = db
    .query(
      `select ar.session_id
      from session_awaiting_replies ar
      inner join sessions s on s.session_id = ar.session_id
      where ar.bot_token = ?
        and ar.chat_id = ?
        and s.archived = 0
      order by ar.started_at desc, ar.session_id desc
      limit 1`,
    )
    .get(botToken, chatId) as { session_id?: string } | null;

  return typeof row?.session_id === "string" && row.session_id.length > 0 ? row.session_id : null;
}

function findTelegramSessionByRef(
  db: Database,
  botToken: string,
  chatId: string,
  sessionRef: string,
) {
  const row = db
    .query(
      `select distinct
        s.session_id,
        s.session_ref,
        s.title
      from sessions s
      inner join session_notifications sn on sn.session_id = s.session_id
      inner join notifications n on n.id = sn.notification_id
      where n.channel = 'telegram'
        and n.bot_token = ?
        and n.chat_id = ?
        and lower(s.session_ref) = lower(?)
      limit 1`,
    )
    .get(botToken, chatId, sessionRef) as {
    session_id?: string;
    session_ref?: string;
    title?: string | null;
  } | null;

  if (!row?.session_id || !row?.session_ref) {
    return null;
  }

  return {
    sessionId: row.session_id,
    sessionRef: row.session_ref,
    title: row.title ?? null,
  };
}

function parseReplyCommand(text: string) {
  const match = /^\/reply(?:@\w+)?\s+(\S+)\s+([\s\S]+)$/i.exec(text.trim());
  if (!match) {
    return null;
  }

  const sessionRef = match[1]?.trim().toUpperCase() ?? "";
  const promptText = match[2]?.trim() ?? "";
  if (sessionRef.length === 0 || promptText.length === 0) {
    return null;
  }

  return { sessionRef, promptText };
}

function parseModeCommand(text: string) {
  const match = /^\/mode(?:@\w+)?\s+(\S+)\s+(\S+)$/i.exec(text.trim());
  if (!match) {
    return null;
  }

  const rawTarget = match[1]?.trim() ?? "";
  const rawMode = match[2]?.trim().toLowerCase() ?? "";
  if (rawTarget.length === 0 || rawMode.length === 0) {
    return null;
  }

  const preset: LoopPreset | null =
    rawMode === "off"
      ? null
      : rawMode === "infinite"
        ? "infinite"
        : rawMode === "await"
          ? "await-reply"
          : rawMode === "checks"
            ? "completion-checks"
            : null;
  if (rawMode !== "off" && preset === null) {
    return null;
  }

  return {
    target: rawTarget.toLowerCase() === "global" ? "global" : rawTarget.toUpperCase(),
    preset,
    rawMode,
  };
}

function updateSessionPresetFromBridge(db: Database, sessionId: string, preset: LoopPreset | null) {
  const existingSession = db
    .query(
      "select preset, preset_overridden, active_since, archived from sessions where session_id = ? limit 1",
    )
    .get(sessionId) as {
    preset?: unknown;
    preset_overridden?: unknown;
    active_since?: string | null;
    archived?: unknown;
  } | null;
  if (!existingSession) {
    return false;
  }
  if (existingSession.archived) {
    return false;
  }

  const previousPreset = resolveSessionPresetState(
    existingSession.preset,
    existingSession.preset_overridden,
    null,
  ).effectivePreset;
  const nextActiveSince =
    previousPreset === null && preset !== null
      ? nowIsoString()
      : previousPreset !== null && preset === null
        ? null
        : (existingSession.active_since ?? null);
  const isRestartingFromOff = previousPreset === null && preset !== null;

  const applyUpdate = db.transaction(() => {
    db.query(
      `update sessions
       set preset = ?,
           preset_overridden = 1,
           active_since = ?
       where session_id = ?`,
    ).run(preset, nextActiveSince, sessionId);

    db.query("delete from session_runtime where session_id = ?").run(sessionId);

    if (preset !== "await-reply") {
      db.query("delete from session_awaiting_replies where session_id = ?").run(sessionId);
    }

    if (isRestartingFromOff) {
      db.query("delete from session_remote_prompts where session_id = ?").run(sessionId);
      return;
    }

    if (preset === null) {
      db.query("delete from session_remote_prompts where session_id = ?").run(sessionId);
      return;
    }

    if (!isPersistentPromptPreset(preset)) {
      db.query(
        "delete from session_remote_prompts where session_id = ? and delivery_mode = 'persistent'",
      ).run(sessionId);
    }
  });

  applyUpdate();

  return true;
}

function updateGlobalPresetFromBridge(db: Database, preset: LoopPreset | null) {
  const applyUpdate = db.transaction(() => {
    if (preset !== null) {
      optOutExistingInactiveSessionsFromGlobalPreset(db);
    }

    db.query("update settings set global_preset = ? where id = 1").run(preset);

    if (preset === null) {
      db.run(
        `update sessions
         set active_since = null
         where archived = 0
           and preset_overridden = 0`,
      );
    }

    db.query("delete from session_runtime").run();

    if (preset !== "await-reply") {
      db.run(
        `delete from session_awaiting_replies
         where session_id in (
           select session_id
           from sessions
           where preset is null
             and preset_overridden = 0
             and archived = 0
         )`,
      );
    }

    if (preset === null) {
      db.run(
        `delete from session_remote_prompts
         where session_id in (
           select session_id
           from sessions
           where preset is null
             and preset_overridden = 0
             and archived = 0
         )`,
      );
      return;
    }

    if (!isPersistentPromptPreset(preset)) {
      db.run(
        `delete from session_remote_prompts
         where delivery_mode = 'persistent'
           and session_id in (
             select session_id
             from sessions
             where preset is null
               and preset_overridden = 0
               and archived = 0
           )`,
      );
    }
  });

  applyUpdate();
}

function upsertSessionRemotePrompt(
  db: Database,
  sessionId: string,
  promptText: string,
  deliveryMode: "once" | "persistent",
  message: TelegramInboundMessage,
) {
  const trimmedPrompt = promptText.trim();
  if (trimmedPrompt.length === 0) {
    return false;
  }

  db.query(
    `insert into session_remote_prompts (
      session_id,
      source,
      delivery_mode,
      prompt_text,
      telegram_chat_id,
      telegram_message_id,
      created_at
    ) values (?, 'telegram', ?, ?, ?, ?, ?)
    on conflict(session_id, delivery_mode) do update set
      source = excluded.source,
      delivery_mode = excluded.delivery_mode,
      prompt_text = excluded.prompt_text,
      telegram_chat_id = excluded.telegram_chat_id,
      telegram_message_id = excluded.telegram_message_id,
      created_at = excluded.created_at`,
  ).run(
    sessionId,
    deliveryMode,
    trimmedPrompt,
    typeof message.chat?.id === "number" || typeof message.chat?.id === "string"
      ? String(message.chat.id)
      : null,
    typeof message.message_id === "number" ? message.message_id : null,
    nowIsoString(),
  );

  return true;
}

type TelegramBridgeUpdateContext = {
  paths: LoopndrollPaths;
  db: Database;
  botToken: string;
  update: TelegramUpdate;
  message: TelegramInboundMessage;
  trimmedText: string;
  chatId: string;
};

function createTelegramBridgeUpdateContext(
  paths: LoopndrollPaths,
  db: Database,
  botToken: string,
  update: TelegramUpdate,
): TelegramBridgeUpdateContext | null {
  const message = update.message;
  if (!message || typeof message.text !== "string") {
    return null;
  }

  const trimmedText = message.text.trim();
  if (trimmedText.length === 0) {
    return null;
  }

  const chatId =
    typeof message.chat?.id === "number" || typeof message.chat?.id === "string"
      ? String(message.chat.id)
      : null;
  if (!chatId) {
    return null;
  }

  return { paths, db, botToken, update, message, trimmedText, chatId };
}

async function prepareTelegramBridgeUpdate(context: TelegramBridgeUpdateContext) {
  if (!isAuthorizedTelegramBridgeChat(context.db, context.botToken, context.chatId)) {
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "ignored-message",
      reason: "unauthorized-chat",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
    });
    return false;
  }

  const discoveredChats = collectTelegramChatsFromUpdates([context.update]);
  if (discoveredChats.length > 0) {
    upsertKnownTelegramChats(context.db, context.botToken, discoveredChats);
  }

  return true;
}

function formatTelegramSessionLabel(targetSession: { sessionRef: string; title: string | null }) {
  return `[${targetSession.sessionRef}]${targetSession.title ? ` - ${targetSession.title}` : ""}`;
}

async function handleListCommand(context: TelegramBridgeUpdateContext) {
  const sessionsForChat = listRegisteredTelegramSessions(
    context.db,
    context.botToken,
    context.chatId,
  );
  await sendTelegramBridgeMessage(
    context.botToken,
    context.chatId,
    buildTelegramSessionListText(sessionsForChat),
  );
  await appendHookDebugLog(context.paths, {
    type: "telegram-bridge",
    action: "list-sessions",
    botToken: context.botToken,
    updateId: context.update.update_id ?? null,
    chatId: context.chatId,
    sessionCount: sessionsForChat.length,
  });
}

async function handleStatusCommand(context: TelegramBridgeUpdateContext) {
  const sessionsForChat = listRegisteredTelegramSessions(
    context.db,
    context.botToken,
    context.chatId,
  );
  const settingsSnapshot = getTelegramStatusSnapshot(context.db);
  await sendTelegramBridgeMessage(
    context.botToken,
    context.chatId,
    buildTelegramStatusText(settingsSnapshot, sessionsForChat),
  );
  await appendHookDebugLog(context.paths, {
    type: "telegram-bridge",
    action: "status",
    botToken: context.botToken,
    updateId: context.update.update_id ?? null,
    chatId: context.chatId,
    sessionCount: sessionsForChat.length,
    scope: settingsSnapshot.scope,
    globalPreset: settingsSnapshot.globalPreset,
  });
}

async function handleHelpCommand(context: TelegramBridgeUpdateContext) {
  await sendTelegramBridgeMessage(context.botToken, context.chatId, buildTelegramHelpText());
  await appendHookDebugLog(context.paths, {
    type: "telegram-bridge",
    action: "help",
    botToken: context.botToken,
    updateId: context.update.update_id ?? null,
    chatId: context.chatId,
  });
}

async function handleReplyCommand(context: TelegramBridgeUpdateContext) {
  const parsedReply = parseReplyCommand(context.trimmedText);
  if (!parsedReply) {
    await sendTelegramBridgeMessage(
      context.botToken,
      context.chatId,
      "Usage: /reply C12 your message",
    );
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "reply-usage",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
    });
    return;
  }

  const targetSession = findTelegramSessionByRef(
    context.db,
    context.botToken,
    context.chatId,
    parsedReply.sessionRef,
  );
  if (!targetSession) {
    await sendTelegramBridgeMessage(
      context.botToken,
      context.chatId,
      `Chat ${parsedReply.sessionRef} is not registered to this Telegram destination.`,
    );
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "reply-miss",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      sessionRef: parsedReply.sessionRef,
    });
    return;
  }

  const effectivePreset = getEffectivePresetForSession(context.db, targetSession.sessionId);
  if (!effectivePreset) {
    await sendTelegramBridgeMessage(
      context.botToken,
      context.chatId,
      `[${targetSession.sessionRef}] has no active mode. Use /mode ${targetSession.sessionRef} infinite|await first.`,
    );
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "reply-no-mode",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      sessionId: targetSession.sessionId,
      sessionRef: targetSession.sessionRef,
    });
    return;
  }

  upsertSessionRemotePrompt(
    context.db,
    targetSession.sessionId,
    parsedReply.promptText,
    effectivePreset === "await-reply" ? "once" : "persistent",
    context.message,
  );
  await sendTelegramBridgeMessage(
    context.botToken,
    context.chatId,
    effectivePreset === "await-reply"
      ? `Queued for ${formatTelegramSessionLabel(targetSession)}.`
      : `Prompt override set for ${formatTelegramSessionLabel(targetSession)}.`,
  );
  await appendHookDebugLog(context.paths, {
    type: "telegram-bridge",
    action: "queue-command-prompt",
    botToken: context.botToken,
    updateId: context.update.update_id ?? null,
    chatId: context.chatId,
    sessionId: targetSession.sessionId,
    sessionRef: targetSession.sessionRef,
  });
}

async function handleModeCommand(context: TelegramBridgeUpdateContext) {
  const parsedMode = parseModeCommand(context.trimmedText);
  if (!parsedMode) {
    await sendTelegramBridgeMessage(
      context.botToken,
      context.chatId,
      "Usage: /mode global infinite|await|off or /mode C22 infinite|await|off",
    );
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "mode-usage",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
    });
    return;
  }

  if (parsedMode.target === "global") {
    updateGlobalPresetFromBridge(context.db, parsedMode.preset);
    await sendTelegramBridgeMessage(
      context.botToken,
      context.chatId,
      `Global mode set to ${getModeCommandLabel(parsedMode.preset)}.`,
    );
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "mode-global",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      preset: parsedMode.preset,
    });
    return;
  }

  const targetSession = findTelegramSessionByRef(
    context.db,
    context.botToken,
    context.chatId,
    parsedMode.target,
  );
  if (!targetSession) {
    await sendTelegramBridgeMessage(
      context.botToken,
      context.chatId,
      `Chat ${parsedMode.target} is not registered to this Telegram destination.`,
    );
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "mode-miss",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      sessionRef: parsedMode.target,
    });
    return;
  }

  updateSessionPresetFromBridge(context.db, targetSession.sessionId, parsedMode.preset);
  await sendTelegramBridgeMessage(
    context.botToken,
    context.chatId,
    `${formatTelegramSessionLabel(targetSession)} set to ${getModeCommandLabel(parsedMode.preset)}.`,
  );
  await appendHookDebugLog(context.paths, {
    type: "telegram-bridge",
    action: "mode-session",
    botToken: context.botToken,
    updateId: context.update.update_id ?? null,
    chatId: context.chatId,
    sessionId: targetSession.sessionId,
    sessionRef: targetSession.sessionRef,
    preset: parsedMode.preset,
  });
}

async function handleTelegramBridgeCommand(
  context: TelegramBridgeUpdateContext,
  commandName: string,
) {
  switch (commandName) {
    case "list": {
      await handleListCommand(context);
      return true;
    }
    case "status": {
      await handleStatusCommand(context);
      return true;
    }
    case "help": {
      await handleHelpCommand(context);
      return true;
    }
    case "reply": {
      await handleReplyCommand(context);
      return true;
    }
    case "mode": {
      await handleModeCommand(context);
      return true;
    }
    default: {
      return false;
    }
  }
}

async function handleFreeformTelegramMessage(context: TelegramBridgeUpdateContext) {
  const replyToMessageId = context.message.reply_to_message?.message_id;
  const sessionId =
    typeof replyToMessageId === "number"
      ? findTelegramReplySessionId(context.db, context.botToken, context.chatId, replyToMessageId)
      : findLatestAwaitingTelegramSessionId(context.db, context.botToken, context.chatId);
  if (!sessionId) {
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "ignored-message",
      reason: typeof replyToMessageId === "number" ? "unknown-reply-target" : "no-waiting-session",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      replyToMessageId: typeof replyToMessageId === "number" ? replyToMessageId : null,
    });
    return;
  }

  const effectivePreset = getEffectivePresetForSession(context.db, sessionId);
  if (!effectivePreset) {
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "ignored-message",
      reason: "no-active-mode",
      botToken: context.botToken,
      sessionId,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      replyToMessageId: typeof replyToMessageId === "number" ? replyToMessageId : null,
    });
    return;
  }

  const stored = upsertSessionRemotePrompt(
    context.db,
    sessionId,
    context.trimmedText,
    effectivePreset === "await-reply" ? "once" : "persistent",
    context.message,
  );
  await appendHookDebugLog(context.paths, {
    type: "telegram-bridge",
    action: stored ? "queue-prompt" : "ignored-message",
    reason: stored ? undefined : "empty-text",
    botToken: context.botToken,
    sessionId,
    updateId: context.update.update_id ?? null,
    chatId: context.chatId,
    replyToMessageId: typeof replyToMessageId === "number" ? replyToMessageId : null,
  });
}

async function processTelegramBridgeUpdate(
  paths: LoopndrollPaths,
  db: Database,
  botToken: string,
  update: TelegramUpdate,
) {
  const context = createTelegramBridgeUpdateContext(paths, db, botToken, update);
  if (!context || !(await prepareTelegramBridgeUpdate(context))) {
    return;
  }

  const commandName = getTelegramCommandName(context.trimmedText);
  if (commandName && (await handleTelegramBridgeCommand(context, commandName))) {
    return;
  }
  if (commandName) {
    await appendHookDebugLog(context.paths, {
      type: "telegram-bridge",
      action: "ignored-command",
      botToken: context.botToken,
      updateId: context.update.update_id ?? null,
      chatId: context.chatId,
      commandName,
    });
    return;
  }

  await handleFreeformTelegramMessage(context);
}

let telegramBridgeStarted = false;
let telegramBridgePolling = false;

async function pollTelegramReplies() {
  const paths = getLoopndrollPaths();
  const { client } = getLoopndrollDatabase(paths.databasePath);
  const botTokens = getTelegramBridgeBotTokens(client);

  for (const botToken of botTokens) {
    const cursor = getTelegramUpdateCursor(client, botToken);
    const updates = await fetchTelegramUpdates(
      botToken,
      typeof cursor === "number" ? cursor + 1 : undefined,
    );
    if (updates.length === 0) {
      continue;
    }

    const lastUpdateId = updates.reduce((max, update) => {
      return typeof update.update_id === "number" && update.update_id > max
        ? update.update_id
        : max;
    }, cursor ?? -1);

    for (const update of updates) {
      await processTelegramBridgeUpdate(paths, client, botToken, update);
    }

    setTelegramUpdateCursor(client, botToken, lastUpdateId);
  }
}

export function startLoopndrollTelegramBridge() {
  if (telegramBridgeStarted) {
    return;
  }

  telegramBridgeStarted = true;

  const runPoll = async () => {
    if (telegramBridgePolling) {
      return;
    }

    telegramBridgePolling = true;
    try {
      await pollTelegramReplies();
    } catch (error) {
      await appendHookDebugLog(getLoopndrollPaths(), {
        type: "telegram-bridge",
        action: "poll-error",
        error: error instanceof Error ? error.message : String(error),
      }).catch(() => {
        // Ignore logging failures while preserving the bridge loop.
      });
    } finally {
      telegramBridgePolling = false;
    }
  };

  void runPoll();
  setInterval(() => {
    void runPoll();
  }, TELEGRAM_BRIDGE_POLL_INTERVAL_MS);
}
