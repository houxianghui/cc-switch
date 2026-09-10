//! 定时切换规则相关类型定义
//!
//! 这些类型是 Scheduled Provider Switching 功能的 schema 基础。
//! 整个模块是纯类型定义，不涉及 IO、SQL 或业务逻辑。
//!
//! 对应 spec: docs/superpowers/specs/2026-09-08-scheduled-provider-switching-design.md
//! 对应数据库表（schema v19）：schedule_rules / app_schedule_state / app_fallback_providers / schedule_switch_log

use chrono::{Datelike, Local, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 切换来源 — 区分用户主动切换、定时切换、深链接触发、初始默认值
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwitchSource {
    /// 用户在 UI 里手动触发
    Manual,
    /// 定时调度器触发
    Scheduled,
    /// 深链接（ccswitch:// URL）触发
    Deeplink,
    /// 应用首次启动时的初始状态
    Initial,
}

/// 一个时间窗口：周内某几天内的某段时间（半开区间 [start, end)）
///
/// 跨午夜窗口不在 v1 范围（spec 1 节「Non-goals」）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeWindow {
    /// 周内日列表，0 = 周日，6 = 周六
    pub dow: Vec<u8>,
    /// 开始时间，格式 "HH:MM"，包含
    pub start: String,
    /// 结束时间，格式 "HH:MM"，不包含（半开区间）
    pub end: String,
}

impl TimeWindow {
    /// 判断给定时刻是否落在本窗口内
    ///
    /// 行为：
    /// - 星期不匹配 → false
    /// - `start` / `end` 解析失败 → false（防御性，不 panic）
    /// - 时刻 ∈ [start, end) → true，否则 false
    pub fn matches(&self, now: chrono::DateTime<Local>) -> bool {
        let dow = now.weekday().num_days_from_sunday() as u8;
        if !self.dow.contains(&dow) {
            return false;
        }
        let s = match NaiveTime::parse_from_str(&self.start, "%H:%M") {
            Ok(t) => t,
            Err(_) => return false,
        };
        let e = match NaiveTime::parse_from_str(&self.end, "%H:%M") {
            Ok(t) => t,
            Err(_) => return false,
        };
        let t = now.time();
        s <= t && t < e
    }
}

/// 一条调度规则：(app, provider) 在 windows 描述的时段内应被激活
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleRule {
    /// UUID v4
    pub id: String,
    /// AppType 枚举的字符串值（如 "claude"）
    pub app: String,
    /// 关联的 provider id（软引用 providers.id）
    pub provider_id: String,
    /// 触发时段列表（至少 1 个）
    pub windows: Vec<TimeWindow>,
    /// 优先级，数值越大越优先；同优先级按 updated_at DESC, id DESC 决胜
    pub priority: i32,
    /// 是否启用
    pub enabled: bool,
    /// ISO8601 字符串
    pub created_at: String,
    /// ISO8601 字符串
    pub updated_at: String,
    /// 可选备注
    pub note: Option<String>,
}

/// 新建规则请求 — 前端通过 IPC 传入，由 service 层校验后生成 id / 时间戳
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewScheduleRuleRequest {
    pub app: String,
    pub provider_id: String,
    pub windows: Vec<TimeWindow>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub note: Option<String>,
}

fn default_true() -> bool {
    true
}

/// 校验窗口列表：至少一个窗口，dow ∈ 0..=6 且非空，start < end
///
/// create 与 update 共用：后端是唯一的可信边界（spec 5.2），前端 zod 不是。
pub fn validate_windows(windows: &[TimeWindow]) -> Result<(), String> {
    if windows.is_empty() {
        return Err("at least one window required".into());
    }
    for w in windows {
        if w.dow.is_empty() {
            return Err("window dow must be non-empty".into());
        }
        if let Some(bad) = w.dow.iter().find(|d| **d > 6) {
            return Err(format!("window dow must be 0..=6, got {bad}"));
        }
        let s = NaiveTime::parse_from_str(&w.start, "%H:%M").map_err(|e| e.to_string())?;
        let e = NaiveTime::parse_from_str(&w.end, "%H:%M").map_err(|e| e.to_string())?;
        if s >= e {
            return Err("window start must be < end".into());
        }
    }
    Ok(())
}

/// 校验优先级范围（create 与 update 共用）
pub fn validate_priority(priority: i32) -> Result<(), String> {
    if !(0..=1000).contains(&priority) {
        return Err("priority must be 0..=1000".into());
    }
    Ok(())
}

impl NewScheduleRuleRequest {
    /// 校验请求合法性，返回 `Err(msg)` 说明第一个失败的字段
    pub fn validate(&self) -> Result<(), String> {
        validate_windows(&self.windows)?;
        validate_priority(self.priority)
    }
}

/// 局部更新规则请求 — 所有字段都是 Option，None 表示不修改
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScheduleRulePatch {
    pub provider_id: Option<String>,
    pub windows: Option<Vec<TimeWindow>>,
    pub priority: Option<i32>,
    pub enabled: Option<bool>,
    /// `Some(None)` 表示把 note 置空；`None` 表示不动
    pub note: Option<Option<String>>,
}

impl ScheduleRulePatch {
    /// 校验补丁里实际出现的字段；`None` 字段不修改，因此不校验
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ref windows) = self.windows {
            validate_windows(windows)?;
        }
        if let Some(priority) = self.priority {
            validate_priority(priority)?;
        }
        Ok(())
    }
}

/// 下一次切换的预告信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NextSwitch {
    pub app: String,
    pub provider_id: String,
    /// ISO8601
    pub at: String,
    /// "rule" | "fallback"
    pub reason: String,
}

/// 一条已发生的定时切换记录（切换历史面板）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwitchLogEntry {
    pub app: String,
    pub provider_id: String,
    /// 记录当时的供应商名；供应商被删除后为 `None`，前端回退显示 id。
    pub provider_name: Option<String>,
    /// ISO8601
    pub fired_at: String,
    /// "rule" | "fallback"
    pub reason: String,
}

/// 调度器健康状态（用于前端徽标 + 重试提示）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleHealth {
    pub last_tick_at: Option<String>,
    pub last_tick_error: Option<String>,
    pub consecutive_failures: u32,
}

/// 一次完整 evaluate 的结果（按 app 分组）
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EvaluationReport {
    pub apps: Vec<AppEvalReport>,
}

/// 单个 app 的评估结果
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppEvalReport {
    pub app: String,
    pub fired: bool,
    pub reason: String,
    pub from_provider: Option<String>,
    pub to_provider: Option<String>,
    pub skipped_due_to: Option<String>,
}

/// 生成新的规则 id（UUID v4 字符串）
pub fn new_rule_id() -> String {
    Uuid::new_v4().to_string()
}

/// 规则排序键：priority DESC → updated_at DESC → id DESC
///
/// 对应 spec 5.3 节的决胜顺序（D4 / E6）。
pub(crate) fn rule_sort_key(
    r: &ScheduleRule,
) -> (
    std::cmp::Reverse<i32>,
    std::cmp::Reverse<String>,
    std::cmp::Reverse<String>,
) {
    (
        std::cmp::Reverse(r.priority),
        std::cmp::Reverse(r.updated_at.clone()),
        std::cmp::Reverse(r.id.clone()),
    )
}

/// 解析 `now` 时刻应生效的规则
///
/// 纯函数：先过滤 `enabled`，再过滤命中任一窗口的规则，
/// 最后按 priority DESC / updated_at DESC / id DESC 取第一条。
pub fn resolve_active_rule(
    rules: &[ScheduleRule],
    now: chrono::DateTime<Local>,
) -> Option<&ScheduleRule> {
    let mut candidates: Vec<&ScheduleRule> = rules
        .iter()
        .filter(|r| r.enabled)
        .filter(|r| r.windows.iter().any(|w| w.matches(now)))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|r| rule_sort_key(r));
    candidates.into_iter().next()
}

/// 返回 `now` 所处窗口的起始时刻（由 `resolve_active_rule` 选出的规则决定）
///
/// 无规则覆盖时返回 `None`。用于维护 `last_window_start_at` 账本（spec 5.4）。
pub fn current_window_start_for(
    rules: &[ScheduleRule],
    now: chrono::DateTime<Local>,
) -> Option<chrono::DateTime<Local>> {
    let r = resolve_active_rule(rules, now)?;
    let w = r.windows.iter().find(|w| w.matches(now))?;
    let s = NaiveTime::parse_from_str(&w.start, "%H:%M").ok()?;
    // 向前回溯最近一个「星期在 w.dow 内且 start 时刻 <= now」的日期
    let mut date = now.date_naive();
    for _ in 0..7 {
        if w.dow
            .contains(&(date.weekday().num_days_from_sunday() as u8))
        {
            let candidate = date.and_time(s).and_local_timezone(Local).single()?;
            if candidate <= now {
                return Some(candidate);
            }
        }
        date = date.pred_opt()?;
    }
    None
}

/// 返回 `now` 之前（含）最近一个已结束窗口的结束时刻
///
/// fallback 决策的「本次决策何时开始」基线：规则窗口一结束，fallback 决策即开始，
/// 手动 pin 也随之释放（spec 5.4 / E8）。搜索范围为过去 8 天，覆盖整周 + 当天。
pub fn previous_window_end_before(
    rules: &[ScheduleRule],
    now: chrono::DateTime<Local>,
) -> Option<chrono::DateTime<Local>> {
    let mut date = now.date_naive();
    let mut ends: Vec<chrono::DateTime<Local>> = Vec::new();
    for _ in 0..8 {
        let dow = date.weekday().num_days_from_sunday() as u8;
        for r in rules.iter().filter(|r| r.enabled) {
            for w in r.windows.iter().filter(|w| w.dow.contains(&dow)) {
                let end = match NaiveTime::parse_from_str(&w.end, "%H:%M") {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                // DST 折叠/跳变时该本地时刻不唯一或不存在，跳过而不是放弃整次搜索。
                if let Some(candidate) = date.and_time(end).and_local_timezone(Local).single() {
                    if candidate <= now {
                        ends.push(candidate);
                    }
                }
            }
        }
        date = match date.pred_opt() {
            Some(d) => d,
            None => break,
        };
    }
    ends.into_iter().max()
}

#[cfg(test)]
mod tests {
    use super::*;
    // TimeZone is only needed by the fixtures below; keeping it at module level makes
    // it an unused import in the non-test build that CI lints.
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    fn rule(id: &str, prio: i32, ws: Vec<TimeWindow>) -> ScheduleRule {
        ScheduleRule {
            id: id.into(),
            app: "claude".into(),
            provider_id: "p".into(),
            windows: ws,
            priority: prio,
            enabled: true,
            created_at: "2026-09-08T00:00:00+00:00".into(),
            updated_at: "2026-09-08T00:00:00+00:00".into(),
            note: None,
        }
    }

    fn wdow() -> Vec<TimeWindow> {
        vec![TimeWindow {
            dow: vec![1, 2, 3, 4, 5],
            start: "09:00".into(),
            end: "18:00".into(),
        }]
    }

    #[test]
    fn window_matches_dow_and_half_open() {
        let w = TimeWindow {
            dow: vec![1, 2, 3, 4, 5],
            start: "09:00".into(),
            end: "18:00".into(),
        };
        // Mon 2026-09-07 09:00 — start inclusive
        assert!(w.matches(at(2026, 9, 7, 9, 0)));
        // Mon 2026-09-07 17:59 — in range
        assert!(w.matches(at(2026, 9, 7, 17, 59)));
        // Mon 2026-09-07 18:00 — end exclusive
        assert!(!w.matches(at(2026, 9, 7, 18, 0)));
        // Sun 2026-09-06 12:00 — wrong day
        assert!(!w.matches(at(2026, 9, 6, 12, 0)));
    }

    #[test]
    fn new_rule_request_validates_windows() {
        use uuid::Uuid;

        // valid case
        let r = NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "p1".into(),
            windows: vec![TimeWindow {
                dow: vec![1],
                start: "09:00".into(),
                end: "12:00".into(),
            }],
            priority: 0,
            enabled: true,
            note: None,
        };
        assert!(r.validate().is_ok());

        // empty dow
        let bad = NewScheduleRuleRequest {
            windows: vec![TimeWindow {
                dow: vec![],
                start: "09:00".into(),
                end: "12:00".into(),
            }],
            ..r.clone()
        };
        assert!(bad.validate().is_err());

        // dow out of range — the backend is the source of truth (spec 5.2), so an
        // out-of-range value arriving over IPC must be rejected here, not only by zod.
        let bad_dow = NewScheduleRuleRequest {
            windows: vec![TimeWindow {
                dow: vec![0, 7],
                start: "09:00".into(),
                end: "12:00".into(),
            }],
            ..r.clone()
        };
        assert!(bad_dow.validate().is_err());

        // zero-length
        let bad2 = NewScheduleRuleRequest {
            windows: vec![TimeWindow {
                dow: vec![1],
                start: "10:00".into(),
                end: "10:00".into(),
            }],
            ..r
        };
        assert!(bad2.validate().is_err());

        // ensure id generation works (sanity, not a validation case)
        let _ = Uuid::new_v4().to_string();
    }

    #[test]
    fn resolve_picks_priority_then_updated_then_id() {
        // Mon 10:00
        let now = at(2026, 9, 7, 10, 0);
        // Two rules covering the same window:
        let mut r_low = rule("aaaa", 5, wdow());
        r_low.updated_at = "2026-09-08T00:00:00+00:00".into();
        let mut r_high = rule("bbbb", 10, wdow());
        r_high.updated_at = "2026-09-08T00:00:00+00:00".into();
        // Higher priority wins
        let candidates = [r_low, r_high];
        let picked = resolve_active_rule(&candidates, now).unwrap();
        assert_eq!(picked.id, "bbbb");

        // Same priority, different updated_at — newer wins
        let mut r_old = rule("cccc", 5, wdow());
        r_old.updated_at = "2026-09-01T00:00:00+00:00".into();
        let mut r_new = rule("dddd", 5, wdow());
        r_new.updated_at = "2026-09-08T00:00:00+00:00".into();
        let candidates = [r_old, r_new];
        let picked = resolve_active_rule(&candidates, now).unwrap();
        assert_eq!(picked.id, "dddd");

        // Same priority + updated_at — id DESC wins
        let r1 = rule("aaaa", 5, wdow());
        let r2 = rule("bbbb", 5, wdow());
        let candidates = [r1, r2];
        let picked = resolve_active_rule(&candidates, now).unwrap();
        assert_eq!(picked.id, "bbbb");
    }

    #[test]
    fn resolve_returns_none_outside_windows() {
        let now = at(2026, 9, 7, 20, 0); // Mon 20:00, outside 09-18
        let r = rule("a", 0, wdow());
        assert!(resolve_active_rule(&[r], now).is_none());
    }

    #[test]
    fn disabled_rules_ignored() {
        let now = at(2026, 9, 7, 10, 0);
        let mut r = rule("a", 0, wdow());
        r.enabled = false;
        assert!(resolve_active_rule(&[r], now).is_none());
    }

    #[test]
    fn current_window_start_for_returns_start_of_active_window() {
        let now = at(2026, 9, 7, 10, 0); // Mon 10:00
        let r = rule("a", 0, wdow());
        let ws = current_window_start_for(&[r], now).unwrap();
        assert_eq!(ws, at(2026, 9, 7, 9, 0));
    }

    #[test]
    fn previous_window_end_before_picks_latest_past_end() {
        // Mon 20:00, window Mon-Fri 09:00-18:00 → the 18:00 boundary today.
        let now = at(2026, 9, 7, 20, 0);
        let r = rule("a", 0, wdow());
        assert_eq!(
            previous_window_end_before(&[r], now),
            Some(at(2026, 9, 7, 18, 0))
        );
    }

    #[test]
    fn previous_window_end_before_none_without_rules() {
        assert_eq!(previous_window_end_before(&[], at(2026, 9, 7, 20, 0)), None);
    }

    #[test]
    fn previous_window_end_before_ignores_disabled_and_future_ends() {
        // Mon 10:00 — today's 18:00 end is still in the future, so the answer must
        // come from the previous weekday (Fri 2026-09-04 18:00).
        let now = at(2026, 9, 7, 10, 0);
        let r = rule("a", 0, wdow());
        assert_eq!(
            previous_window_end_before(std::slice::from_ref(&r), now),
            Some(at(2026, 9, 4, 18, 0))
        );

        let mut disabled = r;
        disabled.enabled = false;
        assert_eq!(previous_window_end_before(&[disabled], now), None);
    }
}
