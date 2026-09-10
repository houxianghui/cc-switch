import { describe, it, expect, vi, beforeAll } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import i18n from "i18next";
import { AddEditRuleDialog } from "@/components/schedule/AddEditRuleDialog";
import type { ScheduleRuleDto } from "@/lib/api/schedule";

const mutations = vi.hoisted(() => ({
  create: vi.fn(),
  update: vi.fn(),
}));

vi.mock("@/hooks/useScheduleRules", () => ({
  useCreateScheduleRule: () => ({
    mutate: mutations.create,
    isPending: false,
  }),
  useUpdateScheduleRule: () => ({
    mutate: mutations.update,
    isPending: false,
  }),
}));

vi.mock("@/hooks/useAppProviders", () => ({
  useAppProviders: (app: string) => ({
    data:
      app === "claude"
        ? { p1: { id: "p1", name: "Claude Provider" } }
        : { p9: { id: "p9", name: "Codex Provider" } },
    isPending: false,
  }),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
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

/** [app select, provider select] in DOM order. */
const selects = () => screen.getAllByTestId("select") as HTMLSelectElement[];

describe("AddEditRuleDialog", () => {
  beforeAll(() => {
    // A single real translation, so "did the message go through t()?" is
    // observable — the raw zod message is the bare i18n key.
    i18n.addResourceBundle(
      "zh",
      "translation",
      { schedule: { error: { providerRequired: "PICK-A-PROVIDER" } } },
      true,
      true,
    );
  });

  it("renders the add-rule heading and an app selector over every app", () => {
    render(<AddEditRuleDialog open onOpenChange={() => {}} app="claude" />);

    expect(screen.getByText("schedule.addRule")).toBeInTheDocument();
    const [appSelect] = selects();
    expect(appSelect).not.toBeDisabled();
    expect(appSelect.value).toBe("claude");
    expect(appSelect.querySelectorAll("option")).toHaveLength(9);
  });

  it("resets provider_id when the app changes", async () => {
    render(<AddEditRuleDialog open onOpenChange={() => {}} app="claude" />);

    const [appSelect, providerSelect] = selects();
    fireEvent.change(providerSelect, { target: { value: "p1" } });
    expect(selects()[1].value).toBe("p1");

    fireEvent.change(appSelect, { target: { value: "codex" } });

    expect(selects()[0].value).toBe("codex");
    // The provider list now comes from the newly selected app, and — the rule
    // dialog being a required field — carries no empty option.
    const providerOptions = [...selects()[1].querySelectorAll("option")].map(
      (o) => o.value,
    );
    expect(providerOptions).toEqual(["p9"]);
    // The native <select> stand-in cannot represent "nothing selected", so the
    // reset is observed through validation rather than through `.value`.
    fireEvent.click(screen.getByText("common.save"));
    expect(await screen.findByText("PICK-A-PROVIDER")).toBeInTheDocument();
    expect(mutations.create).not.toHaveBeenCalled();
  });

  it("switches to the edit heading and locks the app when given a rule", () => {
    render(
      <AddEditRuleDialog
        open
        onOpenChange={() => {}}
        app="claude"
        rule={rule}
      />,
    );

    expect(screen.getByText("schedule.editRule")).toBeInTheDocument();
    expect(selects()[0]).toBeDisabled();
    expect(selects()[1].value).toBe("p1");
  });

  it("renders a translated validation message instead of the raw key", async () => {
    render(<AddEditRuleDialog open onOpenChange={() => {}} app="claude" />);

    fireEvent.click(screen.getByText("common.save"));

    expect(await screen.findByText("PICK-A-PROVIDER")).toBeInTheDocument();
    expect(mutations.create).not.toHaveBeenCalled();
  });

  it("offers a labelled close affordance in the header", () => {
    render(<AddEditRuleDialog open onOpenChange={() => {}} app="claude" />);

    expect(screen.getByLabelText("common.close")).toBeInTheDocument();
  });

  it("labels every field so no input relies on a placeholder alone", () => {
    render(<AddEditRuleDialog open onOpenChange={() => {}} app="claude" />);

    expect(screen.getByLabelText("schedule.priority")).toBeInTheDocument();
    expect(screen.getByLabelText("schedule.note")).toBeInTheDocument();
  });
});
