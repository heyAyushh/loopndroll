import { useEffect, useState } from "react";
import {
  applyAppUpdate,
  checkForAppUpdate,
  downloadAppUpdate,
  getAppUpdateState,
  type AppUpdateState,
} from "./app-update";

const APP_UPDATE_POLL_INTERVAL_MS = 2000;

type UseAppUpdateResult = {
  state: AppUpdateState | null;
  isLoading: boolean;
  applyUpdate: () => Promise<void>;
  checkForUpdates: () => Promise<void>;
  downloadUpdate: () => Promise<void>;
};

export function useAppUpdate(): UseAppUpdateResult {
  const [state, setState] = useState<AppUpdateState | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    async function hydrate() {
      try {
        const nextState = await getAppUpdateState();
        if (!cancelled && nextState) {
          setState(nextState);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    void hydrate();

    const intervalId = window.setInterval(() => {
      void getAppUpdateState().then((nextState) => {
        if (!cancelled && nextState) {
          setState(nextState);
        }
      });
    }, APP_UPDATE_POLL_INTERVAL_MS);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, []);

  async function runAction(action: () => Promise<AppUpdateState | undefined>) {
    const nextState = await action();
    if (nextState) {
      setState(nextState);
    }
  }

  return {
    state,
    isLoading,
    applyUpdate() {
      return runAction(() => applyAppUpdate());
    },
    checkForUpdates() {
      return runAction(() => checkForAppUpdate());
    },
    downloadUpdate() {
      return runAction(() => downloadAppUpdate());
    },
  };
}
