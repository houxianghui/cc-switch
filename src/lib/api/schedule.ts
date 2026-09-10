import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "@/lib/api";

export interface TimeWindowDto {
  dow: number[];
  start: string;
  end: string;
}

export interface ScheduleRuleDto {
  id: string;
  app: AppId;
  provider_id: string;
  windows: TimeWindowDto[];
  priority: number;
  enabled: boolean;
  created_at: string;
  updated_at: string;
  note: string | null;
}

export interface NewScheduleRuleDto {
  app: AppId;
  provider_id: string;
  windows: TimeWindowDto[];
  priority: number;
  enabled: boolean;
  note: string | null;
}

export interface ScheduleRulePatchDto {
  provider_id?: string;
  windows?: TimeWindowDto[];
  priority?: number;
  enabled?: boolean;
  note?: string | null;
}

export interface NextSwitchDto {
  app: AppId;
  provider_id: string;
  at: string;
  reason: string;
}

export interface ScheduleHealthDto {
  last_tick_at: string | null;
  last_tick_error: string | null;
  consecutive_failures: number;
}

export interface SwitchLogEntryDto {
  app: AppId;
  provider_id: string;
  provider_name: string | null;
  fired_at: string;
  reason: string;
}

export interface AppEvalReportDto {
  app: AppId;
  fired: boolean;
  reason: string;
  from_provider: string | null;
  to_provider: string | null;
  skipped_due_to: string | null;
}

export interface EvaluationReportDto {
  apps: AppEvalReportDto[];
}

export const scheduleApi = {
  list: (app?: AppId) =>
    invoke<ScheduleRuleDto[]>("list_schedule_rules", { app }),
  create: (newRule: NewScheduleRuleDto) =>
    invoke<ScheduleRuleDto>("create_schedule_rule", { newRule }),
  update: (id: string, patch: ScheduleRulePatchDto) =>
    invoke<ScheduleRuleDto>("update_schedule_rule", { id, patch }),
  remove: (id: string) => invoke<void>("delete_schedule_rule", { id }),
  evaluateNow: (app?: AppId) =>
    invoke<EvaluationReportDto>("evaluate_schedule_now", { app }),
  getNext: (app: AppId) =>
    invoke<NextSwitchDto | null>("get_next_scheduled_switch", { app }),
  getHealth: () => invoke<ScheduleHealthDto>("get_schedule_health"),
  listSwitchLog: (app?: AppId, limit?: number) =>
    invoke<SwitchLogEntryDto[]>("list_schedule_switch_log", { app, limit }),
};
