import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Badge } from "@/components/ui/badge";
import { useScheduleSwitchLog } from "@/hooks/useScheduleSwitchLog";

/**
 * Collapsed by default: the page's job is the rule list, and the history is
 * something you go looking for rather than something you monitor.
 */
export function SwitchHistorySection() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const { data: entries, isPending, isError } = useScheduleSwitchLog();

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="space-y-2">
      <CollapsibleTrigger className="flex items-center gap-1 text-lg font-semibold">
        <ChevronRight
          className={`size-4 transition-transform ${open ? "rotate-90" : ""}`}
        />
        {t("schedule.history.title")}
      </CollapsibleTrigger>
      <CollapsibleContent>
        {isPending ? (
          <div className="text-sm text-muted-foreground">
            {t("common.loading")}
          </div>
        ) : isError ? (
          <div className="text-sm text-destructive">
            {t("schedule.history.loadFailed")}
          </div>
        ) : entries.length === 0 ? (
          <div className="text-sm text-muted-foreground">
            {t("schedule.history.empty")}
          </div>
        ) : (
          <ul className="divide-y rounded-md border">
            {entries.map((e) => (
              <li
                key={`${e.fired_at}-${e.app}-${e.provider_id}`}
                className="flex items-center gap-3 px-3 py-2 text-sm"
              >
                <span className="tabular-nums text-muted-foreground">
                  {new Date(e.fired_at).toLocaleString()}
                </span>
                <span className="text-muted-foreground">
                  {t(`apps.${e.app}`)}
                </span>
                <span className="flex-1 truncate font-medium">
                  {e.provider_name ?? e.provider_id}
                </span>
                <Badge variant="outline">
                  {t(`schedule.history.reason.${e.reason}`, e.reason)}
                </Badge>
              </li>
            ))}
          </ul>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
}
