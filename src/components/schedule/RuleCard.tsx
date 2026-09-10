import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Pencil, Trash2 } from "lucide-react";
import { summarizeWindows } from "@/utils/scheduleWindows";
import { useAppProviders } from "@/hooks/useAppProviders";
import type { ScheduleRuleDto } from "@/lib/api/schedule";

interface Props {
  rule: ScheduleRuleDto;
  onEdit: () => void;
  onDelete: () => void;
  onToggle?: (enabled: boolean) => void;
}

export function RuleCard({ rule, onEdit, onDelete, onToggle }: Props) {
  const { t } = useTranslation();
  const { data: providers, isPending } = useAppProviders(rule.app);
  const provider = providers?.[rule.provider_id];
  // Only claim the provider is gone once the lookup has actually resolved.
  const providerMissing = !isPending && !provider;
  const providerLabel = provider
    ? provider.name
    : isPending
      ? t("common.loading")
      : rule.provider_id;

  return (
    <div
      className="rounded-lg border p-4 flex flex-col gap-2"
      data-testid="rule-card"
    >
      <div className="flex items-center justify-between">
        <div className="font-medium flex items-center gap-2">
          <span>
            {t(`apps.${rule.app}`)} → {providerLabel}
          </span>
          {providerMissing && (
            <span className="rounded bg-destructive/10 px-1.5 py-0.5 text-xs font-normal text-destructive">
              {t("schedule.error.providerMissing")}
            </span>
          )}
        </div>
        <Switch checked={rule.enabled} onCheckedChange={onToggle} />
      </div>
      <div className="text-sm text-muted-foreground">
        {summarizeWindows(rule.windows, t)}
      </div>
      <div className="text-xs text-muted-foreground">
        {t("schedule.priorityBadge", { n: rule.priority })}
      </div>
      {rule.note && <div className="text-xs italic">"{rule.note}"</div>}
      <div className="flex gap-2 mt-2">
        <Button size="sm" variant="outline" onClick={onEdit}>
          <Pencil className="w-3 h-3 mr-1" />
          {t("common.edit")}
        </Button>
        <Button size="sm" variant="outline" onClick={onDelete}>
          <Trash2 className="w-3 h-3 mr-1" />
          {t("common.delete")}
        </Button>
      </div>
    </div>
  );
}
