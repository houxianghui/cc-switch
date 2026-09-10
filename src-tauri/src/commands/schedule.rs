//! 定时切换调度命令
//!
//! 对应 spec: docs/superpowers/specs/2026-09-08-scheduled-provider-switching-design.md
//! 9 个 Tauri IPC 命令，覆盖规则 CRUD、回退 provider、即时评估、下次切换预告、健康状态。

use crate::error::AppError;
use crate::schedule_rules::{
    EvaluationReport, NewScheduleRuleRequest, NextSwitch, ScheduleHealth, ScheduleRule,
    ScheduleRulePatch, SwitchLogEntry,
};
use crate::services::schedule::ScheduleServiceState;
use crate::store::AppState;

#[tauri::command]
pub async fn list_schedule_rules(
    state: tauri::State<'_, AppState>,
    app: Option<String>,
) -> Result<Vec<ScheduleRule>, AppError> {
    state.db.list_schedule_rules(app.as_deref())
}

#[tauri::command]
pub async fn create_schedule_rule(
    state: tauri::State<'_, AppState>,
    new_rule: NewScheduleRuleRequest,
) -> Result<ScheduleRule, AppError> {
    state.db.create_schedule_rule(&new_rule)
}

#[tauri::command]
pub async fn update_schedule_rule(
    state: tauri::State<'_, AppState>,
    id: String,
    patch: ScheduleRulePatch,
) -> Result<ScheduleRule, AppError> {
    state.db.update_schedule_rule(&id, &patch)
}

#[tauri::command]
pub async fn delete_schedule_rule(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.db.delete_schedule_rule(&id)
}

#[tauri::command]
pub async fn get_fallback_provider(
    state: tauri::State<'_, AppState>,
    app: String,
) -> Result<Option<String>, AppError> {
    state.db.get_fallback_provider(&app)
}

#[tauri::command]
pub async fn set_fallback_provider(
    state: tauri::State<'_, AppState>,
    app: String,
    provider_id: Option<String>,
) -> Result<(), AppError> {
    state.db.set_fallback_provider(&app, provider_id.as_deref())
}

/// 立即评估调度：`app` 为 `Some` 时只评估该 app，为 `None` 时评估全部。
///
/// 只依赖 `ScheduleServiceState`；服务自身持有 `Arc<Database>`，
/// 因此这里不需要 `AppState`（brief 里的 `state` 形参未被使用，会触发
/// `unused_variables`，在 `clippy -D warnings` 下阻断 CI，故移除）。
///
/// 全量评估走 `tick_once`，这样 UI 的「Run now」重试也会刷新健康状态（AC7）。
#[tauri::command]
pub async fn evaluate_schedule_now(
    svc: tauri::State<'_, ScheduleServiceState>,
    app: Option<String>,
) -> Result<EvaluationReport, AppError> {
    match app {
        Some(a) => {
            let r = svc.0.evaluate_for_app(&a).await?;
            Ok(EvaluationReport { apps: vec![r] })
        }
        None => svc.0.tick_once().await,
    }
}

#[tauri::command]
pub async fn get_next_scheduled_switch(
    svc: tauri::State<'_, ScheduleServiceState>,
    app: String,
) -> Result<Option<NextSwitch>, AppError> {
    Ok(svc.0.next_switch_for(&app, chrono::Local::now()))
}

/// 定时切换历史：`app` 为 `None` 时返回全部应用，按时间倒序。
///
/// `limit` 由前端给定并在此夹到 500，避免一次把整张表（每 app 上限 1000 行）
/// 序列化过 IPC。
#[tauri::command]
pub async fn list_schedule_switch_log(
    state: tauri::State<'_, AppState>,
    app: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<SwitchLogEntry>, AppError> {
    let limit = limit.unwrap_or(50).clamp(1, 500);
    state.db.list_switch_log(app.as_deref(), limit)
}

#[tauri::command]
pub async fn get_schedule_health(
    state: tauri::State<'_, AppState>,
) -> Result<ScheduleHealth, AppError> {
    let last_tick_at = state.db.get_setting("schedule_last_tick_at").ok().flatten();
    // A successful tick clears the error by storing an empty string; the health
    // contract exposes "no error" as null.
    let last_error = state
        .db
        .get_setting("schedule_last_tick_error")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let consecutive: u32 = state
        .db
        .get_setting("schedule_consecutive_failures")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(ScheduleHealth {
        last_tick_at,
        last_tick_error: last_error,
        consecutive_failures: consecutive,
    })
}

#[cfg(test)]
mod tests {
    use crate::database::Database;

    // Tauri 的 State 需要真实 App；这里直接测试 DAO 层。
    // 命令体是 1 行委托，集成由前端 schedule.test.tsx 覆盖。
    #[test]
    fn fallback_round_trip_via_dao() {
        let db = Database::memory().unwrap();
        // app_fallback_providers 上有指向 providers 的外键，先插入父行。
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
                 VALUES ('p1', 'claude', 'claude-p1', '{}', '{}', 0)",
                [],
            )
            .unwrap();
        }

        assert_eq!(db.get_fallback_provider("claude").unwrap(), None);
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        assert_eq!(
            db.get_fallback_provider("claude").unwrap().as_deref(),
            Some("p1")
        );
        db.set_fallback_provider("claude", None).unwrap();
        assert_eq!(db.get_fallback_provider("claude").unwrap(), None);
    }
}
