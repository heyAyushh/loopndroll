import { cn } from "@/lib/utils";

export type ChatStatusIndicatorState = "working" | "waiting" | "stopped";

type ChatStatusIndicatorProps = {
  state?: ChatStatusIndicatorState;
  className?: string;
};

export function ChatStatusIndicator({ state = "stopped", className }: ChatStatusIndicatorProps) {
  return (
    <span
      aria-hidden="true"
      className={cn("chat-status-indicator", `chat-status-indicator--${state}`, className)}
    >
      <span className="chat-status-indicator__ball" />
    </span>
  );
}
