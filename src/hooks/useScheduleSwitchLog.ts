import { useQuery } from "@tanstack/react-query";
import { scheduleApi } from "@/lib/api/schedule";

export const SWITCH_LOG_PAGE_SIZE = 20;

export function useScheduleSwitchLog(limit: number = SWITCH_LOG_PAGE_SIZE) {
  return useQuery({
    queryKey: ["schedule", "switchLog", limit],
    queryFn: () => scheduleApi.listSwitchLog(undefined, limit),
  });
}
