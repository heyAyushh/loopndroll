import type { LoopPreset, LoopScope } from "./app-rpc";

export type MobileSessionStatus = "active" | "waiting" | "stopped" | "archived";
export type MobileQuickAction = "open-session" | "continue" | "reply" | "archive" | "mute-session";

export type HostSummary = {
  id: string;
  name: string;
  address: string;
  isReachable: boolean;
  lastSyncedAt: string;
};

export type GlobalSettings = {
  defaultPrompt: string;
  globalMode: LoopPreset | null;
  scope: LoopScope;
  notificationLabel: string | null;
  completionCheckLabel: string | null;
  completionCheckWaitForReply: boolean;
};

export type MobileNotification = {
  id: string;
  label: string;
  channel: "slack" | "telegram";
};

export type MobileCompletionCheck = {
  id: string;
  label: string;
  commandCount: number;
};

export type MobilePushEnvironment = "development" | "production";
export type MobilePushRegistrationState = "enabled" | "stored-awaiting-provider";

export type MobilePushRegistrationRequest = {
  installationId: string;
  deviceToken: string;
  bundleId: string;
  environment: MobilePushEnvironment;
  deviceName?: string | null;
};

export type MobilePushRegistrationResponse = {
  state: MobilePushRegistrationState;
  environment: MobilePushEnvironment;
  registeredAt: string;
  message: string;
};

export type MobilePushTestResponse = {
  delivered: boolean;
  message: string;
};

export type MobileSessionSummary = {
  id: string;
  ref: string;
  title: string;
  status: MobileSessionStatus;
  effectiveMode: LoopPreset | null;
  lastUpdatedAt: string;
  assistantPreview: string | null;
  isArchived: boolean;
};

export type MobileSessionDetail = MobileSessionSummary & {
  latestAssistantMessage: string | null;
  notificationIds: string[];
  completionCheckID: string | null;
  completionCheckWaitForReply: boolean;
  availableNotifications: MobileNotification[];
  availableCompletionChecks: MobileCompletionCheck[];
};

export type MobileSnapshot = {
  host: HostSummary;
  globalSettings: GlobalSettings;
  sessions: MobileSessionSummary[];
  notifications: MobileNotification[];
  completionChecks: MobileCompletionCheck[];
};

export type FallbackMobileState = {
  snapshot: MobileSnapshot;
  sessionDetails: Record<string, MobileSessionDetail>;
};
