import { describe, it, expect, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

vi.mock("@/lib/api/schedule", () => ({
  scheduleApi: {
    list: vi.fn(async (app?: string) => app ? [{ id: "r1", app, provider_id: "p1" }] : []),
    create: vi.fn(async (r) => ({ ...r, id: "new" })),
    update: vi.fn(async (id, patch) => ({ id, ...patch })),
    remove: vi.fn(async () => {}),
    evaluateNow: vi.fn(async () => ({ apps: [] })),
    getNext: vi.fn(async () => null),
    getFallback: vi.fn(async () => null),
    setFallback: vi.fn(async () => {}),
    getHealth: vi.fn(async () => ({ last_tick_at: null, last_tick_error: null, consecutive_failures: 0 })),
  },
}));

import { useScheduleRules } from "@/hooks/useScheduleRules";

const qc = () => new QueryClient({ defaultOptions: { queries: { retry: false } } });
const wrap = (client: QueryClient) => ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={client}>{children}</QueryClientProvider>
);

describe("useScheduleRules", () => {
  it("returns rules from api", async () => {
    const client = qc();
    const { result } = renderHook(() => useScheduleRules("claude"), { wrapper: wrap(client) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.[0].id).toBe("r1");
  });
});
