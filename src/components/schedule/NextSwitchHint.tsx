import { useTranslation } from "react-i18next";
import { useNextScheduledSwitch } from "@/hooks/useScheduleRules";
import { useAppProviders } from "@/hooks/useAppProviders";
import { relativeTime } from "@/utils/scheduleWindows";
import type { AppId } from "@/lib/api";

/** One line per app that has an upcoming switch; renders nothing when there is none. */
export function NextSwitchHint({ app }: { app: AppId }) {
  const { t } = useTranslation();
  const { data: next } = useNextScheduledSwitch(app);
  const { data: providers } = useAppProviders(app);

  if (!next) return null;

  const provider = providers?.[next.provider_id]?.name ?? next.provider_id;
  return (
    <div className="text-xs text-muted-foreground">
      {t(`apps.${app}`)} ·{" "}
      {t("schedule.nextSwitch", {
        provider,
        rel: relativeTime(next.at, t),
      })}
    </div>
  );
}
