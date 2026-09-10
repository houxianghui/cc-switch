import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { RuleCard } from "@/components/schedule/RuleCard";
import type { ScheduleRuleDto } from "@/lib/api/schedule";

const rule: ScheduleRuleDto = {
  id: "r1",
  app: "claude",
  provider_id: "p1",
  windows: [{ dow: [1, 2, 3, 4, 5], start: "09:00", end: "18:00" }],
  priority: 10,
  enabled: true,
  created_at: "2026-09-08T00:00:00+00:00",
  updated_at: "2026-09-08T00:00:00+00:00",
  note: "workday",
};

const providers = vi.hoisted(() => ({
  value: { data: {} as Record<string, unknown>, isPending: false },
}));

vi.mock("@/hooks/useAppProviders", () => ({
  useAppProviders: () => providers.value,
}));

describe("RuleCard", () => {
  it("renders the app label, provider name, localized summary and priority", () => {
    providers.value = {
      data: { p1: { id: "p1", name: "Provider A" } },
      isPending: false,
    };
    render(<RuleCard rule={rule} onEdit={() => {}} onDelete={() => {}} />);

    expect(screen.getByText(/apps\.claude → Provider A/)).toBeInTheDocument();
    expect(screen.getByText("schedule.summary.weekdays")).toBeInTheDocument();
    expect(screen.getByText("schedule.priorityBadge")).toBeInTheDocument();
    expect(screen.getByText(/workday/)).toBeInTheDocument();
    expect(
      screen.queryByText("schedule.error.providerMissing"),
    ).not.toBeInTheDocument();
  });

  it("badges a rule whose provider no longer exists", () => {
    providers.value = { data: {}, isPending: false };
    render(<RuleCard rule={rule} onEdit={() => {}} onDelete={() => {}} />);

    expect(
      screen.getByText("schedule.error.providerMissing"),
    ).toBeInTheDocument();
  });

  it("does not accuse a provider of being missing while the lookup is pending", () => {
    providers.value = { data: undefined as never, isPending: true };
    render(<RuleCard rule={rule} onEdit={() => {}} onDelete={() => {}} />);

    expect(
      screen.queryByText("schedule.error.providerMissing"),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/common\.loading/)).toBeInTheDocument();
  });
});
