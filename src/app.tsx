import { ArrowCircleDown, Bell, Gear } from "@phosphor-icons/react";
import { useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { WindowTrafficLights } from "@/components/window-traffic-lights";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAppUpdate } from "@/lib/use-app-update";
import { Toaster } from "@/components/ui/sonner";
import { useLoopndrollState } from "@/lib/use-loopndroll-state";

const GLOBAL_NOTIFICATION_NONE_VALUE = "__none__";

function App() {
  const navigate = useNavigate();
  const { snapshot, updateGlobalNotification } = useLoopndrollState();
  const {
    state: updateState,
    isLoading: isUpdateLoading,
    applyUpdate,
    downloadUpdate,
  } = useAppUpdate();
  const [isNotificationsOpen, setIsNotificationsOpen] = useState(false);
  const showUpdateButton =
    !isUpdateLoading &&
    !!updateState &&
    (updateState.isUpdateAvailable || updateState.isDownloading || updateState.isUpdateReady);
  const updateLabel = updateState?.isUpdateReady
    ? "Restart to Update"
    : updateState?.isDownloading
      ? "Downloading"
      : "Update";

  return (
    <div className="flex h-screen flex-col overflow-hidden rounded-[12px] border border-white/10 bg-background shadow-[0_0_0_1px_rgba(255,255,255,0.03)]">
      <header className="electrobun-webkit-app-region-drag relative z-20 h-14 flex-none bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80">
        <WindowTrafficLights />
        <div className="electrobun-webkit-app-region-no-drag absolute inset-y-0 right-4 flex items-center gap-2 md:right-8">
          {showUpdateButton ? (
            <Button
              disabled={updateState.isDownloading}
              onClick={() => {
                if (updateState.isUpdateReady) {
                  void applyUpdate();
                  return;
                }

                if (!updateState.isDownloading) {
                  void downloadUpdate();
                }
              }}
              size="sm"
              title={
                updateState.availableVersion
                  ? `Update to ${updateState.availableVersion}`
                  : updateLabel
              }
              type="button"
            >
              <ArrowCircleDown data-icon="inline-start" weight="regular" />
              {updateLabel}
            </Button>
          ) : null}
          <DropdownMenu open={isNotificationsOpen} onOpenChange={setIsNotificationsOpen}>
            <DropdownMenuTrigger
              render={
                <Button size="sm" type="button" variant="ghost">
                  <Bell data-icon="inline-start" weight="regular" />
                  Notifications
                </Button>
              }
            />
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuGroup>
                <DropdownMenuRadioGroup
                  onValueChange={(value) => {
                    void updateGlobalNotification(
                      value === GLOBAL_NOTIFICATION_NONE_VALUE ? null : value,
                    );
                    setIsNotificationsOpen(false);
                  }}
                  value={snapshot?.globalNotificationId ?? GLOBAL_NOTIFICATION_NONE_VALUE}
                >
                  <DropdownMenuRadioItem value={GLOBAL_NOTIFICATION_NONE_VALUE}>
                    None
                  </DropdownMenuRadioItem>
                  {(snapshot?.notifications ?? []).map((notification) => (
                    <DropdownMenuRadioItem key={notification.id} value={notification.id}>
                      {notification.label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button onClick={() => navigate("/settings")} size="sm" type="button" variant="outline">
            <Gear data-icon="inline-start" weight="regular" />
            Settings
          </Button>
        </div>
      </header>

      <main className="min-h-0 flex-1 overflow-hidden bg-transparent">
        <div className="min-h-0 h-full overflow-y-auto">
          <Outlet />
        </div>
      </main>

      <Toaster position="bottom-right" richColors />
    </div>
  );
}

export default App;
