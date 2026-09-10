import { describe, it, expect } from "vitest";
import { newRuleSchema } from "@/lib/schemas/schedule";

describe("newRuleSchema", () => {
  it("rejects empty windows", () => {
    const r = newRuleSchema.safeParse({
      app: "claude", provider_id: "p1", windows: [],
      priority: 0, enabled: true, note: null,
    });
    expect(r.success).toBe(false);
  });
  it("rejects cross-midnight", () => {
    const r = newRuleSchema.safeParse({
      app: "claude", provider_id: "p1",
      windows: [{ dow: [1], start: "22:00", end: "06:00" }],
      priority: 0, enabled: true, note: null,
    });
    expect(r.success).toBe(false);
  });
  it("rejects zero-length", () => {
    const r = newRuleSchema.safeParse({
      app: "claude", provider_id: "p1",
      windows: [{ dow: [1], start: "10:00", end: "10:00" }],
      priority: 0, enabled: true, note: null,
    });
    expect(r.success).toBe(false);
  });
  it("rejects empty dow", () => {
    const r = newRuleSchema.safeParse({
      app: "claude", provider_id: "p1",
      windows: [{ dow: [], start: "10:00", end: "12:00" }],
      priority: 0, enabled: true, note: null,
    });
    expect(r.success).toBe(false);
  });
  it("accepts a valid rule", () => {
    const r = newRuleSchema.safeParse({
      app: "claude", provider_id: "p1",
      windows: [{ dow: [1,2,3,4,5], start: "09:00", end: "18:00" }],
      priority: 0, enabled: true, note: null,
    });
    expect(r.success).toBe(true);
  });
});
