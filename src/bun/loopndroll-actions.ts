import { and, asc, eq } from "drizzle-orm";
import type {
  CreateLoopNotificationInput,
  LoopPreset,
  LoopScope,
  UpdateLoopNotificationInput,
} from "../shared/app-rpc";
import { DEFAULT_PROMPT } from "./constants";
import { getLoopndrollDatabase } from "./db/client";
import {
  completionChecks,
  notifications,
  sessionAwaitingReplies,
  sessionNotifications,
  sessionRemotePrompts,
  sessionRuntime,
  sessions,
  settings,
} from "./db/schema";
import { loadSnapshot } from "./hook-management";
import {
  allocateNextSessionRef,
  applyGlobalNotificationToSession,
  buildNewSession,
  buildTelegramBotUrl,
  createNotification,
  getLoopndrollPaths,
  getNotificationBaseLabel,
  getStoredGlobalNotificationId,
  getUniqueCompletionCheckLabel,
  getUniqueNotificationLabel,
  isPersistentPromptPreset,
  mapNotificationRow,
  normalizeCompletionCheckCommands,
  normalizeGlobalCompletionCheckId,
  normalizeGlobalNotificationId,
  notificationInsertFromValue,
  nowIsoString,
  optOutExistingInactiveSessionsFromGlobalPreset,
  readSnapshotFromDatabase,
  resolveSessionPresetState,
  stringifyCompletionCheckCommands,
} from "./loopndroll-core";

export async function saveDefaultPrompt(defaultPrompt: string) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.update(settings)
    .set({ defaultPrompt: defaultPrompt.trim() || DEFAULT_PROMPT })
    .where(eq(settings.id, 1))
    .run();

  return loadSnapshot(paths);
}

export async function createLoopNotification(notification: CreateLoopNotificationInput) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);
  const existingNotifications = readSnapshotFromDatabase().notifications;
  const nextNotification = createNotification(notification);

  nextNotification.label = getUniqueNotificationLabel(
    existingNotifications,
    getNotificationBaseLabel(notification),
  );

  db.insert(notifications).values(notificationInsertFromValue(nextNotification)).run();

  return loadSnapshot(paths);
}

export async function createCompletionCheck(input: { label?: string; commands: string[] }) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);
  const existingCompletionChecks = readSnapshotFromDatabase().completionChecks;
  const commands = normalizeCompletionCheckCommands(input.commands);
  if (commands.length === 0) {
    throw new Error("At least one command is required.");
  }

  db.insert(completionChecks)
    .values({
      id: crypto.randomUUID(),
      label: getUniqueCompletionCheckLabel(
        existingCompletionChecks,
        input.label?.trim() || "Completion check",
      ),
      commandsJson: stringifyCompletionCheckCommands(commands),
      createdAt: nowIsoString(),
    })
    .run();

  return loadSnapshot(paths);
}

export async function updateLoopNotification(notification: UpdateLoopNotificationInput) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);
  const existingNotificationRows = db
    .select()
    .from(notifications)
    .orderBy(asc(notifications.createdAt), asc(notifications.id))
    .all();
  const existingNotifications = existingNotificationRows.map(mapNotificationRow);
  const currentNotification = existingNotifications.find(
    (current) => current.id === notification.id,
  );

  if (!currentNotification) {
    return loadSnapshot(paths);
  }

  const label = getUniqueNotificationLabel(
    existingNotifications,
    getNotificationBaseLabel(notification),
    notification.id,
  );

  if (notification.channel === "slack") {
    db.update(notifications)
      .set({
        label,
        channel: "slack",
        webhookUrl: notification.webhookUrl.trim(),
        chatId: null,
        botToken: null,
        botUrl: null,
        chatUsername: null,
        chatDisplayName: null,
      })
      .where(eq(notifications.id, notification.id))
      .run();
  } else {
    db.update(notifications)
      .set({
        label,
        channel: "telegram",
        webhookUrl: null,
        chatId: notification.chatId.trim(),
        botToken: notification.botToken.trim(),
        botUrl: buildTelegramBotUrl(notification.botToken.trim()),
        chatUsername: notification.chatUsername?.trim() || null,
        chatDisplayName: notification.chatDisplayName?.trim() || null,
      })
      .where(eq(notifications.id, notification.id))
      .run();
  }

  return loadSnapshot(paths);
}

export async function updateCompletionCheck(input: {
  id: string;
  label?: string;
  commands: string[];
}) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);
  const existingCompletionChecks = readSnapshotFromDatabase().completionChecks;
  const currentCheck = existingCompletionChecks.find(
    (completionCheck) => completionCheck.id === input.id,
  );
  if (!currentCheck) {
    return loadSnapshot(paths);
  }

  const commands = normalizeCompletionCheckCommands(input.commands);
  if (commands.length === 0) {
    throw new Error("At least one command is required.");
  }

  db.update(completionChecks)
    .set({
      label: getUniqueCompletionCheckLabel(
        existingCompletionChecks,
        input.label?.trim() || currentCheck.label,
        input.id,
      ),
      commandsJson: stringifyCompletionCheckCommands(commands),
    })
    .where(eq(completionChecks.id, input.id))
    .run();

  return loadSnapshot(paths);
}

export async function deleteLoopNotification(notificationId: string) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.transaction((tx) => {
    tx.delete(notifications).where(eq(notifications.id, notificationId)).run();
    tx.update(settings)
      .set({ globalNotificationId: null })
      .where(eq(settings.globalNotificationId, notificationId))
      .run();
  });

  return loadSnapshot(paths);
}

export async function deleteCompletionCheck(completionCheckId: string) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.transaction((tx) => {
    tx.delete(completionChecks).where(eq(completionChecks.id, completionCheckId)).run();
    tx.update(settings)
      .set({ globalCompletionCheckId: null, globalCompletionCheckWaitForReply: false })
      .where(eq(settings.globalCompletionCheckId, completionCheckId))
      .run();
    tx.update(sessions)
      .set({ completionCheckId: null, completionCheckWaitForReply: false })
      .where(eq(sessions.completionCheckId, completionCheckId))
      .run();
  });

  return loadSnapshot(paths);
}

export async function setSessionNotifications(sessionId: string, notificationIds: string[]) {
  const paths = getLoopndrollPaths();
  const { client, db } = getLoopndrollDatabase(paths.databasePath);
  const validNotificationIds = new Set(
    db
      .select({ id: notifications.id })
      .from(notifications)
      .all()
      .map((row) => row.id),
  );
  const dedupedNotificationIds = [...new Set(notificationIds)].filter((id) =>
    validNotificationIds.has(id),
  );
  const nextSessionRef = allocateNextSessionRef(client);

  db.transaction((tx) => {
    const existingSession = tx
      .select()
      .from(sessions)
      .where(eq(sessions.sessionId, sessionId))
      .get();
    if (!existingSession) {
      tx.insert(sessions).values(buildNewSession(sessionId, nextSessionRef)).run();
      applyGlobalNotificationToSession(tx, sessionId, getStoredGlobalNotificationId(tx));
    }

    tx.delete(sessionNotifications).where(eq(sessionNotifications.sessionId, sessionId)).run();

    if (existingSession?.archived) {
      return;
    }

    if (dedupedNotificationIds.length > 0) {
      tx.insert(sessionNotifications)
        .values(
          dedupedNotificationIds.map((notificationId) => ({
            sessionId,
            notificationId,
          })),
        )
        .run();
    }
  });

  return loadSnapshot(paths);
}

export async function setLoopScope(scope: LoopScope) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.transaction((tx) => {
    tx.update(settings).set({ scope }).where(eq(settings.id, 1)).run();
    tx.delete(sessionRuntime).run();
  });

  return loadSnapshot(paths);
}

export async function setGlobalPreset(preset: LoopPreset | null) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.transaction((tx) => {
    if (preset !== null) {
      optOutExistingInactiveSessionsFromGlobalPreset(tx);
    }

    tx.update(settings).set({ globalPreset: preset }).where(eq(settings.id, 1)).run();
    if (preset === null) {
      tx.run(
        `update sessions
         set active_since = null
         where archived = 0
           and preset_overridden = 0`,
      );
    }
    tx.delete(sessionRuntime).run();
    if (preset !== "await-reply") {
      tx.run(
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
      tx.run(
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
      tx.run(
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

  return loadSnapshot(paths);
}

export async function setGlobalNotification(notificationId: string | null) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);
  const nextNotificationId = normalizeGlobalNotificationId(
    db
      .select({ id: notifications.id })
      .from(notifications)
      .all()
      .map((row) => row.id),
    notificationId,
  );

  db.update(settings)
    .set({ globalNotificationId: nextNotificationId })
    .where(eq(settings.id, 1))
    .run();

  return loadSnapshot(paths);
}

export async function setGlobalCompletionCheckConfig(
  completionCheckId: string | null,
  waitForReplyAfterCompletion: boolean,
) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);
  const nextCompletionCheckId = normalizeGlobalCompletionCheckId(
    db
      .select({ id: completionChecks.id })
      .from(completionChecks)
      .all()
      .map((row) => row.id),
    completionCheckId,
  );

  db.update(settings)
    .set({
      globalCompletionCheckId: nextCompletionCheckId,
      globalCompletionCheckWaitForReply:
        nextCompletionCheckId === null ? false : waitForReplyAfterCompletion,
    })
    .where(eq(settings.id, 1))
    .run();

  return loadSnapshot(paths);
}

export async function setSessionCompletionCheckConfig(
  sessionId: string,
  completionCheckId: string | null,
  waitForReplyAfterCompletion: boolean,
) {
  const paths = getLoopndrollPaths();
  const { client, db } = getLoopndrollDatabase(paths.databasePath);
  const nextSessionRef = allocateNextSessionRef(client);
  const nextCompletionCheckId = normalizeGlobalCompletionCheckId(
    db
      .select({ id: completionChecks.id })
      .from(completionChecks)
      .all()
      .map((row) => row.id),
    completionCheckId,
  );

  db.transaction((tx) => {
    const existingSession = tx
      .select()
      .from(sessions)
      .where(eq(sessions.sessionId, sessionId))
      .get();

    if (!existingSession) {
      tx.insert(sessions).values(buildNewSession(sessionId, nextSessionRef)).run();
      applyGlobalNotificationToSession(tx, sessionId, getStoredGlobalNotificationId(tx));
    }

    tx.update(sessions)
      .set({
        completionCheckId: nextCompletionCheckId,
        completionCheckWaitForReply:
          nextCompletionCheckId === null ? false : waitForReplyAfterCompletion,
      })
      .where(eq(sessions.sessionId, sessionId))
      .run();

    if (nextCompletionCheckId !== null) {
      return;
    }

    tx.delete(sessionAwaitingReplies).where(eq(sessionAwaitingReplies.sessionId, sessionId)).run();
  });

  return loadSnapshot(paths);
}

export async function setSessionPreset(sessionId: string, preset: LoopPreset | null) {
  const paths = getLoopndrollPaths();
  const { client, db } = getLoopndrollDatabase(paths.databasePath);
  const nextSessionRef = allocateNextSessionRef(client);

  db.transaction((tx) => {
    const existingSession = tx
      .select()
      .from(sessions)
      .where(eq(sessions.sessionId, sessionId))
      .get();

    if (!existingSession) {
      tx.insert(sessions).values(buildNewSession(sessionId, nextSessionRef)).run();
      applyGlobalNotificationToSession(tx, sessionId, getStoredGlobalNotificationId(tx));
    }

    if (existingSession?.archived) {
      tx.update(sessions)
        .set({
          preset: null,
          presetOverridden: false,
          activeSince: null,
          completionCheckId: null,
          completionCheckWaitForReply: false,
        })
        .where(eq(sessions.sessionId, sessionId))
        .run();
      tx.delete(sessionRuntime).where(eq(sessionRuntime.sessionId, sessionId)).run();
      tx.delete(sessionAwaitingReplies)
        .where(eq(sessionAwaitingReplies.sessionId, sessionId))
        .run();
      tx.delete(sessionRemotePrompts).where(eq(sessionRemotePrompts.sessionId, sessionId)).run();
      return;
    }

    const previousPreset = resolveSessionPresetState(
      existingSession?.preset,
      existingSession?.presetOverridden,
      null,
    ).effectivePreset;
    const nextActiveSince =
      previousPreset === null && preset !== null
        ? nowIsoString()
        : previousPreset !== null && preset === null
          ? null
          : (existingSession?.activeSince ?? null);
    const isRestartingFromOff = previousPreset === null && preset !== null;

    tx.update(sessions)
      .set({
        preset,
        presetOverridden: true,
        activeSince: nextActiveSince,
      })
      .where(eq(sessions.sessionId, sessionId))
      .run();
    tx.delete(sessionRuntime).where(eq(sessionRuntime.sessionId, sessionId)).run();
    if (preset !== "await-reply") {
      tx.delete(sessionAwaitingReplies)
        .where(eq(sessionAwaitingReplies.sessionId, sessionId))
        .run();
    }
    if (isRestartingFromOff) {
      tx.delete(sessionRemotePrompts).where(eq(sessionRemotePrompts.sessionId, sessionId)).run();
      return;
    }
    if (preset === null) {
      tx.delete(sessionRemotePrompts).where(eq(sessionRemotePrompts.sessionId, sessionId)).run();
      return;
    }
    if (!isPersistentPromptPreset(preset)) {
      tx.delete(sessionRemotePrompts)
        .where(
          and(
            eq(sessionRemotePrompts.sessionId, sessionId),
            eq(sessionRemotePrompts.deliveryMode, "persistent"),
          ),
        )
        .run();
    }
  });

  return loadSnapshot(paths);
}

export async function setSessionArchived(sessionId: string, archived: boolean) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.transaction((tx) => {
    const existingSession = tx
      .select()
      .from(sessions)
      .where(eq(sessions.sessionId, sessionId))
      .get();

    if (!existingSession) {
      return;
    }

    tx.update(sessions)
      .set({
        archived,
        preset: archived ? null : existingSession.preset,
        presetOverridden: archived ? false : existingSession.presetOverridden,
        activeSince: archived ? null : existingSession.activeSince,
        completionCheckId: archived ? null : existingSession.completionCheckId,
        completionCheckWaitForReply: archived ? false : existingSession.completionCheckWaitForReply,
      })
      .where(eq(sessions.sessionId, sessionId))
      .run();

    if (!archived) {
      return;
    }

    tx.delete(sessionNotifications).where(eq(sessionNotifications.sessionId, sessionId)).run();
    tx.delete(sessionRuntime).where(eq(sessionRuntime.sessionId, sessionId)).run();
    tx.delete(sessionAwaitingReplies).where(eq(sessionAwaitingReplies.sessionId, sessionId)).run();
    tx.delete(sessionRemotePrompts).where(eq(sessionRemotePrompts.sessionId, sessionId)).run();
  });

  return loadSnapshot(paths);
}

export async function deleteSession(sessionId: string) {
  const paths = getLoopndrollPaths();
  const { db } = getLoopndrollDatabase(paths.databasePath);

  db.delete(sessions).where(eq(sessions.sessionId, sessionId)).run();

  return loadSnapshot(paths);
}
