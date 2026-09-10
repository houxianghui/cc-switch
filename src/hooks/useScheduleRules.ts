import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  scheduleApi,
  type NewScheduleRuleDto,
  type ScheduleRulePatchDto,
} from "@/lib/api/schedule";
import type { AppId } from "@/lib/api";

const KEYS = {
  rules: (app?: AppId) => ["schedule", "rules", app ?? "all"] as const,
  next: (app: AppId) => ["schedule", "next", app] as const,
};

export function useScheduleRules(app?: AppId) {
  return useQuery({
    queryKey: KEYS.rules(app),
    queryFn: () => scheduleApi.list(app),
    refetchInterval: 60_000,
  });
}
export function useCreateScheduleRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (r: NewScheduleRuleDto) => scheduleApi.create(r),
    onSuccess: (_, r) => {
      qc.invalidateQueries({ queryKey: ["schedule", "rules"] });
      qc.invalidateQueries({ queryKey: ["schedule", "next", r.app] });
    },
  });
}
export function useUpdateScheduleRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: ScheduleRulePatchDto }) =>
      scheduleApi.update(id, patch),
    onSuccess: (rule) => {
      qc.invalidateQueries({ queryKey: ["schedule", "rules"] });
      qc.invalidateQueries({ queryKey: ["schedule", "next", rule.app] });
    },
  });
}
export function useDeleteScheduleRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => scheduleApi.remove(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["schedule", "rules"] }),
  });
}
export function useEvaluateScheduleNow() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (app?: AppId) => scheduleApi.evaluateNow(app),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["schedule"] });
      qc.invalidateQueries({ queryKey: ["providers"] });
    },
  });
}
export function useNextScheduledSwitch(app: AppId) {
  return useQuery({
    queryKey: KEYS.next(app),
    queryFn: () => scheduleApi.getNext(app),
    refetchInterval: 60_000,
  });
}
