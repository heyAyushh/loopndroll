import { useEffect, useState } from "react";
import { getWindowControlsState, runWindowControl } from "@/lib/window-controls";
import type { WindowControlAction, WindowControlsState } from "../shared/app-rpc";

function TrafficLightIcon({ action }: { action: WindowControlAction }) {
  if (action === "close") {
    return (
      <svg aria-hidden="true" className="size-2.5" viewBox="0 0 10 10">
        <path
          d="M2 2 8 8M8 2 2 8"
          fill="none"
          stroke="currentColor"
          strokeLinecap="round"
          strokeWidth="1.4"
        />
      </svg>
    );
  }

  if (action === "minimize") {
    return (
      <svg aria-hidden="true" className="size-2.5" viewBox="0 0 10 10">
        <path
          d="M2 5h6"
          fill="none"
          stroke="currentColor"
          strokeLinecap="round"
          strokeWidth="1.4"
        />
      </svg>
    );
  }

  return (
    <svg aria-hidden="true" className="size-2.5" viewBox="0 0 10 10">
      <path
        d="M5 2v6M2 5h6"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeWidth="1.4"
      />
    </svg>
  );
}

function TrafficLightButton({
  action,
  ariaLabel,
  buttonClassName,
  iconClassName,
  onAction,
}: {
  action: WindowControlAction;
  ariaLabel: string;
  buttonClassName: string;
  iconClassName: string;
  onAction: (action: WindowControlAction) => void;
}) {
  return (
    <button
      aria-label={ariaLabel}
      className={`group flex size-3.5 items-center justify-center rounded-full border border-black/15 shadow-sm transition-transform hover:scale-[1.03] active:scale-95 ${buttonClassName}`}
      onClick={() => onAction(action)}
      type="button"
    >
      <span className={`opacity-0 transition-opacity group-hover:opacity-100 ${iconClassName}`}>
        <TrafficLightIcon action={action} />
      </span>
    </button>
  );
}

export function WindowTrafficLights() {
  const [windowState, setWindowState] = useState<WindowControlsState | null>(null);

  useEffect(() => {
    let isMounted = true;

    void getWindowControlsState().then((state) => {
      if (isMounted && state) {
        setWindowState(state);
      }
    });

    return () => {
      isMounted = false;
    };
  }, []);

  if (windowState?.platform !== "darwin") {
    return null;
  }

  const handleAction = async (action: WindowControlAction) => {
    const nextState = await runWindowControl(action);

    if (nextState) {
      setWindowState(nextState);
    }
  };

  return (
    <div className="electrobun-webkit-app-region-no-drag absolute top-2 left-2 z-30 flex items-center gap-2">
      <TrafficLightButton
        action="close"
        ariaLabel="Close window"
        buttonClassName="bg-[#ff5f57]"
        iconClassName="text-[#5c1b16]"
        onAction={handleAction}
      />
      <TrafficLightButton
        action="minimize"
        ariaLabel="Minimize window"
        buttonClassName="bg-[#febc2e]"
        iconClassName="text-[#6a4500]"
        onAction={handleAction}
      />
      <TrafficLightButton
        action="toggle-primary"
        ariaLabel={windowState.isFullScreen ? "Exit full screen" : "Enter full screen"}
        buttonClassName="bg-[#28c840]"
        iconClassName="text-[#0f5b1b]"
        onAction={handleAction}
      />
    </div>
  );
}
