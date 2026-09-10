import { describe, it, expect, vi } from "vitest";
import i18n from "i18next";
import type { TFunction } from "i18next";
import { relativeTime, summarizeWindows } from "@/utils/scheduleWindows";
import type { TimeWindowDto } from "@/lib/api/schedule";

// The test harness initialises i18next with empty resources, so `t(key)`
// returns the key itself — assertions below are on keys, not English prose.
const t = i18n.t.bind(i18n) as TFunction;

/** Records what the utility asked i18next for, so interpolation args are visible. */
function spyT() {
  return vi.fn(
    (key: string, options?: Record<string, unknown>) =>
      `${key}(${JSON.stringify(options ?? {})})`,
  ) as unknown as TFunction;
}

describe("summarizeWindows", () => {
  it("daily window uses the daily key", () => {
    const w: TimeWindowDto = {
      dow: [0, 1, 2, 3, 4, 5, 6],
      start: "09:00",
      end: "18:00",
    };
    expect(summarizeWindows([w], t)).toBe("schedule.summary.daily");
  });

  it("weekdays window uses the weekdays key", () => {
    const w: TimeWindowDto = {
      dow: [1, 2, 3, 4, 5],
      start: "09:00",
      end: "18:00",
    };
    expect(summarizeWindows([w], t)).toBe("schedule.summary.weekdays");
  });

  it("weekends window uses the weekends key", () => {
    const w: TimeWindowDto = { dow: [0, 6], start: "10:00", end: "14:00" };
    expect(summarizeWindows([w], t)).toBe("schedule.summary.weekends");
  });

  it("passes start and end to the summary key", () => {
    const spy = spyT();
    const w: TimeWindowDto = {
      dow: [1, 2, 3, 4, 5],
      start: "09:00",
      end: "18:00",
    };
    expect(summarizeWindows([w], spy)).toBe(
      'schedule.summary.weekdays({"start":"09:00","end":"18:00"})',
    );
  });

  it("multi-window of the same shape localizes only the first block", () => {
    const ws: TimeWindowDto[] = [
      { dow: [1, 2, 3, 4, 5], start: "09:00", end: "12:00" },
      { dow: [1, 2, 3, 4, 5], start: "14:00", end: "18:00" },
    ];
    expect(summarizeWindows(ws, t)).toBe(
      "schedule.summary.weekdays, 14:00-18:00",
    );
  });

  it("mixed shapes localize each block", () => {
    const ws: TimeWindowDto[] = [
      { dow: [1, 2, 3, 4, 5], start: "09:00", end: "12:00" },
      { dow: [0, 6], start: "14:00", end: "18:00" },
    ];
    expect(summarizeWindows(ws, t)).toBe(
      "schedule.summary.weekdays, schedule.summary.weekends",
    );
  });

  it("complex (irregular dow) falls back to the count key", () => {
    const ws: TimeWindowDto[] = [
      { dow: [1, 3, 5], start: "09:00", end: "12:00" },
      { dow: [2, 4], start: "14:00", end: "18:00" },
    ];
    const spy = spyT();
    expect(summarizeWindows(ws, spy)).toBe('schedule.summary.complex({"n":2})');
  });

  it("does not mutate the input windows", () => {
    const w: TimeWindowDto = {
      dow: [5, 1, 3, 2, 4],
      start: "09:00",
      end: "18:00",
    };
    summarizeWindows([w], t);
    expect(w.dow).toEqual([5, 1, 3, 2, 4]);
  });

  it("renders a dash for no windows", () => {
    expect(summarizeWindows([], t)).toBe("—");
  });
});

describe("relativeTime", () => {
  const now = new Date("2026-09-08T12:00:00Z");

  it("uses minutes below an hour", () => {
    const spy = spyT();
    expect(relativeTime("2026-09-08T12:30:00Z", spy, now)).toBe(
      'schedule.rel.minutes({"n":30})',
    );
  });

  it("uses hours below a day", () => {
    const spy = spyT();
    expect(relativeTime("2026-09-08T17:00:00Z", spy, now)).toBe(
      'schedule.rel.hours({"n":5})',
    );
  });

  it("uses days beyond a day", () => {
    const spy = spyT();
    expect(relativeTime("2026-09-11T12:00:00Z", spy, now)).toBe(
      'schedule.rel.days({"n":3})',
    );
  });

  it("clamps past instants to zero minutes instead of an English literal", () => {
    const spy = spyT();
    expect(relativeTime("2026-09-08T11:00:00Z", spy, now)).toBe(
      'schedule.rel.minutes({"n":0})',
    );
  });
});
