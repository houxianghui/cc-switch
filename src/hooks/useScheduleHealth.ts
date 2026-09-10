import { useQuery } from "@tanstack/react-query";
import { scheduleApi } from "@/lib/api/schedule";

export const SCHEDULE_DEGRADED_FAILURE_THRESHOLD = 3;

export function useScheduleHealth() {
  return useQuery({
    queryKey: ["schedule", "health"],
    queryFn: () => scheduleApi.getHealth(),
    refetchInterval: 60_000,
  });
}
