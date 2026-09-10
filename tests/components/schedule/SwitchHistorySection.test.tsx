import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SwitchHistorySection } from "@/components/schedule/SwitchHistorySection";
import type { SwitchLogEntryDto } from "@/lib/api/schedule";

const listSwitchLog = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/schedule", () => ({
  scheduleApi: { listSwitchLog },
}));

const entry = (patch: Partial<SwitchLogEntryDto> = {}): SwitchLogEntryDto => ({
  app: "claude",
  provider_id: "p1",
  provider_name: "Provider A",
  fired_at: "2026-09-10T09:00:23+08:00",
  reason: "rule",
  ...patch,
});

function renderSection() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, throwOnError: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SwitchHistorySection />
    </QueryClientProvider>,
  );
}

const expand = () =>
  fireEvent.click(screen.getByText("schedule.history.title"));

describe("SwitchHistorySection", () => {
  beforeEach(() => {
    listSwitchLog.mockReset();
  });

  it("stays collapsed until the heading is clicked", async () => {
    listSwitchLog.mockResolvedValue([entry()]);
    renderSection();

    expect(screen.queryByText("Provider A")).not.toBeInTheDocument();

    expand();

    expect(await screen.findByText("Provider A")).toBeInTheDocument();
  });

  it("shows the empty state when nothing has fired", async () => {
    listSwitchLog.mockResolvedValue([]);
    renderSection();
    expand();

    expect(
      await screen.findByText("schedule.history.empty"),
    ).toBeInTheDocument();
  });

  it("falls back to the raw id when the provider was deleted", async () => {
    listSwitchLog.mockResolvedValue([
      entry({ provider_id: "gone-123", provider_name: null }),
    ]);
    renderSection();
    expand();

    expect(await screen.findByText("gone-123")).toBeInTheDocument();
  });

  it("surfaces a load failure instead of an empty list", async () => {
    listSwitchLog.mockRejectedValue(new Error("boom"));
    renderSection();
    expand();

    expect(
      await screen.findByText("schedule.history.loadFailed"),
    ).toBeInTheDocument();
  });
});
