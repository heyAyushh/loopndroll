import { spawnSync } from "node:child_process";
import { hostname } from "node:os";
import { basename } from "node:path";
import type {
  CompletionCheck,
  LoopNotification,
  LoopPreset,
  LoopSession,
  LoopndrollSnapshot,
} from "../shared/app-rpc";
import type {
  FallbackMobileState,
  MobileAssistantClient,
  MobileCompletionCheck,
  MobileNotification,
  MobileGitRepositoryMetadata,
  MobileSessionMetadata,
  MobileSessionTaskKind,
  MobileSessionSourceReference,
  MobileSessionDetail,
  MobileSessionStatus,
  MobileSnapshot,
  MobileSessionSummary,
} from "../shared/mobile-contract";

const FALLBACK_HOST_NAME = "looper Dev Mac";
const DEFAULT_MOBILE_API_BASE_URL = "http://127.0.0.1:8787";
const FALLBACK_PRIMARY_SESSION_ID = "session-ios-1";
const FALLBACK_WAITING_SESSION_ID = "session-ios-2";
const FALLBACK_STOPPED_SESSION_ID = "session-ios-3";
const FALLBACK_ARCHIVED_SESSION_ID = "session-ios-4";
const FALLBACK_SUPER_ENGINEERING_SESSION_ID = "session-ios-5";
const FALLBACK_PROJECT_ROOT = "/tmp/looper-demo";
const FALLBACK_IOS_PROJECT_ROOT = `${FALLBACK_PROJECT_ROOT}/ios`;
const FALLBACK_SUPER_ENGINEERING_ROOT = "/tmp/super-engineering-demo";
const MILLISECONDS_PER_MINUTE = 60 * 1000;
const MILLISECONDS_PER_HOUR = 60 * MILLISECONDS_PER_MINUTE;
const FALLBACK_PRIMARY_SESSION_AGE_MS = 3 * MILLISECONDS_PER_MINUTE;
const FALLBACK_WAITING_SESSION_AGE_MS = 23 * MILLISECONDS_PER_MINUTE;
const FALLBACK_STOPPED_SESSION_AGE_MS = 2 * MILLISECONDS_PER_HOUR;
const FALLBACK_ARCHIVED_SESSION_AGE_MS = 24 * MILLISECONDS_PER_HOUR;
const FALLBACK_SUPER_ENGINEERING_SESSION_AGE_MS = 45 * MILLISECONDS_PER_MINUTE;

function stripMarkdownTitle(value: string | null) {
  if (!value) {
    return "Untitled Session";
  }

  return value
    .replace(/!\[([^\]]*)\]\([^)]+\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/(^|\s)(?:#{1,6}\s+|>\s+|\d+\.\s+|[-+*]\s+)/gm, "$1")
    .replace(/(\*\*|__)(.*?)\1/g, "$2")
    .replace(/(\*|_)(.*?)\1/g, "$2")
    .replace(/~~(.*?)~~/g, "$1")
    .replace(/\\([\\`*_#[\]~>])/g, "$1")
    .replace(/[\\`*_#[\]~>]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function summarizeAssistantMessage(value: string | null) {
  if (!value) {
    return null;
  }

  const collapsedValue = value.replace(/\s+/g, " ").trim();
  return collapsedValue.length > 180 ? `${collapsedValue.slice(0, 177)}...` : collapsedValue;
}

function displayNameFromPath(path: string) {
  const normalizedPath = path.trim().replace(/\/+$/, "");
  const name = basename(normalizedPath);
  return name.length > 0 ? name : normalizedPath;
}

function runGit(projectPath: string, args: string[]) {
  const result = spawnSync("git", ["-C", projectPath, ...args], {
    encoding: "utf8",
    timeout: 1_000,
  });

  if (result.status !== 0) {
    return null;
  }

  const value = result.stdout.trim();
  return value.length > 0 ? value : null;
}

function mapGitRepository(projectPath: string | null): MobileGitRepositoryMetadata | null {
  if (!projectPath) {
    return null;
  }

  const repositoryPath = runGit(projectPath, ["rev-parse", "--show-toplevel"]);
  if (!repositoryPath) {
    return null;
  }

  return {
    repositoryName: displayNameFromPath(repositoryPath),
    repositoryPath,
    remoteURL: runGit(repositoryPath, ["config", "--get", "remote.origin.url"]),
    branch: runGit(repositoryPath, ["branch", "--show-current"]),
    commit: runGit(repositoryPath, ["rev-parse", "--short", "HEAD"]),
  };
}

function inferTaskKind(session: LoopSession): MobileSessionTaskKind {
  const haystack = [session.title, session.lastAssistantMessage]
    .filter(Boolean)
    .join("\n")
    .toLowerCase();

  if (/\bplan mode\b|\bplanning\b|\bplan\b/.test(haystack)) {
    return "plan";
  }

  if (/\btodo\b|\bto do\b|\btask list\b|\bchecklist\b/.test(haystack)) {
    return "todo";
  }

  if (session.activeSince || session.stopCount > 0) {
    return "implementation";
  }

  return "unknown";
}

function inferPullRequestURL(gitRepository: MobileGitRepositoryMetadata | null): string | null {
  if (!gitRepository?.remoteURL || !gitRepository.branch) {
    return null;
  }

  return null;
}

function inferSupportsSubagents(session: LoopSession) {
  const client = inferMobileAssistantClient(session);
  if (client === "codex" || client === "claude-code" || client === "super-engineering") {
    return true;
  }

  const haystack = [session.title, session.lastAssistantMessage]
    .filter(Boolean)
    .join("\n")
    .toLowerCase();
  return /\bsubagents?\b|\bsub-agents?\b|\bagents?\b/.test(haystack);
}

function mapSessionSources(
  session: LoopSession,
  gitRepository: MobileGitRepositoryMetadata | null,
  pullRequestURL: string | null,
): MobileSessionSourceReference[] {
  const sources: MobileSessionSourceReference[] = [];

  if (session.cwd) {
    sources.push({
      kind: "cwd",
      label: "Working Directory",
      value: session.cwd,
      url: null,
    });
  }

  if (session.transcriptPath) {
    sources.push({
      kind: "transcript",
      label: "Transcript",
      value: session.transcriptPath,
      url: null,
    });
  }

  if (gitRepository) {
    sources.push({
      kind: "git",
      label: "Git Repository",
      value: gitRepository.repositoryName,
      url: gitRepository.remoteURL,
    });
  }

  if (pullRequestURL) {
    sources.push({
      kind: "pull-request",
      label: "Pull Request",
      value: pullRequestURL,
      url: pullRequestURL,
    });
  }

  return sources;
}

function mapSessionMetadata(session: LoopSession): MobileSessionMetadata {
  const projectPath = session.cwd?.trim() || null;
  const gitRepository = mapGitRepository(projectPath);
  const pullRequestURL = inferPullRequestURL(gitRepository);
  const taskKind = inferTaskKind(session);
  const supportsSubagents = inferSupportsSubagents(session);
  const tags = [
    projectPath ? "project" : "instant-chat",
    session.source,
    taskKind === "unknown" ? null : taskKind,
    supportsSubagents ? "subagents" : null,
    gitRepository ? "git" : null,
    pullRequestURL ? "pull-request" : null,
    session.transcriptPath ? "transcript" : null,
  ].filter((tag): tag is string => tag !== null);

  return {
    kind: projectPath ? "project" : "instant-chat",
    source: session.source,
    projectName: projectPath ? displayNameFromPath(projectPath) : null,
    projectPath,
    taskKind,
    transcriptAvailable: session.transcriptPath !== null,
    gitRepository,
    pullRequestURL,
    supportsSubagents,
    installedPlugins: [],
    sources: mapSessionSources(session, gitRepository, pullRequestURL),
    tags,
  };
}

export function inferMobileAssistantClient(
  session: Pick<LoopSession, "cwd" | "transcriptPath">,
): MobileAssistantClient {
  const segments = [session.cwd, session.transcriptPath].filter(
    (value): value is string => typeof value === "string" && value.trim().length > 0,
  );

  if (segments.length === 0) {
    return "unknown";
  }

  const haystack = segments.join("\n").toLowerCase();

  if (haystack.includes("openclaw")) {
    return "openclaw";
  }

  if (
    haystack.includes("/.claude/") ||
    haystack.includes("\\.claude\\") ||
    haystack.includes("/claude-code") ||
    haystack.includes(".claude/projects")
  ) {
    return "claude-code";
  }

  if (
    haystack.includes("/.cursor/") ||
    haystack.includes("\\.cursor\\") ||
    haystack.includes("application support/cursor") ||
    haystack.endsWith("/.cursor") ||
    haystack.includes("/cursor/user/")
  ) {
    return "cursor";
  }

  if (
    haystack.includes("super.engineering") ||
    haystack.includes("superengineering") ||
    haystack.includes("super-engineering")
  ) {
    return "super-engineering";
  }

  if (
    haystack.includes("/.codex/") ||
    haystack.includes("\\.codex\\") ||
    haystack.includes(".codex/sessions")
  ) {
    return "codex";
  }

  return "unknown";
}

function getSessionStatus(session: LoopSession): MobileSessionStatus {
  if (session.archived) {
    return "archived";
  }

  if (session.effectivePreset === "await-reply") {
    return "waiting";
  }

  if (session.activeSince) {
    return "active";
  }

  return "stopped";
}

function mapNotification(notification: LoopNotification): MobileNotification {
  return {
    id: notification.id,
    label: notification.label,
    channel: notification.channel,
  };
}

function mapCompletionCheck(completionCheck: CompletionCheck): MobileCompletionCheck {
  return {
    id: completionCheck.id,
    label: completionCheck.label,
    commandCount: completionCheck.commands.length,
  };
}

function mapSessionSummary(session: LoopSession): MobileSessionSummary {
  return {
    id: session.sessionId,
    ref: session.sessionRef,
    title: stripMarkdownTitle(session.title),
    status: getSessionStatus(session),
    effectiveMode: session.effectivePreset,
    lastUpdatedAt: session.lastSeenAt,
    assistantPreview: summarizeAssistantMessage(session.lastAssistantMessage),
    isArchived: session.archived,
    assistantClient: inferMobileAssistantClient(session),
    metadata: mapSessionMetadata(session),
  };
}

export function mapLoopSessionToDetail(
  session: LoopSession,
  snapshot: LoopndrollSnapshot,
): MobileSessionDetail {
  const summary = mapSessionSummary(session);

  return {
    ...summary,
    latestAssistantMessage: session.lastAssistantMessage,
    notificationIds: session.notificationIds,
    completionCheckID: session.effectiveCompletionCheckId,
    completionCheckWaitForReply: session.effectiveCompletionCheckWaitForReply,
    availableNotifications: snapshot.notifications.map(mapNotification),
    availableCompletionChecks: snapshot.completionChecks.map(mapCompletionCheck),
  };
}

function hostAddressFromBaseURL(baseURL: string) {
  try {
    const url = new URL(baseURL);
    return url.host;
  } catch {
    return baseURL.replace(/^https?:\/\//, "");
  }
}

export function mapLoopndrollSnapshotToMobile(
  snapshot: LoopndrollSnapshot,
  apiBaseURL = DEFAULT_MOBILE_API_BASE_URL,
): MobileSnapshot {
  const globalNotification = snapshot.notifications.find(
    (notification) => notification.id === snapshot.globalNotificationId,
  );
  const globalCompletionCheck = snapshot.completionChecks.find(
    (completionCheck) => completionCheck.id === snapshot.globalCompletionCheckId,
  );

  return {
    host: {
      id: "local-mac",
      name: hostname(),
      address: hostAddressFromBaseURL(apiBaseURL),
      isReachable: true,
      lastSyncedAt: new Date().toISOString(),
    },
    globalSettings: {
      defaultPrompt: snapshot.defaultPrompt,
      globalMode: snapshot.globalPreset,
      scope: snapshot.scope,
      notificationLabel: globalNotification?.label ?? null,
      completionCheckLabel: globalCompletionCheck?.label ?? null,
      completionCheckWaitForReply: snapshot.globalCompletionCheckWaitForReply,
    },
    sessions: snapshot.sessions.map(mapSessionSummary),
    notifications: snapshot.notifications.map(mapNotification),
    completionChecks: snapshot.completionChecks.map(mapCompletionCheck),
  };
}

function createFallbackNotifications(): MobileNotification[] {
  return [
    {
      id: "telegram-main",
      label: "Telegram Main",
      channel: "telegram",
    },
    {
      id: "slack-builds",
      label: "Slack Builds",
      channel: "slack",
    },
  ];
}

function createFallbackCompletionChecks(): MobileCompletionCheck[] {
  return [
    {
      id: "check-1",
      label: "Repo Green",
      commandCount: 3,
    },
    {
      id: "check-2",
      label: "Smoke Test",
      commandCount: 1,
    },
  ];
}

function createFallbackSession(
  id: string,
  ref: string,
  title: string,
  status: MobileSessionStatus,
  effectiveMode: LoopPreset | null,
  assistantPreview: string,
  lastUpdatedAt: string,
  isArchived: boolean,
  assistantClient: MobileAssistantClient,
  metadata: MobileSessionMetadata,
): MobileSessionSummary {
  return {
    id,
    ref,
    title,
    status,
    effectiveMode,
    lastUpdatedAt,
    assistantPreview,
    isArchived,
    assistantClient,
    metadata,
  };
}

function createFallbackSessionMetadata(input: {
  source: MobileSessionMetadata["source"];
  projectName: string | null;
  projectPath: string | null;
  taskKind: MobileSessionTaskKind;
  transcriptAvailable: boolean;
  supportsSubagents: boolean;
}): MobileSessionMetadata {
  const kind = input.projectPath ? "project" : "instant-chat";
  return {
    kind,
    source: input.source,
    projectName: input.projectName,
    projectPath: input.projectPath,
    taskKind: input.taskKind,
    transcriptAvailable: input.transcriptAvailable,
    gitRepository: null,
    pullRequestURL: null,
    supportsSubagents: input.supportsSubagents,
    installedPlugins: [],
    sources: [],
    tags: [
      kind,
      input.source,
      input.taskKind,
      input.supportsSubagents ? "subagents" : null,
      input.transcriptAvailable ? "transcript" : null,
    ].filter((tag): tag is string => tag !== null),
  };
}

function createFallbackSessionSummaries(now: Date): MobileSessionSummary[] {
  return [
    createFallbackSession(
      FALLBACK_PRIMARY_SESSION_ID,
      "C22",
      "Make an iOS app for looper",
      "active",
      "infinite",
      "I’ve scaffolded the iPhone companion app and I’m wiring the simulator data source now.",
      new Date(now.getTime() - FALLBACK_PRIMARY_SESSION_AGE_MS).toISOString(),
      false,
      "codex",
      createFallbackSessionMetadata({
        source: "startup",
        projectName: "looper",
        projectPath: FALLBACK_PROJECT_ROOT,
        taskKind: "implementation",
        transcriptAvailable: true,
        supportsSubagents: true,
      }),
    ),
    createFallbackSession(
      FALLBACK_WAITING_SESSION_ID,
      "C21",
      "Debug haptics on device",
      "waiting",
      "await-reply",
      "I’m waiting for a reply before continuing with the haptics pass.",
      new Date(now.getTime() - FALLBACK_WAITING_SESSION_AGE_MS).toISOString(),
      false,
      "cursor",
      createFallbackSessionMetadata({
        source: "resume",
        projectName: "looper-ios",
        projectPath: FALLBACK_IOS_PROJECT_ROOT,
        taskKind: "implementation",
        transcriptAvailable: true,
        supportsSubagents: false,
      }),
    ),
    createFallbackSession(
      FALLBACK_STOPPED_SESSION_ID,
      "C17",
      "Fix completion checks for release build",
      "stopped",
      "completion-checks",
      "Typecheck passed, but the simulator smoke test still needs work.",
      new Date(now.getTime() - FALLBACK_STOPPED_SESSION_AGE_MS).toISOString(),
      false,
      "claude-code",
      createFallbackSessionMetadata({
        source: "stop",
        projectName: "looper",
        projectPath: FALLBACK_PROJECT_ROOT,
        taskKind: "todo",
        transcriptAvailable: true,
        supportsSubagents: true,
      }),
    ),
    createFallbackSession(
      FALLBACK_ARCHIVED_SESSION_ID,
      "C11",
      "Archive old desktop polish branch",
      "archived",
      null,
      "The work is done and the session has been archived.",
      new Date(now.getTime() - FALLBACK_ARCHIVED_SESSION_AGE_MS).toISOString(),
      true,
      "openclaw",
      createFallbackSessionMetadata({
        source: "resume",
        projectName: null,
        projectPath: null,
        taskKind: "plan",
        transcriptAvailable: false,
        supportsSubagents: false,
      }),
    ),
    createFallbackSession(
      FALLBACK_SUPER_ENGINEERING_SESSION_ID,
      "C09",
      "Ship Super.Engineering integration",
      "active",
      "infinite",
      "Wiring the Super.Engineering bridge and validating session sync.",
      new Date(now.getTime() - FALLBACK_SUPER_ENGINEERING_SESSION_AGE_MS).toISOString(),
      false,
      "super-engineering",
      createFallbackSessionMetadata({
        source: "startup",
        projectName: "super-engineering",
        projectPath: FALLBACK_SUPER_ENGINEERING_ROOT,
        taskKind: "implementation",
        transcriptAvailable: true,
        supportsSubagents: true,
      }),
    ),
  ];
}

function createFallbackSessionDetail(
  summary: MobileSessionSummary,
  notifications: MobileNotification[],
  completionChecks: MobileCompletionCheck[],
  latestAssistantMessage: string,
  notificationIds: string[],
  completionCheckID: string | null,
  completionCheckWaitForReply: boolean,
): MobileSessionDetail {
  return {
    ...summary,
    latestAssistantMessage,
    notificationIds,
    completionCheckID,
    completionCheckWaitForReply,
    availableNotifications: notifications,
    availableCompletionChecks: completionChecks,
  };
}

function createFallbackSessionDetails(
  sessionSummaries: MobileSessionSummary[],
  notifications: MobileNotification[],
  completionChecks: MobileCompletionCheck[],
) {
  const sessionMap = new Map(sessionSummaries.map((session) => [session.id, session]));

  return {
    [FALLBACK_PRIMARY_SESSION_ID]: createFallbackSessionDetail(
      sessionMap.get(FALLBACK_PRIMARY_SESSION_ID)!,
      notifications,
      completionChecks,
      "I’ve scaffolded the iPhone app and I’m wiring the simulator data source now. Next I’m finishing the session detail view and the Bun dev API so the simulator can show real looper-shaped state.",
      ["telegram-main"],
      "check-1",
      true,
    ),
    [FALLBACK_WAITING_SESSION_ID]: createFallbackSessionDetail(
      sessionMap.get(FALLBACK_WAITING_SESSION_ID)!,
      notifications,
      completionChecks,
      "I’m waiting for a reply before continuing with the haptics pass.",
      ["telegram-main"],
      null,
      false,
    ),
    [FALLBACK_STOPPED_SESSION_ID]: createFallbackSessionDetail(
      sessionMap.get(FALLBACK_STOPPED_SESSION_ID)!,
      notifications,
      completionChecks,
      "Typecheck passed, but the simulator smoke test still needs work.",
      ["slack-builds"],
      "check-1",
      true,
    ),
    [FALLBACK_ARCHIVED_SESSION_ID]: createFallbackSessionDetail(
      sessionMap.get(FALLBACK_ARCHIVED_SESSION_ID)!,
      notifications,
      completionChecks,
      "The work is done and the session has been archived.",
      [],
      null,
      false,
    ),
    [FALLBACK_SUPER_ENGINEERING_SESSION_ID]: createFallbackSessionDetail(
      sessionMap.get(FALLBACK_SUPER_ENGINEERING_SESSION_ID)!,
      notifications,
      completionChecks,
      "Wiring the Super.Engineering bridge and validating session sync.",
      ["telegram-main"],
      null,
      false,
    ),
  };
}

export function createFallbackMobileState(
  apiBaseURL = DEFAULT_MOBILE_API_BASE_URL,
): FallbackMobileState {
  const notifications = createFallbackNotifications();
  const completionChecks = createFallbackCompletionChecks();
  const now = new Date();
  const sessionSummaries = createFallbackSessionSummaries(now);

  return {
    snapshot: {
      host: {
        id: "fallback-mac",
        name: FALLBACK_HOST_NAME,
        address: hostAddressFromBaseURL(apiBaseURL),
        isReachable: true,
        lastSyncedAt: now.toISOString(),
      },
      globalSettings: {
        defaultPrompt: "Keep working on the task. Do not finish yet.",
        globalMode: "infinite",
        scope: "per-task",
        notificationLabel: "Telegram Main",
        completionCheckLabel: "Repo Green",
        completionCheckWaitForReply: true,
      },
      sessions: sessionSummaries,
      notifications,
      completionChecks,
    },
    sessionDetails: createFallbackSessionDetails(sessionSummaries, notifications, completionChecks),
  };
}
