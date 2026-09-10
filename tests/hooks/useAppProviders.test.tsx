import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

const mockProviders = {
  p1: { id: "p1", name: "Provider One" },
  p2: { id: "p2", name: "Provider Two" },
} as any;

vi.mock("@/lib/api", () => ({
  providersApi: {
    getAll: vi.fn(async (app: string) => (app === "claude" ? mockProviders : {})),
    getCurrent: vi.fn(async () => "p1"),
  },
}));

import { useAppProviders } from "@/hooks/useAppProviders";
import { useProvidersQuery } from "@/lib/query/queries";

const qc = () => new QueryClient({ defaultOptions: { queries: { retry: false } } });
const wrap = (client: QueryClient) => ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={client}>{children}</QueryClientProvider>
);

describe("useAppProviders", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns a flat provider map keyed by id", async () => {
    const client = qc();
    const { result } = renderHook(() => useAppProviders("claude"), {
      wrapper: wrap(client),
    });
    await waitFor(() => expect(result.current.data).toBeDefined());
    expect(result.current.data?.p1?.name).toBe("Provider One");
  });

  // Regression: both hooks occupy the ["providers", app] cache entry. When the
  // main provider list had already populated it with {providers, currentProviderId},
  // this hook read that wrapper and reported every provider as missing.
  it("resolves providers when useProvidersQuery already populated the cache", async () => {
    const client = qc();
    const { result: listResult } = renderHook(() => useProvidersQuery("claude"), {
      wrapper: wrap(client),
    });
    await waitFor(() => expect(listResult.current.isSuccess).toBe(true));

    const { result } = renderHook(() => useAppProviders("claude"), {
      wrapper: wrap(client),
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));

    expect(result.current.data?.p1?.name).toBe("Provider One");
    expect(result.current.data?.p2?.name).toBe("Provider Two");
  });

  it("does not report a provider as missing after the list query resolves", async () => {
    const client = qc();
    const { result: listResult } = renderHook(() => useProvidersQuery("claude"), {
      wrapper: wrap(client),
    });
    await waitFor(() => expect(listResult.current.isSuccess).toBe(true));

    const { result } = renderHook(() => useAppProviders("claude"), {
      wrapper: wrap(client),
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));

    // This is exactly what RuleCard computes.
    const providerMissing = !result.current.isPending && !result.current.data?.p1;
    expect(providerMissing).toBe(false);
  });
});