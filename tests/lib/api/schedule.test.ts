import { describe, it, expect, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "list_schedule_rules") return [{ id: "r1", app: "claude", provider_id: "p1" }];
    if (cmd === "create_schedule_rule") return { id: "new" };
    return null;
  }),
}));

import { scheduleApi } from "@/lib/api/schedule";

describe("scheduleApi", () => {
  it("list_schedule_rules returns rules", async () => {
    const r = await scheduleApi.list("claude");
    expect(r[0].id).toBe("r1");
  });

  it("create_schedule_rule sends the request", async () => {
    const r = await scheduleApi.create({
      app: "claude", provider_id: "p1", windows: [], priority: 0, enabled: true, note: null,
    });
    expect(r.id).toBe("new");
  });
});
