import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { Plus, RefreshCw } from "lucide-react";
import {
  useScheduleRules,
  useDeleteScheduleRule,
  useUpdateScheduleRule,
  useEvaluateScheduleNow,
} from "@/hooks/useScheduleRules";
import {
  SCHEDULE_DEGRADED_FAILURE_THRESHOLD,
  useScheduleHealth,
} from "@/hooks/useScheduleHealth";
import { RuleCard } from "./RuleCard";
import { AddEditRuleDialog } from "./AddEditRuleDialog";
import { NextSwitchHint } from "./NextSwitchHint";
import { SwitchHistorySection } from "./SwitchHistorySection";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { ScheduleRuleDto } from "@/lib/api/schedule";
import { toast } from "sonner";

interface DialogState {
  open: boolean;
  rule: ScheduleRuleDto | null;
}

export function SchedulesPage() {
  const { t } = useTranslation();
  const [dialog, setDialog] = useState<DialogState>({
    open: false,
    rule: null,
  });
  const { data: rules = [], isLoading, refetch } = useScheduleRules();
  const remove = useDeleteScheduleRule();
  const update = useUpdateScheduleRule();
  const evaluate = useEvaluateScheduleNow();
  const { data: health } = useScheduleHealth();

  // Only the apps the user has actually enabled a rule for, so the hint does
  // not fan out into one query per supported CLI.
  const scheduledApps = [
    ...new Set(rules.filter((r) => r.enabled).map((r) => r.app)),
  ];

  const runNow = useCallback(() => {
    evaluate.mutate(undefined, {
      onSuccess: (r) =>
        toast.success(
          t("schedule.runNowResult", {
            fired: r.apps.filter((a: { fired: boolean }) => a.fired).length,
            skipped: r.apps.filter((a: { fired: boolean }) => !a.fired).length,
          }),
        ),
      onError: (e: Error) => toast.error(extractErrorMessage(e)),
    });
  }, [evaluate, t]);

  // `useMutation` / `useQuery` return fresh objects every render, so subscribing
  // directly on them would tear down and re-register the Tauri listeners on each
  // render. The ref keeps the listeners registered once while always invoking the
  // latest closures.
  const handlersRef = useRef({ runNow, refetch });
  handlersRef.current = { runNow, refetch };

  // Tray menu bridge (T13): "Run Scheduler Now" emits `schedule://evaluate-now`,
  // "Open Schedules" emits `schedule://open` after raising the main window.
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const register = (event: string, handler: () => void) => {
      listen(event, handler)
        .then((unlisten) => {
          if (cancelled) {
            unlisten();
          } else {
            unlisteners.push(unlisten);
          }
        })
        .catch((error) => {
          console.warn(`[schedule] failed to listen for ${event}`, error);
        });
    };

    // The page is already mounted when this fires, so there is no navigation to do —
    // switching `currentView` lives in App.tsx's scope (parked, see task-13-report).
    // Refetching is still worthwhile: the tray may have switched providers since the
    // window was last visible.
    register("schedule://open", () => {
      void handlersRef.current.refetch();
    });
    register("schedule://evaluate-now", () => handlersRef.current.runNow());

    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  return (
    <div className="px-6 pt-4 space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t("schedule.title")}</h1>
        <div className="flex gap-2">
          <Button onClick={() => setDialog({ open: true, rule: null })}>
            <Plus className="w-4 h-4 mr-1" />
            {t("schedule.addRule")}
          </Button>
          <Button
            variant="outline"
            disabled={evaluate.isPending}
            onClick={runNow}
          >
            <RefreshCw className="w-4 h-4 mr-1" />
            {t("schedule.runNow")}
          </Button>
        </div>
      </div>

      {health &&
        health.consecutive_failures >= SCHEDULE_DEGRADED_FAILURE_THRESHOLD && (
          <div className="rounded bg-destructive/10 p-2 text-sm text-destructive">
            {t("schedule.health.degraded", { n: health.consecutive_failures })}
          </div>
        )}

      {isLoading ? (
        <div className="text-muted-foreground text-sm">
          {t("common.loading")}
        </div>
      ) : rules.length === 0 ? (
        <div className="text-muted-foreground text-sm">
          {t("schedule.empty")}
        </div>
      ) : (
        <div className="space-y-2">
          {rules.map((r) => (
            <RuleCard
              key={r.id}
              rule={r}
              onEdit={() => setDialog({ open: true, rule: r })}
              onDelete={() => remove.mutate(r.id)}
              onToggle={(enabled) =>
                update.mutate({ id: r.id, patch: { enabled } })
              }
            />
          ))}
        </div>
      )}

      {scheduledApps.length > 0 && (
        <div className="space-y-1">
          {scheduledApps.map((app) => (
            <NextSwitchHint key={app} app={app} />
          ))}
        </div>
      )}

      <SwitchHistorySection />

      <AddEditRuleDialog
        open={dialog.open}
        onOpenChange={(v) =>
          setDialog((prev) => ({ open: v, rule: v ? prev.rule : null }))
        }
        app={dialog.rule?.app ?? "claude"}
        rule={dialog.rule}
      />
    </div>
  );
}
