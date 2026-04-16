import { cn } from "@/lib/utils";
import type { ChatCardTheme } from "@/components/chat-card";

type ChatStatusIndicatorProps = {
  active?: boolean;
  theme?: ChatCardTheme;
  className?: string;
};

const ACTIVE_THEME_CLASSES: Record<ChatCardTheme, string> = {
  orange: "text-orange-500",
  cyan: "text-cyan-500",
  emerald: "text-emerald-500",
  olive: "text-olive-500",
};

export function ChatStatusIndicator({
  active = false,
  theme = "orange",
  className,
}: ChatStatusIndicatorProps) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "chat-status-indicator",
        active
          ? cn("chat-status-indicator--fillsweep", ACTIVE_THEME_CLASSES[theme])
          : "chat-status-indicator--diagsweep text-foreground/20",
        className,
      )}
    />
  );
}
