import type { TFunction } from "i18next";
import type { TimeWindowDto } from "@/lib/api/schedule";

type DowShape = "daily" | "weekdays" | "weekends" | "irregular";

function dowShape(dow: number[]): DowShape {
  const set = new Set(dow);
  if (set.size === 7) return "daily";
  if (set.size === 5 && [1, 2, 3, 4, 5].every((d) => set.has(d)))
    return "weekdays";
  if (set.size === 2 && set.has(0) && set.has(6)) return "weekends";
  return "irregular";
}

/**
 * Renders a rule's windows as one short line. `t` is passed in rather than
 * pulled from i18next's global instance so this stays a pure function.
 */
export function summarizeWindows(
  windows: TimeWindowDto[],
  t: TFunction,
): string {
  if (windows.length === 0) return "—";

  const groups = new Map<string, TimeWindowDto>();
  for (const w of windows) {
    const key = `${[...w.dow].sort((a, b) => a - b).join(",")}|${w.start}|${w.end}`;
    if (!groups.has(key)) groups.set(key, w);
  }
  const ordered = [...groups.values()];
  const shapes = ordered.map((w) => dowShape(w.dow));

  // Anything that does not reduce to daily / weekdays / weekends, or more than
  // two distinct groups, is summarised by count instead of spelled out.
  if (shapes.some((s) => s === "irregular") || groups.size > 2) {
    return t("schedule.summary.complex", { n: windows.length });
  }

  const sameShape = shapes.every((s) => s === shapes[0]);
  return ordered
    .map((w, i) =>
      sameShape && i > 0
        ? `${w.start}-${w.end}`
        : t(`schedule.summary.${shapes[i]}`, { start: w.start, end: w.end }),
    )
    .join(", ");
}

/** Coarse "in 5m" / "in 3h" / "in 2d" label; past instants clamp to zero. */
export function relativeTime(
  iso: string,
  t: TFunction,
  now = new Date(),
): string {
  const target = new Date(iso);
  const mins = Math.max(
    0,
    Math.round((target.getTime() - now.getTime()) / 60000),
  );
  if (mins < 60) return t("schedule.rel.minutes", { n: mins });
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return t("schedule.rel.hours", { n: hrs });
  return t("schedule.rel.days", { n: Math.round(hrs / 24) });
}
