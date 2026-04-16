import { ArrowLeft } from "@phosphor-icons/react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { revealHooksFile } from "@/lib/loopndroll";
import { handleExternalLinkClick } from "./settings/common";
import { CompletionCheckDialog, NotificationDialog } from "./settings/dialogs";
import { useSettingsRouteModel } from "./settings/model";
import {
  CompletionChecksSection,
  DefaultPromptSection,
  HookRegistrationSection,
  NotificationsSection,
} from "./settings/sections";

function SettingsDialogs({ model }: { model: ReturnType<typeof useSettingsRouteModel> }) {
  return (
    <>
      <NotificationDialog
        botTokenError={model.notificationForm.formState.errors.botToken?.message}
        editingNotificationId={model.editingNotificationId}
        form={model.notificationForm}
        isLoadingTelegramChats={model.isLoadingTelegramChats}
        isOpen={model.isNotificationDialogOpen}
        normalizedNotificationBotToken={model.notificationForm.watch("botToken").trim()}
        onDocsClick={(event) => {
          void handleExternalLinkClick(
            event,
            "https://github.com/lnikell/loopndroll?tab=readme-ov-file#telegram-setup",
          );
        }}
        onOpenChange={model.setIsNotificationDialogOpen}
        onSubmit={model.saveHandlers.saveNotification}
        selectedTelegramChat={model.selectedTelegramChat}
        shouldShowTelegramChatsError={model.shouldShowTelegramChatsError}
        telegramChatIdError={model.notificationForm.formState.errors.telegramChatId?.message}
        telegramChatItems={model.telegramChatItems}
        telegramChatsError={model.telegramChatsError}
        webhookUrlError={model.notificationForm.formState.errors.webhookUrl?.message}
      />
      <CompletionCheckDialog
        commandsError={model.completionCheckForm.formState.errors.commandsText?.message}
        editingCompletionCheckId={model.editingCompletionCheckId}
        form={model.completionCheckForm}
        isOpen={model.isCompletionCheckDialogOpen}
        onOpenChange={model.setIsCompletionCheckDialogOpen}
        onSubmit={model.saveHandlers.saveCompletionCheck}
      />
    </>
  );
}

function SettingsContent({
  model,
  navigate,
}: {
  model: ReturnType<typeof useSettingsRouteModel>;
  navigate: ReturnType<typeof useNavigate>;
}) {
  return (
    <section aria-label="Settings" className="relative px-4 pt-16 pb-32 md:px-6">
      <div className="fixed top-16 left-4 z-20">
        <Button
          aria-label="Go back"
          onClick={() => {
            if (window.history.length > 1) {
              navigate(-1);
            } else {
              navigate("/");
            }
          }}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          <ArrowLeft weight="regular" />
        </Button>
      </div>
      <div className="mx-auto flex w-full max-w-[816px] flex-col gap-6">
        <div className="space-y-0.5">
          <h1 className="text-4xl leading-tight font-semibold tracking-[-0.03em] text-[#fafafa]">Settings</h1>
        </div>
        {model.errorMessage ? <p className="text-sm text-destructive">{model.errorMessage}</p> : null}
        <div className="space-y-5">
          <DefaultPromptSection
            defaultPromptError={model.settingsForm.formState.errors.defaultPrompt?.message}
            form={model.settingsForm}
            onSubmit={() => {
              void model.saveHandlers.saveDefaultPrompt();
            }}
          />
          <NotificationsSection
            notifications={model.notifications}
            onAdd={model.openCreateNotificationDialog}
            onDocsClick={(event) => {
              void handleExternalLinkClick(
                event,
                "https://github.com/lnikell/loopndroll?tab=readme-ov-file#telegram-commands",
              );
            }}
            onEdit={model.openEditNotificationDialog}
            onRemove={(notificationId) => {
              void model.removeNotification(notificationId);
            }}
          />
          <CompletionChecksSection
            completionChecks={model.completionChecks}
            onAdd={model.openCreateCompletionCheckDialog}
            onDocsClick={(event) => {
              void handleExternalLinkClick(
                event,
                "https://github.com/lnikell/loopndroll?tab=readme-ov-file#4-completion-checks",
              );
            }}
            onEdit={model.openEditCompletionCheckDialog}
            onRemove={(completionCheckId) => {
              void model.removeCompletionCheck(completionCheckId);
            }}
          />
          <HookRegistrationSection
            hasResolvedHookState={model.hasResolvedHookState}
            hooksDetected={model.hooksDetected}
            onClearHooks={() => {
              void model.uninstallHooks();
            }}
            onRegisterHooks={() => {
              void model.installHooks();
            }}
            onRevealHooksFile={() => {
              void revealHooksFile();
            }}
          />
        </div>
      </div>
    </section>
  );
}

export function SettingsRoute() {
  const navigate = useNavigate();
  const model = useSettingsRouteModel();

  return (
    <>
      <SettingsDialogs model={model} />
      <SettingsContent model={model} navigate={navigate} />
    </>
  );
}
