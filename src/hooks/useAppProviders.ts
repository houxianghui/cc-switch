import { useProvidersQuery } from "@/lib/query/queries";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";

/**
 * Read-only provider map for one app, keyed by provider id. Derived from
 * `useProvidersQuery` rather than fetching on its own: both would occupy the
 * `["providers", app]` cache entry and the first writer's shape would win,
 * which made every rule report its provider as missing.
 */
export function useAppProviders(app: AppId): {
  data: Record<string, Provider> | undefined;
  isPending: boolean;
  isError: boolean;
} {
  const { data, isPending, isError } = useProvidersQuery(app);
  return { data: data?.providers, isPending, isError };
}
