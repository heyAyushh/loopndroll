export type WindowControlAction = "close" | "minimize" | "toggle-primary";

export type WindowControlsState = {
  isFullScreen: boolean;
  isMaximized: boolean;
  platform: string;
};

export type AppUpdateStage = "idle" | "checking" | "available" | "downloading" | "ready" | "error";

export type AppUpdateState = {
  currentVersion: string | null;
  currentChannel: string | null;
  releaseBaseUrl: string | null;
  availableVersion: string | null;
  stage: AppUpdateStage;
  isConfigured: boolean;
  isChecking: boolean;
  isDownloading: boolean;
  isUpdateAvailable: boolean;
  isUpdateReady: boolean;
  statusMessage: string | null;
  errorMessage: string | null;
  lastCheckedAt: string | null;
};

export type LoopScope = "global" | "per-task";

export type LoopPreset =
  | "infinite"
  | "await-reply"
  | "completion-checks"
  | "max-turns-1"
  | "max-turns-2"
  | "max-turns-3";
export type LoopSessionPresetSource = "global" | "session" | "off";

export type NotificationChannel = "slack" | "telegram";

export type SlackNotification = {
  id: string;
  label: string;
  channel: "slack";
  webhookUrl: string;
  createdAt: string;
};

export type TelegramNotification = {
  id: string;
  label: string;
  channel: "telegram";
  chatId: string;
  botToken: string;
  chatUsername: string | null;
  chatDisplayName: string | null;
  createdAt: string;
};

export type LoopNotification = SlackNotification | TelegramNotification;

export type CompletionCheck = {
  id: string;
  label: string;
  commands: string[];
  createdAt: string;
};

export type TelegramChatOption = {
  chatId: string;
  kind: "dm" | "group" | "channel";
  username: string | null;
  displayName: string;
};

export type CreateLoopNotificationInput =
  | {
      label?: string;
      channel: "slack";
      webhookUrl: string;
    }
  | {
      label?: string;
      channel: "telegram";
      chatId: string;
      botToken: string;
      chatUsername?: string | null;
      chatDisplayName?: string | null;
    };

export type UpdateLoopNotificationInput = CreateLoopNotificationInput & {
  id: string;
};

export type LoopSession = {
  sessionId: string;
  sessionRef: string;
  source: "startup" | "resume" | "stop";
  cwd: string | null;
  notificationIds: string[];
  archived: boolean;
  firstSeenAt: string;
  lastSeenAt: string;
  activeSince: string | null;
  stopCount: number;
  preset: LoopPreset | null;
  presetSource: LoopSessionPresetSource;
  effectivePreset: LoopPreset | null;
  completionCheckId: string | null;
  completionCheckWaitForReply: boolean;
  effectiveCompletionCheckId: string | null;
  effectiveCompletionCheckWaitForReply: boolean;
  title: string | null;
  transcriptPath: string | null;
  lastAssistantMessage: string | null;
};

export type LoopndrollSnapshot = {
  defaultPrompt: string;
  scope: LoopScope;
  globalPreset: LoopPreset | null;
  globalNotificationId: string | null;
  globalCompletionCheckId: string | null;
  globalCompletionCheckWaitForReply: boolean;
  hooksAutoRegistration: boolean;
  notifications: LoopNotification[];
  completionChecks: CompletionCheck[];
  health: {
    registered: boolean;
    issues: string[];
  };
  sessions: LoopSession[];
};

export type AppRpcSchema = {
  bun: {
    requests: {
      getWindowState: {
        params: undefined;
        response: WindowControlsState;
      };
      windowControl: {
        params: {
          action: WindowControlAction;
        };
        response: WindowControlsState;
      };
      getAppUpdateState: {
        params: undefined;
        response: AppUpdateState;
      };
      checkForAppUpdate: {
        params: undefined;
        response: AppUpdateState;
      };
      downloadAppUpdate: {
        params: undefined;
        response: AppUpdateState;
      };
      applyAppUpdate: {
        params: undefined;
        response: AppUpdateState;
      };
      ensureLoopndrollSetup: {
        params: undefined;
        response: LoopndrollSnapshot;
      };
      getLoopndrollState: {
        params: undefined;
        response: LoopndrollSnapshot;
      };
      saveDefaultPrompt: {
        params: {
          defaultPrompt: string;
        };
        response: LoopndrollSnapshot;
      };
      createNotification: {
        params: {
          notification: CreateLoopNotificationInput;
        };
        response: LoopndrollSnapshot;
      };
      createCompletionCheck: {
        params: {
          completionCheck: {
            label?: string;
            commands: string[];
          };
        };
        response: LoopndrollSnapshot;
      };
      getTelegramChats: {
        params: {
          botToken: string;
          waitForUpdates?: boolean;
        };
        response: TelegramChatOption[];
      };
      openExternalUrl: {
        params: {
          url: string;
        };
        response: boolean;
      };
      updateNotification: {
        params: {
          notification: UpdateLoopNotificationInput;
        };
        response: LoopndrollSnapshot;
      };
      updateCompletionCheck: {
        params: {
          completionCheck: {
            id: string;
            label?: string;
            commands: string[];
          };
        };
        response: LoopndrollSnapshot;
      };
      setSessionNotifications: {
        params: {
          sessionId: string;
          notificationIds: string[];
        };
        response: LoopndrollSnapshot;
      };
      deleteNotification: {
        params: {
          notificationId: string;
        };
        response: LoopndrollSnapshot;
      };
      deleteCompletionCheck: {
        params: {
          completionCheckId: string;
        };
        response: LoopndrollSnapshot;
      };
      setLoopScope: {
        params: {
          scope: LoopScope;
        };
        response: LoopndrollSnapshot;
      };
      setGlobalPreset: {
        params: {
          preset: LoopPreset | null;
        };
        response: LoopndrollSnapshot;
      };
      setGlobalNotification: {
        params: {
          notificationId: string | null;
        };
        response: LoopndrollSnapshot;
      };
      setGlobalCompletionCheckConfig: {
        params: {
          completionCheckId: string | null;
          waitForReplyAfterCompletion: boolean;
        };
        response: LoopndrollSnapshot;
      };
      setSessionPreset: {
        params: {
          sessionId: string;
          preset: LoopPreset | null;
        };
        response: LoopndrollSnapshot;
      };
      setSessionCompletionCheckConfig: {
        params: {
          sessionId: string;
          completionCheckId: string | null;
          waitForReplyAfterCompletion: boolean;
        };
        response: LoopndrollSnapshot;
      };
      setSessionArchived: {
        params: {
          sessionId: string;
          archived: boolean;
        };
        response: LoopndrollSnapshot;
      };
      deleteSession: {
        params: {
          sessionId: string;
        };
        response: LoopndrollSnapshot;
      };
      registerHooks: {
        params: undefined;
        response: LoopndrollSnapshot;
      };
      clearHooks: {
        params: undefined;
        response: LoopndrollSnapshot;
      };
      revealHooksFile: {
        params: undefined;
        response: {
          revealed: boolean;
          path: string;
        };
      };
    };
    messages: {};
  };
  webview: {
    requests: {};
    messages: {};
  };
};
