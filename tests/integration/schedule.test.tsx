import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  screen,
  waitFor,
  within,
  fireEvent,
} from "@testing-library/react";
import { describe, it, expect, beforeAll, vi } from "vitest";
import i18n from "i18next";
import { SchedulesPage } from "@/components/schedule/SchedulesPage";
import { useDeleteProviderMutation } from "@/lib/query/mutations";
import {
  createScheduleRule,
  listScheduleRules,
  setScheduleEvaluation,
  setScheduleHealthState,
} from "../msw/state";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastInfoMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: { open: boolean; children: React.ReactNode }) =>
    open ? <div data-testid="rule-dialog">{children}</div> : null,
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h2>{children}</h2>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogClose: (props: React.ComponentPropsWithoutRef<"button">) => (
    <button type="button" {...props} />
  ),
}));

// Radix's Select is not driveable in jsdom; a native <select> keeps the
// `value` / `onValueChange` contract observable.
vi.mock("@/components/ui/select", () => ({
  Select: ({
    value,
    disabled,
    onValueChange,
    children,
  }: {
    value: string;
    disabled?: boolean;
    onValueChange: (v: string) => void;
    children: React.ReactNode;
  }) => (
    <select
      data-testid="select"
      value={value}
      disabled={disabled}
      onChange={(e) => onValueChange(e.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
  SelectValue: () => null,
  SelectContent: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
  SelectItem: ({
    value,
    children,
  }: {
    value: string;
    children: React.ReactNode;
  }) => <option value={value}>{children}</option>,
}));

const renderPage = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SchedulesPage />
    </QueryClientProvider>,
  );
};

const clickButton = (name: string) =>
  fireEvent.click(screen.getByRole("button", { name }));

/** Drives the real delete-provider mutation, which owns the cascade toast. */
function DeleteProviderHarness({ providerId }: { providerId: string }) {
  const remove = useDeleteProviderMutation("claude");
  return (
    <button type="button" onClick={() => remove.mutate(providerId)}>
      delete-provider
    </button>
  );
}

const renderDeleteHarness = (providerId: string) => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DeleteProviderHarness providerId={providerId} />
    </QueryClientProvider>,
  );
};

/** [app select, provider select] inside the add/edit dialog, in DOM order. */
const dialogSelects = () =>
  within(screen.getByTestId("rule-dialog")).getAllByTestId(
    "select",
  ) as HTMLSelectElement[];

describe("SchedulesPage integration with MSW", () => {
  beforeAll(() => {
    // A real translation for the one string whose interpolated counts the test
    // needs to observe; every other key renders as the bare key.
    i18n.addResourceBundle(
      "zh",
      "translation",
      {
        schedule: { runNowResult: "Fired {{fired}}, skipped {{skipped}}" },
        notifications: {
          scheduleRulesCascadeDeleted: "Cascaded {{count}} rule(s)",
        },
      },
      true,
      true,
    );
  });

  it("renders the empty state when no rules are staged", async () => {
    renderPage();

    expect(await screen.findByText("schedule.empty")).toBeInTheDocument();
    expect(screen.queryByTestId("rule-card")).not.toBeInTheDocument();
  });

  it("creates a rule from the dialog and shows it in the list", async () => {
    renderPage();

    await screen.findByText("schedule.empty");
    clickButton("schedule.addRule");

    const providerSelect = dialogSelects()[1];
    await waitFor(() =>
      expect(
        within(providerSelect).getByRole("option", { name: "Claude Default" }),
      ).toBeInTheDocument(),
    );
    fireEvent.change(providerSelect, { target: { value: "claude-1" } });

    clickButton("common.save");

    const card = await screen.findByTestId("rule-card");
    expect(card).toHaveTextContent("apps.claude → Claude Default");
    expect(card).toHaveTextContent("schedule.priorityBadge");
    expect(screen.queryByText("schedule.empty")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByTestId("rule-dialog")).not.toBeInTheDocument(),
    );
    expect(listScheduleRules().map((rule) => rule.id)).toEqual(["rule-1"]);
  }, 15_000);

  it("reports the fired count in a toast when the scheduler is run now", async () => {
    setScheduleEvaluation({
      apps: [
        {
          app: "claude",
          fired: true,
          reason: "rule",
          from_provider: "claude-1",
          to_provider: "claude-2",
          skipped_due_to: null,
        },
      ],
    });

    renderPage();

    await screen.findByText("schedule.empty");
    clickButton("schedule.runNow");

    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Fired 1, skipped 0"),
    );
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("shows the degraded banner once the scheduler has failed three times", async () => {
    setScheduleHealthState({ consecutive_failures: 3 });

    renderPage();

    expect(
      await screen.findByText("schedule.health.degraded"),
    ).toBeInTheDocument();
  });

  it("reports the cascaded rule count in a toast when a referenced provider is deleted", async () => {
    const ruleWindow = { dow: [1], start: "09:00", end: "17:00" };
    createScheduleRule({
      app: "claude",
      provider_id: "claude-1",
      windows: [ruleWindow],
      priority: 0,
      enabled: true,
      note: null,
    });
    createScheduleRule({
      app: "claude",
      provider_id: "claude-1",
      windows: [ruleWindow],
      priority: 1,
      enabled: true,
      note: null,
    });
    // A rule on a different provider must survive the cascade.
    createScheduleRule({
      app: "claude",
      provider_id: "claude-2",
      windows: [ruleWindow],
      priority: 2,
      enabled: true,
      note: null,
    });

    renderDeleteHarness("claude-1");
    clickButton("delete-provider");

    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("Cascaded 2 rule(s)"),
    );
    expect(listScheduleRules().map((rule) => rule.provider_id)).toEqual([
      "claude-2",
    ]);
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("does not fire the cascade toast when no rule referenced the provider", async () => {
    renderDeleteHarness("claude-1");
    clickButton("delete-provider");

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
    expect(toastInfoMock).not.toHaveBeenCalled();
  });
});
