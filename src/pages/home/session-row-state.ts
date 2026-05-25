import type { LoopNotification, LoopPreset, LoopSession } from "@/lib/loopndroll";

export type SessionOrbState = "working" | "waiting" | "stopped";
export type SessionHoverActionKind = "notifications" | "completion-check" | "transcript";

export type SessionRowState = {
  orbState: SessionOrbState;
  summaryLabel: string;
  hoverActionKinds: SessionHoverActionKind[];
};

function getTurnLimit(preset: LoopPreset | null) {
  if (preset === "max-turns-1") {
    return 1;
  }

  if (preset === "max-turns-2") {
    return 2;
  }

  if (preset === "max-turns-3") {
    return 3;
  }

  return null;
}

function hasAttachedTelegramNotification(session: LoopSession, notifications: LoopNotification[]) {
  return session.notificationIds.some((notificationId) =>
    notifications.some(
      (notification) => notification.id === notificationId && notification.channel === "telegram",
    ),
  );
}

function getHoverActionKinds(session: LoopSession) {
  const hoverActionKinds: SessionHoverActionKind[] = [];

  if (session.transcriptPath) {
    hoverActionKinds.push("transcript");
  }

  if (session.notificationIds.length > 0) {
    hoverActionKinds.push("notifications");
  }

  if (session.effectiveCompletionCheckId !== null || session.completionCheckId !== null) {
    hoverActionKinds.push("completion-check");
  }

  return hoverActionKinds;
}

export function buildSessionRowState(args: {
  notifications: LoopNotification[];
  session: LoopSession;
  showArchivedSessions: boolean;
}) {
  const { notifications, session, showArchivedSessions } = args;
  const hoverActionKinds = getHoverActionKinds(session);
  const effectivePreset = session.effectivePreset;
  const turnLimit = getTurnLimit(effectivePreset ?? session.preset);

  if (showArchivedSessions || session.archived) {
    return {
      orbState: "stopped",
      summaryLabel: session.transcriptPath ? "Archived · Transcript" : "Archived",
      hoverActionKinds,
    } satisfies SessionRowState;
  }

  if (effectivePreset === null) {
    return {
      orbState: "stopped",
      summaryLabel: session.transcriptPath ? "Stopped · Transcript" : "Stopped",
      hoverActionKinds,
    } satisfies SessionRowState;
  }

  if (effectivePreset === "await-reply") {
    return {
      orbState: "waiting",
      summaryLabel: hasAttachedTelegramNotification(session, notifications)
        ? "Waiting · Telegram"
        : "Waiting · Reply",
      hoverActionKinds,
    } satisfies SessionRowState;
  }

  if (turnLimit !== null) {
    const currentTurn = Math.min(session.stopCount + 1, turnLimit);
    return {
      orbState: "working",
      summaryLabel: `Running · Turn ${currentTurn}/${turnLimit}`,
      hoverActionKinds,
    } satisfies SessionRowState;
  }

  if (effectivePreset === "completion-checks") {
    return {
      orbState: "working",
      summaryLabel: session.effectiveCompletionCheckWaitForReply
        ? "Running · Checks · Reply"
        : "Running · Checks",
      hoverActionKinds,
    } satisfies SessionRowState;
  }

  return {
    orbState: "working",
    summaryLabel: "Running · Infinite",
    hoverActionKinds,
  } satisfies SessionRowState;
}
