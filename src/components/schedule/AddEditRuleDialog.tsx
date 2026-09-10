import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { X } from "lucide-react";
import { ProviderSelect } from "./ProviderSelect";
import { WindowListEditor, createDefaultWindow } from "./WindowListEditor";
import { newRuleSchema, type NewRuleInput } from "@/lib/schemas/schedule";
import {
  useCreateScheduleRule,
  useUpdateScheduleRule,
} from "@/hooks/useScheduleRules";
import { APP_IDS } from "@/config/appConfig";
import type { NewScheduleRuleDto, ScheduleRuleDto } from "@/lib/api/schedule";
import type { AppId } from "@/lib/api";

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  /** App preselected for a new rule. Ignored when `rule` is supplied. */
  app: AppId;
  /** Present in edit mode. */
  rule?: ScheduleRuleDto | null;
  onSaved?: () => void;
}

function blankValues(app: AppId): NewRuleInput {
  return {
    app,
    provider_id: "",
    windows: [createDefaultWindow()],
    priority: 0,
    enabled: true,
    // `null` on the wire, `""` in the form: an uncontrolled <Input> warns on null.
    note: "",
  };
}

function valuesFromRule(rule: ScheduleRuleDto): NewRuleInput {
  return {
    app: rule.app,
    provider_id: rule.provider_id,
    windows: rule.windows.map((w) => ({
      dow: [...w.dow],
      start: w.start,
      end: w.end,
    })),
    priority: rule.priority,
    enabled: rule.enabled,
    note: rule.note ?? "",
  };
}

const IGNORED_ERROR_KEYS = new Set(["ref", "type", "types"]);

/**
 * react-hook-form nests errors to mirror the form shape (`windows.0.end`), so a
 * flat `Object.entries` walk renders `[object Object]` for anything inside the
 * window array. Collect the leaf messages instead.
 */
function collectErrorMessages(node: unknown, out: string[] = []): string[] {
  if (Array.isArray(node)) {
    node.forEach((child) => collectErrorMessages(child, out));
    return out;
  }
  if (!node || typeof node !== "object") return out;
  const record = node as Record<string, unknown>;
  if (typeof record.message === "string") {
    out.push(record.message);
    return out;
  }
  for (const [key, value] of Object.entries(record)) {
    if (IGNORED_ERROR_KEYS.has(key)) continue;
    collectErrorMessages(value, out);
  }
  return out;
}

export function AddEditRuleDialog({
  open,
  onOpenChange,
  app,
  rule,
  onSaved,
}: Props) {
  const { t } = useTranslation();
  const create = useCreateScheduleRule();
  const update = useUpdateScheduleRule();
  const isEdit = Boolean(rule);

  const form = useForm<NewRuleInput>({
    resolver: zodResolver(newRuleSchema),
    defaultValues: blankValues(app),
  });

  // Reset on every open so a previously edited rule cannot leak into the next
  // dialog; `reset` receives a freshly built object rather than a shared one.
  useEffect(() => {
    if (!open) return;
    form.reset(rule ? valuesFromRule(rule) : blankValues(app));
  }, [open, rule, app, form]);

  const selectedApp = (form.watch("app") || app) as AppId;

  const onSubmit = form.handleSubmit((values) => {
    const trimmedNote = values.note?.trim() ?? "";
    const payload: NewScheduleRuleDto = {
      app: values.app as AppId,
      provider_id: values.provider_id,
      windows: values.windows.map((w) => ({
        dow: [...w.dow],
        start: w.start,
        end: w.end,
      })),
      priority: values.priority,
      enabled: values.enabled,
      note: trimmedNote === "" ? null : trimmedNote,
    };
    const settle = {
      onSuccess: () => {
        onSaved?.();
        onOpenChange(false);
      },
    };
    if (rule) {
      // `ScheduleRulePatchDto` has no `app`, which is why the app select is
      // locked in edit mode.
      update.mutate(
        {
          id: rule.id,
          patch: {
            provider_id: payload.provider_id,
            windows: payload.windows,
            priority: payload.priority,
            enabled: payload.enabled,
            note: payload.note,
          },
        },
        settle,
      );
    } else {
      create.mutate(payload, settle);
    }
  });

  const errorMessages = [
    ...new Set(collectErrorMessages(form.formState.errors)),
  ];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader className="relative">
          <DialogTitle>
            {t(isEdit ? "schedule.editRule" : "schedule.addRule")}
          </DialogTitle>
          <DialogClose
            className="absolute right-4 top-1/2 -translate-y-1/2 rounded-full p-1.5 hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2"
            aria-label={t("common.close")}
          >
            <X className="size-4 text-muted-foreground" />
          </DialogClose>
        </DialogHeader>
        <form onSubmit={onSubmit} className="flex min-h-0 flex-1 flex-col">
          <div className="flex-1 space-y-4 overflow-y-auto px-6 py-4">
            <div className="space-y-1.5">
              <label htmlFor="rule-app" className="text-sm font-medium">
                {t("schedule.app")}
              </label>
              <Select
                value={selectedApp}
                disabled={isEdit}
                onValueChange={(next) => {
                  form.setValue("app", next, { shouldDirty: true });
                  // Providers are per app, so the previous pick is now invalid.
                  form.setValue("provider_id", "", { shouldDirty: true });
                }}
              >
                <SelectTrigger id="rule-app" data-testid="rule-app-select">
                  <SelectValue placeholder={t("schedule.app")} />
                </SelectTrigger>
                <SelectContent>
                  {APP_IDS.map((id) => (
                    <SelectItem key={id} value={id}>
                      {t(`apps.${id}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium">
                {t("schedule.provider")}
              </label>
              <ProviderSelect
                app={selectedApp}
                value={form.watch("provider_id") || null}
                onChange={(v) =>
                  form.setValue("provider_id", v ?? "", { shouldDirty: true })
                }
              />
            </div>
            <WindowListEditor form={form} />
            <div className="space-y-1.5">
              <label htmlFor="rule-priority" className="text-sm font-medium">
                {t("schedule.priority")}
              </label>
              <Input
                id="rule-priority"
                type="number"
                min={0}
                max={1000}
                className="w-32"
                {...form.register("priority", {
                  // A cleared number input yields "", which `valueAsNumber`
                  // turns into NaN and zod then rejects unhelpfully.
                  setValueAs: (v) => {
                    const n = Number(v);
                    return Number.isFinite(n) ? n : 0;
                  },
                })}
              />
              <p className="text-xs text-muted-foreground">
                {t("schedule.priorityHint")}
              </p>
            </div>
            <label className="flex items-center gap-2" htmlFor="rule-enabled">
              <Switch
                id="rule-enabled"
                checked={form.watch("enabled")}
                onCheckedChange={(v) =>
                  form.setValue("enabled", v, { shouldDirty: true })
                }
              />
              <span className="text-sm font-medium">
                {t("schedule.enabled")}
              </span>
            </label>
            <div className="space-y-1.5">
              <label htmlFor="rule-note" className="text-sm font-medium">
                {t("schedule.note")}
              </label>
              <Input id="rule-note" {...form.register("note")} />
            </div>
            {errorMessages.map((message) => (
              <div key={message} className="text-sm text-destructive">
                {t(message)}
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              disabled={create.isPending || update.isPending}
            >
              {t("common.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
