import { DotsThree, Plus } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Field,
  FieldContent,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import type { CompletionCheck, LoopNotification } from "@/lib/loopndroll";
import {
  getNotificationChannelLabel,
  settingsSectionCardClassName,
  settingsSectionFooterClassName,
  type SettingsFormValues,
} from "./common";

export function DefaultPromptSection(props: {
  defaultPromptError: string | undefined;
  form: { register: ReturnType<typeof import("react-hook-form").useForm<SettingsFormValues>>["register"] };
  onSubmit: () => void;
}) {
  return (
    <Card className={settingsSectionCardClassName}>
      <CardHeader>
        <CardTitle className="font-semibold">Continue prompt</CardTitle>
        <CardDescription className="leading-normal">
          Sent to Codex when completion is blocked, so the task continues instead of stopping.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <FieldGroup>
          <form
            className="space-y-8"
            id="default-prompt-form"
            onSubmit={(event) => {
              event.preventDefault();
              props.onSubmit();
            }}
          >
            <Field data-invalid={Boolean(props.defaultPromptError)}>
              <FieldContent>
                <FieldLabel className="sr-only" htmlFor="default-prompt">Prompt</FieldLabel>
                <Textarea
                  aria-invalid={Boolean(props.defaultPromptError)}
                  className="min-h-28 max-w-[496px] bg-input px-3 py-2.5 tracking-tight focus-visible:bg-background"
                  id="default-prompt"
                  rows={4}
                  {...props.form.register("defaultPrompt")}
                />
                {props.defaultPromptError ? <FieldError>{props.defaultPromptError}</FieldError> : null}
              </FieldContent>
            </Field>
          </form>
        </FieldGroup>
      </CardContent>
      <CardFooter className={`${settingsSectionFooterClassName} justify-end`}>
        <Button form="default-prompt-form" size="sm" type="submit">Save</Button>
      </CardFooter>
    </Card>
  );
}

function EmptyState({ description, title }: { description: string; title: string }) {
  return (
    <div className="flex min-h-40 flex-col items-center justify-center gap-4 rounded-xl border border-dashed border-border bg-muted/20 px-6 py-8 text-center">
      <div className="space-y-1">
        <p className="text-sm font-medium text-foreground">{title}</p>
        <p className="text-sm text-muted-foreground">{description}</p>
      </div>
    </div>
  );
}

export function NotificationsSection(props: {
  notifications: LoopNotification[];
  onAdd: () => void;
  onDocsClick: (event: React.MouseEvent<HTMLAnchorElement>) => void;
  onEdit: (notification: LoopNotification) => void;
  onRemove: (notificationId: string) => void;
}) {
  return (
    <Card className={settingsSectionCardClassName}>
      <CardHeader>
        <CardTitle className="font-semibold">Notifications</CardTitle>
        <CardDescription className="leading-normal">
          When Codex stops, send its final reply to connected channel.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {props.notifications.length === 0 ? (
          <EmptyState
            description="Add a Slack or Telegram destination for Stop-event delivery."
            title="No notifications yet"
          />
        ) : (
          <Table>
            <TableBody>
              {props.notifications.map((notification) => (
                <TableRow key={notification.id} className="hover:bg-transparent">
                  <TableCell className="pl-0 font-medium">{notification.label}</TableCell>
                  <TableCell className="pr-0 text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger
                        aria-label={`Open actions for ${getNotificationChannelLabel(notification)} notification`}
                        className="inline-flex size-8 items-center justify-center rounded-md transition-colors hover:bg-muted"
                      >
                        <DotsThree aria-hidden="true" weight="bold" />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuGroup>
                          <DropdownMenuItem onClick={() => props.onEdit(notification)}>Edit</DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => props.onRemove(notification.id)}
                            variant="destructive"
                          >
                            Delete
                          </DropdownMenuItem>
                        </DropdownMenuGroup>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
      <CardFooter className={`${settingsSectionFooterClassName} gap-2`}>
        <div className="flex items-center gap-1 text-sm text-muted-foreground">
          <span>Learn more in the </span>
          <a
            className="text-blue-400 transition-colors hover:text-blue-300"
            href="https://github.com/lnikell/loopndroll?tab=readme-ov-file#telegram-commands"
            onClick={props.onDocsClick}
            rel="noreferrer"
            target="_blank"
          >
            documentation
          </a>
        </div>
        <div className="flex min-h-8 min-w-[220px] items-center justify-end gap-2">
          <Button onClick={props.onAdd} size="sm" type="button">
            <Plus weight="bold" />
            Add Notification
          </Button>
        </div>
      </CardFooter>
    </Card>
  );
}

export function CompletionChecksSection(props: {
  completionChecks: CompletionCheck[];
  onAdd: () => void;
  onDocsClick: (event: React.MouseEvent<HTMLAnchorElement>) => void;
  onEdit: (completionCheck: CompletionCheck) => void;
  onRemove: (completionCheckId: string) => void;
}) {
  return (
    <Card className={settingsSectionCardClassName}>
      <CardHeader>
        <CardTitle className="font-semibold">Completion Checks</CardTitle>
        <CardDescription className="leading-normal">
          Register reusable command groups that Completion checks mode can run before a chat is allowed to finish.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {props.completionChecks.length === 0 ? (
          <EmptyState
            description="Add named command groups, then attach one from the Completion checks card on Home."
            title="No completion checks yet"
          />
        ) : (
          <Table>
            <TableBody>
              {props.completionChecks.map((completionCheck) => (
                <TableRow key={completionCheck.id} className="hover:bg-transparent">
                  <TableCell className="pl-0">
                    <div className="space-y-0.5">
                      <p className="font-medium">{completionCheck.label}</p>
                      <p className="text-sm text-muted-foreground">
                        {completionCheck.commands.length} {completionCheck.commands.length === 1 ? "command" : "commands"}
                      </p>
                    </div>
                  </TableCell>
                  <TableCell className="pr-0 text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger
                        aria-label={`Open actions for ${completionCheck.label}`}
                        className="inline-flex size-8 items-center justify-center rounded-md transition-colors hover:bg-muted"
                      >
                        <DotsThree aria-hidden="true" weight="bold" />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuGroup>
                          <DropdownMenuItem onClick={() => props.onEdit(completionCheck)}>Edit</DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => props.onRemove(completionCheck.id)}
                            variant="destructive"
                          >
                            Delete
                          </DropdownMenuItem>
                        </DropdownMenuGroup>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
      <CardFooter className={`${settingsSectionFooterClassName} gap-2`}>
        <div className="flex items-center gap-1 text-sm text-muted-foreground">
          <span>Learn more in the </span>
          <a
            className="text-blue-400 transition-colors hover:text-blue-300"
            href="https://github.com/lnikell/loopndroll?tab=readme-ov-file#4-completion-checks"
            onClick={props.onDocsClick}
            rel="noreferrer"
            target="_blank"
          >
            documentation
          </a>
        </div>
        <div className="flex min-h-8 min-w-[220px] items-center justify-end gap-2">
          <Button onClick={props.onAdd} size="sm" type="button">
            <Plus weight="bold" />
            Add Check
          </Button>
        </div>
      </CardFooter>
    </Card>
  );
}

export function HookRegistrationSection(props: {
  hasResolvedHookState: boolean;
  hooksDetected: boolean;
  onClearHooks: () => void;
  onRegisterHooks: () => void;
  onRevealHooksFile: () => void;
}) {
  return (
    <Card className={settingsSectionCardClassName}>
      <CardHeader>
        <CardTitle className="font-semibold">Hook Registration</CardTitle>
        <CardDescription className="leading-normal">
          Register hooks when they are missing, or clear the current registration state.
        </CardDescription>
      </CardHeader>
      <CardContent />
      <CardFooter className={`${settingsSectionFooterClassName} gap-2`}>
        <div className="flex items-center gap-1 text-sm text-muted-foreground">
          <span>Open</span>
          <button className="text-blue-400 transition-colors hover:text-blue-300" onClick={props.onRevealHooksFile} type="button">
            hooks.json
          </button>
        </div>
        <div className="flex min-h-8 min-w-[220px] items-center justify-end gap-2">
          {props.hasResolvedHookState ? (
            <>
              <Button
                disabled={!props.hooksDetected}
                onClick={props.onClearHooks}
                size="sm"
                type="button"
                variant="outline"
              >
                Clear hooks
              </Button>
              {!props.hooksDetected ? (
                <Button onClick={props.onRegisterHooks} size="sm" type="button">Register</Button>
              ) : null}
            </>
          ) : null}
        </div>
      </CardFooter>
    </Card>
  );
}
