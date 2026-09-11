//! Schedule service — 60-second tick loop, evaluation engine, lib.rs wire-up
//!
//! Corresponds to spec section 5.4 (evaluation algorithm), 5.5 (manual-pin), 5.6 (tick loop),
//! and 6.2 (window-start ledger). Triggered by Tauri setup; exposes the current state
//! synchronously for tray integration.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::schedule_rules::{
    resolve_active_rule, rule_sort_key, AppEvalReport, EvaluationReport, NextSwitch, ScheduleRule,
    SwitchSource,
};
use chrono::{DateTime, Datelike, Local, NaiveTime};
use futures::future::BoxFuture;
use tokio::time::MissedTickBehavior;

/// Current scheduler state for an app — used by tray integration (T13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledState {
    /// A rule covers `now`; the scheduler would activate `provider` until `until` ("HH:MM" local).
    Active { provider: String, until: String },
    /// No rule covers `now` and no fallback is set.
    Idle,
    /// No rule covers `now`, but a fallback provider is configured.
    Fallback { provider: String },
}

/// Async closure that performs the actual provider switch.
/// Production: calls `ProviderService::switch`. Tests: records the call.
pub type SwitchFn = Arc<
    dyn Fn(AppType, String, SwitchSource) -> BoxFuture<'static, Result<(), AppError>> + Send + Sync,
>;

/// Wrapper for `app.manage(ScheduleServiceState(...))` — lets Tauri commands reach the
/// service instance (T6).
pub struct ScheduleServiceState(pub Arc<ScheduleService>);

/// Settings keys backing `get_schedule_health` (spec section 2, AC7).
const SETTING_LAST_TICK_AT: &str = "schedule_last_tick_at";
const SETTING_LAST_TICK_ERROR: &str = "schedule_last_tick_error";
const SETTING_CONSECUTIVE_FAILURES: &str = "schedule_consecutive_failures";

/// Cap on the persisted tick error: the text is rendered in the UI, so it must never
/// grow into an unbounded upstream payload.
const MAX_PERSISTED_ERROR_LEN: usize = 400;

/// Truncate on a char boundary (the text is user-visible, not a byte buffer).
fn truncate_error(text: &str) -> String {
    match text.char_indices().nth(MAX_PERSISTED_ERROR_LEN) {
        Some((idx, _)) => format!("{}…", &text[..idx]),
        None => text.to_string(),
    }
}

/// Parse an RFC3339 ledger timestamp into local time.
///
/// Comparing the raw strings would be wrong: across a DST offset change local-time
/// strings do not sort chronologically, which inverts the manual pin for about an
/// hour a year.
fn parse_local(ts: &str) -> Option<DateTime<Local>> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Local))
}

fn later_of(a: Option<DateTime<Local>>, b: Option<DateTime<Local>>) -> Option<DateTime<Local>> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

/// Credential values we actually hold, keyed by name via `is_sensitive_config_key` —
/// never a "looks like a key" guess (see the redaction rule in CLAUDE.md).
fn secret_values(value: &serde_json::Value) -> Vec<String> {
    use serde_json::Value;
    match value {
        Value::Object(map) => map
            .iter()
            .flat_map(|(k, v)| match v {
                Value::String(s) if !s.is_empty() => key_driven_secrets(k, s),
                other => secret_values(other),
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(secret_values).collect(),
        _ => Vec::new(),
    }
}

/// Secrets attributable to a string value **from its key name**: the whole value of a
/// credential key, plus the userinfo of a URL-valued key. Userinfo is a credential by
/// definition rather than by shape — `redact_url_for_log_with_secrets` in `lib.rs`
/// strips it structurally for the same reason.
fn key_driven_secrets(key: &str, value: &str) -> Vec<String> {
    if crate::services::ProviderService::is_sensitive_config_key(key) {
        return vec![value.to_string()];
    }
    if key.to_ascii_uppercase().contains("URL") {
        return url_userinfo_secrets(value);
    }
    Vec::new()
}

/// The credential half of a `scheme://user:secret@host` authority. Userinfo carrying no
/// password is the "token in the URL" form, where the username itself is the secret.
fn url_userinfo_secrets(raw: &str) -> Vec<String> {
    let Ok(url) = url::Url::parse(raw) else {
        return Vec::new();
    };
    if !url.has_host() {
        return Vec::new();
    }
    match (url.username(), url.password()) {
        (_, Some(password)) if !password.is_empty() => vec![password.to_string()],
        (user, _) if !user.is_empty() => vec![user.to_string()],
        _ => Vec::new(),
    }
}

/// 60-second tick loop + evaluation engine. Created once at startup, managed by Tauri.
pub struct ScheduleService {
    db: Arc<Database>,
    switch_fn: SwitchFn,
}

impl ScheduleService {
    /// Construct a new service. The caller is expected to keep the returned `Arc` alive
    /// for as long as ticks should run.
    pub fn new(db: Arc<Database>, switch_fn: SwitchFn) -> Arc<Self> {
        Arc::new(Self { db, switch_fn })
    }

    /// Spawn the 60-second tick loop and run one immediate evaluation so apps launched
    /// inside a window activate the right provider without waiting up to 60s.
    pub async fn start(self: Arc<Self>) -> Result<(), AppError> {
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.run_tick_loop().await });
        if let Err(e) = self.tick_once().await {
            log::warn!("[schedule] startup evaluation failed: {e}");
        }
        Ok(())
    }

    /// Evaluate every app (all 9 `AppType::all()` entries) and persist the tick health
    /// that `get_schedule_health` reads (spec section 2, AC7). This is the entry point
    /// for the tick loop, the startup evaluation and the UI's "Run now" retry.
    pub async fn tick_once(&self) -> Result<EvaluationReport, AppError> {
        let outcome = self.evaluate_now().await;
        self.record_tick_health(outcome.as_ref().err());
        outcome
    }

    /// Evaluate every app (all 9 `AppType::all()` entries). Every app is evaluated even
    /// if an earlier one failed, but the pass as a whole fails when any app failed:
    /// the health indicator exists to tell the user "scheduling is not working", and
    /// there is no per-app surface in the UI to report a partial failure on. The
    /// per-app detail goes into the error message instead.
    pub async fn evaluate_now(&self) -> Result<EvaluationReport, AppError> {
        let mut apps = Vec::new();
        let mut failures = Vec::new();
        for app in AppType::all() {
            match self.evaluate_for_app_inner(app.as_str()).await {
                Ok(r) => apps.push(r),
                Err(e) => {
                    // Redact here, while we still know whose credentials could appear
                    // in the text — the message is persisted and shown in the UI.
                    let msg = self.redact_for_app(app.as_str(), &e.to_string());
                    log::warn!("[schedule] eval {app:?} failed: {msg}");
                    failures.push(format!("{}: {msg}", app.as_str()));
                }
            }
        }
        if failures.is_empty() {
            Ok(EvaluationReport { apps })
        } else {
            let count = failures.len();
            let detail = failures.join("; ");
            Err(AppError::localized(
                "schedule.apps_failed",
                format!("{count} 个应用的定时切换失败：{detail}"),
                format!("{count} app(s) failed: {detail}"),
            ))
        }
    }

    fn record_tick_health(&self, error: Option<&AppError>) {
        let failures = match error {
            Some(_) => self.consecutive_failures().saturating_add(1),
            None => 0,
        };
        let message = error
            .map(|e| truncate_error(&e.to_string()))
            .unwrap_or_default();
        let now = Local::now().to_rfc3339();
        let failures_text = failures.to_string();
        if let Err(e) = self.db.set_schedule_tick_health([
            (SETTING_LAST_TICK_AT, now.as_str()),
            (SETTING_CONSECUTIVE_FAILURES, failures_text.as_str()),
            (SETTING_LAST_TICK_ERROR, message.as_str()),
        ]) {
            log::warn!("[schedule] persisting tick health failed: {e}");
        }
    }

    fn consecutive_failures(&self) -> u32 {
        self.db
            .get_setting(SETTING_CONSECUTIVE_FAILURES)
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    fn redact_for_app(&self, app: &str, text: &str) -> String {
        let secrets: Vec<String> = match self.db.get_all_providers(app) {
            Ok(providers) => providers
                .values()
                .flat_map(|p| secret_values(&p.settings_config))
                .collect(),
            Err(_) => Vec::new(),
        };
        crate::redact_known_secrets_strict(text, &secrets)
    }

    /// Public wrapper for single-app evaluation (used by T6 IPC commands and tests).
    pub async fn evaluate_for_app(&self, app: &str) -> Result<AppEvalReport, AppError> {
        self.evaluate_for_app_inner(app).await
    }

    /// Find the next future window start for `app` (searches up to 7 days ahead).
    /// Returns `None` when no rule has a future window in that horizon.
    ///
    /// A rule that is active *right now* is not the answer: its window already
    /// started, so the next switch is whatever comes after it. The currently
    /// active provider is reported separately by [`Self::current_state_for`].
    pub fn next_switch_for(&self, app: &str, now: DateTime<Local>) -> Option<NextSwitch> {
        let rules = self.db.list_schedule_rules(Some(app)).ok()?;
        for offset in 0..7 {
            let probe = now + chrono::Duration::days(offset);
            let mut best: Option<(DateTime<Local>, &ScheduleRule)> = None;
            for r in rules.iter().filter(|r| r.enabled) {
                for w in &r.windows {
                    if !w
                        .dow
                        .contains(&(probe.weekday().num_days_from_sunday() as u8))
                    {
                        continue;
                    }
                    let Ok(s) = NaiveTime::parse_from_str(&w.start, "%H:%M") else {
                        continue;
                    };
                    let Some(candidate) = probe
                        .date_naive()
                        .and_time(s)
                        .and_local_timezone(Local)
                        .single()
                    else {
                        continue;
                    };
                    if candidate <= now {
                        continue;
                    }
                    let wins = match &best {
                        None => true,
                        Some((at, chosen)) => {
                            candidate < *at
                                || (candidate == *at && rule_sort_key(r) < rule_sort_key(chosen))
                        }
                    };
                    if wins {
                        best = Some((candidate, r));
                    }
                }
            }
            // Every candidate found at a later offset falls on a later date.
            if let Some((at, r)) = best {
                return Some(NextSwitch {
                    app: app.into(),
                    provider_id: r.provider_id.clone(),
                    at: at.to_rfc3339(),
                    reason: "rule".into(),
                });
            }
        }
        None
    }

    /// Synchronous read of the current scheduler state for `app` at `now`.
    /// Used by tray menu builders that run outside an async context (T13).
    pub fn current_state_for(&self, app: AppType, now: DateTime<Local>) -> ScheduledState {
        let app_str = app.as_str();
        let rules = match self.db.list_schedule_rules(Some(app_str)) {
            Ok(r) => r,
            Err(_) => return ScheduledState::Idle,
        };
        if let Some(rule) = resolve_active_rule(&rules, now) {
            if let Some(w) = rule.windows.iter().find(|w| w.matches(now)) {
                return ScheduledState::Active {
                    provider: self.provider_label(app_str, &rule.provider_id),
                    until: w.end.clone(),
                };
            }
        }
        match self.db.get_fallback_provider(app_str) {
            Ok(Some(p)) => ScheduledState::Fallback {
                provider: self.provider_label(app_str, &p),
            },
            _ => ScheduledState::Idle,
        }
    }

    /// Human-readable provider name for tray labels, falling back to the id when the
    /// provider was deleted out from under a rule.
    fn provider_label(&self, app: &str, provider_id: &str) -> String {
        self.db
            .get_provider_name(provider_id, app)
            .ok()
            .flatten()
            .unwrap_or_else(|| provider_id.to_string())
    }

    /// Internal evaluation logic (spec 5.4 decision table). Updates the decision-epoch
    /// ledger before applying the manual-pin check, so window roll-over is handled.
    async fn evaluate_for_app_inner(&self, app: &str) -> Result<AppEvalReport, AppError> {
        let now = Local::now();
        let rules = self.db.list_schedule_rules(Some(app))?;
        let active = resolve_active_rule(&rules, now);

        let (target_provider, target_reason) = match active {
            Some(r) => (r.provider_id.clone(), "rule".to_string()),
            None => match self.db.get_fallback_provider(app)? {
                Some(p) => (p, "fallback".to_string()),
                None => {
                    return Ok(AppEvalReport {
                        app: app.into(),
                        fired: false,
                        reason: "none".into(),
                        from_provider: None,
                        to_provider: None,
                        skipped_due_to: None,
                    });
                }
            },
        };

        // When the scheduling decision now in force began. For a rule that is the
        // window start; for a fallback it is whichever is later of "the previous window
        // ended" and "the fallback was configured". The fallback arm is load-bearing:
        // without it a fallback-only setup has no epoch at all, so the manual pin can
        // never engage, and a rule+fallback setup keeps a stale window epoch, so the pin
        // never releases when the window ends (spec 5.4 / D2 / E8).
        let decision_start = match active {
            Some(_) => crate::schedule_rules::current_window_start_for(&rules, now),
            None => later_of(
                crate::schedule_rules::previous_window_end_before(&rules, now),
                self.db
                    .get_fallback_updated_at(app)?
                    .as_deref()
                    .and_then(parse_local),
            ),
        };
        if let Some(ds) = decision_start.as_ref() {
            self.db
                .update_app_schedule_state_window(app, Some(&ds.to_rfc3339()))?;
        }

        // Spec 5.4: a target that already equals the current provider is a no-op.
        // Without this every tick rewrote the live config, re-synced MCP and appended a
        // switch_log row — about 1440 rows per app per day.
        let current = self.db.get_current_provider(app)?;
        if current.as_deref() == Some(target_provider.as_str()) {
            return Ok(AppEvalReport {
                app: app.into(),
                fired: false,
                reason: target_reason,
                from_provider: current,
                to_provider: Some(target_provider),
                skipped_due_to: Some("same_provider".into()),
            });
        }

        // Additive-mode apps (OpenCode / OpenClaw / Hermes / Pi) never write
        // `current_provider`, so the check above can never fire for them and the newest
        // switch-log row is the only record of what they are already on. Requiring
        // `decision_start <= fired_at` is what lets a *new* window re-fire: its epoch
        // moves past the previous window's log row.
        let last_fired = if current.is_some() {
            None
        } else {
            self.db.last_switch_log_entry(app)?
        };
        if let Some((provider, fired_at)) = last_fired {
            let fired_local = parse_local(&fired_at);
            let unchanged_since_last_fire = matches!(
                (decision_start.as_ref(), fired_local.as_ref()),
                (Some(ds), Some(f)) if ds <= f
            );
            if provider == target_provider && unchanged_since_last_fire {
                return Ok(AppEvalReport {
                    app: app.into(),
                    fired: false,
                    reason: target_reason,
                    from_provider: current,
                    to_provider: Some(target_provider),
                    skipped_due_to: Some("same_provider".into()),
                });
            }
        }

        // Manual-pin check (spec 5.5 / D2): if the user manually switched at-or-after
        // the start of the current scheduling decision, the schedule yields to them.
        let state = self.db.get_app_schedule_state(app)?;
        let last_manual = state
            .as_ref()
            .and_then(|s| s.1.as_deref())
            .and_then(parse_local);
        // A decision epoch we cannot compute pins rather than stomps: a window whose
        // start is a DST-skipped local time has no instant at all, and losing the
        // user's manual choice is the worse of the two outcomes (spec 5.5 / D2).
        let pinned = match (decision_start.as_ref(), last_manual.as_ref()) {
            (Some(ds), Some(m)) => m >= ds,
            (None, Some(_)) => true,
            _ => false,
        };
        if pinned {
            return Ok(AppEvalReport {
                app: app.into(),
                fired: false,
                reason: target_reason,
                from_provider: current,
                to_provider: Some(target_provider),
                skipped_due_to: Some("manual_pin".into()),
            });
        }

        // Fire: call the switch closure, append a log entry, then enforce retention.
        let app_type = AppType::from_str(app)
            .map_err(|e| AppError::Message(format!("unknown app {app}: {e}")))?;
        (self.switch_fn)(app_type, target_provider.clone(), SwitchSource::Scheduled).await?;
        let now_iso = now.to_rfc3339();
        log::info!("[schedule] {app} switched to {target_provider} ({target_reason})");
        self.db
            .append_switch_log(app, &target_provider, &now_iso, &target_reason)?;
        // Spec 5.1's 1000-rows-per-app retention cap; only the fired path grows the table.
        if let Err(e) = self.db.prune_switch_log_for_app(app) {
            log::warn!("[schedule] pruning switch log failed: {e}");
        }
        Ok(AppEvalReport {
            app: app.into(),
            fired: true,
            reason: target_reason,
            from_provider: current,
            to_provider: Some(target_provider),
            skipped_due_to: None,
        })
    }

    /// The 60s tick loop. `MissedTickBehavior::Skip` means a system suspend/resume
    /// will not replay missed ticks (spec 6.2).
    async fn run_tick_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = self.tick_once().await {
                log::warn!("[schedule] tick evaluation failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule_rules::{NewScheduleRuleRequest, TimeWindow};
    use std::collections::HashSet;
    use std::sync::Mutex;

    type FiredLog = Arc<Mutex<Vec<(String, String)>>>;

    fn make_svc(db: Arc<Database>, fired: FiredLog) -> Arc<ScheduleService> {
        let switch_fn: SwitchFn = Arc::new(move |app, provider, _source| {
            let fired = fired.clone();
            Box::pin(async move {
                fired
                    .lock()
                    .unwrap()
                    .push((app.as_str().to_string(), provider));
                Ok(())
            })
        });
        ScheduleService::new(db, switch_fn)
    }

    fn make_svc_no_record(db: Arc<Database>) -> Arc<ScheduleService> {
        let switch_fn: SwitchFn = Arc::new(|_app, _provider, _source| Box::pin(async { Ok(()) }));
        ScheduleService::new(db, switch_fn)
    }

    fn make_svc_failing(db: Arc<Database>) -> Arc<ScheduleService> {
        let switch_fn: SwitchFn = Arc::new(|_app, _provider, _source| {
            Box::pin(async { Err(AppError::Message("live write refused".into())) })
        });
        ScheduleService::new(db, switch_fn)
    }

    fn make_svc_failing_with(db: Arc<Database>, message: &'static str) -> Arc<ScheduleService> {
        let switch_fn: SwitchFn = Arc::new(move |_app, _provider, _source| {
            Box::pin(async move { Err(AppError::Message(message.to_string())) })
        });
        ScheduleService::new(db, switch_fn)
    }

    fn rule_req(
        app: &str,
        provider_id: &str,
        dow: Vec<u8>,
        start: String,
        end: String,
    ) -> NewScheduleRuleRequest {
        NewScheduleRuleRequest {
            app: app.into(),
            provider_id: provider_id.into(),
            windows: vec![TimeWindow { dow, start, end }],
            priority: 0,
            enabled: true,
            note: None,
        }
    }

    fn switch_log_count(db: &Database, app: &str) -> i64 {
        use rusqlite::params;
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM schedule_switch_log WHERE app = ?1",
            params![app],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn stored_failures(db: &Database) -> u32 {
        db.get_setting("schedule_consecutive_failures")
            .unwrap()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// `set_fallback_provider` always stamps `updated_at = now`; tests that need the
    /// fallback to have been configured *before* a manual switch have to backdate it.
    fn backdate_fallback(db: &Database, app: &str, at: &str) {
        use rusqlite::params;
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "UPDATE app_fallback_providers SET updated_at = ?2 WHERE app = ?1",
            params![app, at],
        )
        .unwrap();
    }

    fn local_at(date: chrono::NaiveDate, hour: u32, minute: u32) -> DateTime<Local> {
        date.and_hms_opt(hour, minute, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .expect("unambiguous local time")
    }

    fn insert_provider(db: &Database, app: &str, id: &str) {
        insert_provider_with_config(db, app, id, "{}");
    }

    fn insert_provider_with_config(db: &Database, app: &str, id: &str, settings_config: &str) {
        use rusqlite::params;
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
             VALUES (?1, ?2, ?3, ?4, '{}', 0)",
            params![id, app, format!("{app}-{id}"), settings_config],
        )
        .unwrap();
    }

    fn all_dow() -> Vec<u8> {
        (0..7u8).collect()
    }

    /// A window that is active right now, for every local time. It must not be
    /// derived from `now ± 1h`: that pair inverts around midnight and
    /// `NewScheduleRuleRequest::validate` rejects `start >= end`.
    fn wide_window_around_now() -> (String, String) {
        ("00:00".to_string(), "23:59".to_string())
    }

    #[tokio::test]
    async fn evaluate_noop_when_no_rules() {
        let db = Arc::new(Database::memory().unwrap());
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        let report = svc.evaluate_now().await.unwrap();
        // Every app is evaluated, but no rule and no fallback means every report is a
        // no-op (fired=false, reason="none") and the switch closure is never called.
        let app_count = AppType::all().count();
        assert_eq!(report.apps.len(), app_count);
        for r in &report.apps {
            assert!(!r.fired);
            assert_eq!(r.reason, "none");
            assert!(r.to_provider.is_none());
        }
        assert!(fired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn tick_fires_at_window_start() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        let (start, end) = wide_window_around_now();
        let req = NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "p1".into(),
            windows: vec![TimeWindow {
                dow: all_dow(),
                start,
                end,
            }],
            priority: 0,
            enabled: true,
            note: None,
        };
        db.create_schedule_rule(&req).unwrap();

        let report = svc.evaluate_for_app("claude").await.unwrap();
        assert!(report.fired, "expected fire, got {report:?}");
        assert_eq!(report.to_provider.as_deref(), Some("p1"));
        assert_eq!(report.reason, "rule");

        let log = fired.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].0, "claude");
        assert_eq!(log[0].1, "p1");
    }

    #[test]
    fn current_state_for_active_rule() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        let svc = make_svc_no_record(db.clone());

        let (start, end) = wide_window_around_now();
        let req = NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "p1".into(),
            windows: vec![TimeWindow {
                dow: all_dow(),
                start,
                end: end.clone(),
            }],
            priority: 0,
            enabled: true,
            note: None,
        };
        db.create_schedule_rule(&req).unwrap();

        let state = svc.current_state_for(AppType::Claude, Local::now());
        match state {
            ScheduledState::Active { provider, until } => {
                assert_eq!(provider, "claude-p1");
                assert_eq!(until, end);
            }
            other => panic!("expected Active, got {other:?}"),
        }
    }

    /// A rule can outlive its provider (deleted via a path that leaves the rule in
    /// place). The tray must still render something, so the id is the fallback.
    #[test]
    fn current_state_for_falls_back_to_the_id_when_the_provider_is_gone() {
        let db = Arc::new(Database::memory().unwrap());
        let svc = make_svc_no_record(db.clone());

        let (start, end) = wide_window_around_now();
        db.create_schedule_rule(&NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "ghost".into(),
            windows: vec![TimeWindow {
                dow: all_dow(),
                start,
                end,
            }],
            priority: 0,
            enabled: true,
            note: None,
        })
        .unwrap();

        match svc.current_state_for(AppType::Claude, Local::now()) {
            ScheduledState::Active { provider, .. } => assert_eq!(provider, "ghost"),
            other => panic!("expected Active, got {other:?}"),
        }
    }

    #[test]
    fn current_state_for_idle_when_no_rule_matches() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        let svc = make_svc_no_record(db.clone());

        // Rule covers a window that is definitely not "now" — pick 00:00-00:30 today,
        // and on the rare chance the test runs in that 30-minute slot, also exclude
        // the current day-of-week from the window.
        let mut dow = HashSet::new();
        let today = Local::now().weekday().num_days_from_sunday() as u8;
        for d in 0..7u8 {
            if d != today {
                dow.insert(d);
            }
        }
        let dow_vec: Vec<u8> = dow.into_iter().collect();
        let req = NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "p1".into(),
            windows: vec![TimeWindow {
                dow: dow_vec,
                start: "00:00".into(),
                end: "00:30".into(),
            }],
            priority: 0,
            enabled: true,
            note: None,
        };
        db.create_schedule_rule(&req).unwrap();

        let state = svc.current_state_for(AppType::Claude, Local::now());
        assert_eq!(state, ScheduledState::Idle);
    }

    #[test]
    fn current_state_for_fallback_when_no_rule_and_fallback_set() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p2");
        let svc = make_svc_no_record(db.clone());

        // No rules, only a fallback.
        db.set_fallback_provider("claude", Some("p2")).unwrap();

        let state = svc.current_state_for(AppType::Claude, Local::now());
        assert_eq!(
            state,
            ScheduledState::Fallback {
                provider: "claude-p2".into()
            }
        );
    }

    // --- Manual-pin ledger (spec D2 / E7 / E8 / E9) ---

    #[tokio::test]
    async fn manual_switch_inside_window_survives_next_tick() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        insert_provider(&db, "claude", "p2");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        let (start, end) = wide_window_around_now();
        db.create_schedule_rule(&rule_req("claude", "p1", all_dow(), start, end))
            .unwrap();

        // First tick activates the rule's provider.
        assert!(svc.evaluate_for_app("claude").await.unwrap().fired);

        // The user manually picks p2 inside the window — this is what
        // ProviderService::switch commits for SwitchSource::Manual.
        db.set_current_provider("claude", "p2").unwrap();
        db.update_app_schedule_state_manual("claude", &Local::now().to_rfc3339())
            .unwrap();

        let report = svc.evaluate_for_app("claude").await.unwrap();
        assert!(
            !report.fired,
            "manual choice must survive the next tick: {report:?}"
        );
        assert_eq!(report.skipped_due_to.as_deref(), Some("manual_pin"));
        assert_eq!(fired.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn manual_switch_survives_in_fallback_only_setup() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        insert_provider(&db, "claude", "p2");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        // No rules at all — the fallback is the whole schedule.
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        db.set_current_provider("claude", "p2").unwrap();
        db.update_app_schedule_state_manual("claude", &Local::now().to_rfc3339())
            .unwrap();

        let report = svc.evaluate_for_app("claude").await.unwrap();
        assert!(
            !report.fired,
            "the fallback must not stomp a later manual switch: {report:?}"
        );
        assert_eq!(report.skipped_due_to.as_deref(), Some("manual_pin"));
        assert!(fired.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pin_releases_when_window_ends_and_fallback_applies() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        insert_provider(&db, "claude", "p2");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        // A 09:00-18:00 rule on *yesterday's* weekday: definitely over, and never
        // active "now", whatever time the suite runs at.
        let now = Local::now();
        let yesterday = now - chrono::Duration::days(1);
        let ydow = yesterday.weekday().num_days_from_sunday() as u8;
        db.create_schedule_rule(&rule_req(
            "claude",
            "p1",
            vec![ydow],
            "09:00".into(),
            "18:00".into(),
        ))
        .unwrap();
        db.set_fallback_provider("claude", Some("p2")).unwrap();
        backdate_fallback(
            &db,
            "claude",
            &(now - chrono::Duration::days(7)).to_rfc3339(),
        );

        // The state an in-window tick would have left behind, plus a manual switch
        // made inside that window.
        let ydate = yesterday.date_naive();
        db.update_app_schedule_state_window("claude", Some(&local_at(ydate, 9, 0).to_rfc3339()))
            .unwrap();
        db.update_app_schedule_state_manual("claude", &local_at(ydate, 10, 0).to_rfc3339())
            .unwrap();

        let report = svc.evaluate_for_app("claude").await.unwrap();
        assert!(
            report.fired,
            "the pin must release once the window ended: {report:?}"
        );
        assert_eq!(report.reason, "fallback");
        assert_eq!(report.to_provider.as_deref(), Some("p2"));
        assert_eq!(fired.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn same_provider_reports_skipped_and_writes_no_log() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        let (start, end) = wide_window_around_now();
        db.create_schedule_rule(&rule_req("claude", "p1", all_dow(), start, end))
            .unwrap();
        db.set_current_provider("claude", "p1").unwrap();

        let report = svc.evaluate_for_app("claude").await.unwrap();
        assert!(
            !report.fired,
            "a same-provider target is a no-op: {report:?}"
        );
        assert_eq!(report.skipped_due_to.as_deref(), Some("same_provider"));
        assert!(fired.lock().unwrap().is_empty());
        assert_eq!(switch_log_count(&db, "claude"), 0);
    }

    #[tokio::test]
    async fn additive_mode_app_does_not_refire_on_the_next_tick() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "opencode", "p1");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        let (start, end) = wide_window_around_now();
        db.create_schedule_rule(&rule_req("opencode", "p1", all_dow(), start, end))
            .unwrap();

        assert!(svc.evaluate_for_app("opencode").await.unwrap().fired);
        // OpenCode is additive-mode: a real switch leaves `is_current` unset, so the
        // switch-log row is the only trace of the fire. The stub closure matches that.
        assert_eq!(db.get_current_provider("opencode").unwrap(), None);
        assert_eq!(switch_log_count(&db, "opencode"), 1);

        let report = svc.evaluate_for_app("opencode").await.unwrap();
        assert!(
            !report.fired,
            "an unchanged additive-mode app must not re-fire: {report:?}"
        );
        assert_eq!(report.skipped_due_to.as_deref(), Some("same_provider"));
        assert_eq!(fired.lock().unwrap().len(), 1);
        assert_eq!(switch_log_count(&db, "opencode"), 1);
    }

    #[tokio::test]
    async fn failing_tick_increments_failures_and_success_resets_them() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        let (start, end) = wide_window_around_now();
        db.create_schedule_rule(&rule_req("claude", "p1", all_dow(), start, end))
            .unwrap();

        let failing = make_svc_failing(db.clone());
        assert!(failing.tick_once().await.is_err());
        assert_eq!(stored_failures(&db), 1);
        assert!(db
            .get_setting("schedule_last_tick_error")
            .unwrap()
            .is_some_and(|s| !s.is_empty()));
        assert!(db.get_setting("schedule_last_tick_at").unwrap().is_some());

        assert!(failing.tick_once().await.is_err());
        assert_eq!(stored_failures(&db), 2);

        let healthy = make_svc_no_record(db.clone());
        assert!(healthy.tick_once().await.is_ok());
        assert_eq!(stored_failures(&db), 0);
        assert_eq!(
            db.get_setting("schedule_last_tick_error")
                .unwrap()
                .as_deref(),
            Some("")
        );
    }

    // --- Redaction of the persisted tick error (spec 9 / security) ---

    #[tokio::test]
    async fn persisted_tick_error_redacts_provider_secrets() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider_with_config(
            &db,
            "claude",
            "p1",
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-test-abc123",
                      "ANTHROPIC_BASE_URL":"https://relay:hunter2@example.com/v1"}}"#,
        );
        let (start, end) = wide_window_around_now();
        db.create_schedule_rule(&rule_req("claude", "p1", all_dow(), start, end))
            .unwrap();

        // An upstream failure that quotes both the token and the whole base_url.
        let svc = make_svc_failing_with(
            db.clone(),
            "rejected sk-test-abc123 at https://relay:hunter2@example.com/v1",
        );
        assert!(svc.tick_once().await.is_err());

        let stored = db
            .get_setting("schedule_last_tick_error")
            .unwrap()
            .expect("a failed tick must persist its error");
        assert!(
            !stored.contains("sk-test-abc123"),
            "the credential must not survive into persisted text: {stored}"
        );
        assert!(
            !stored.contains("hunter2"),
            "base_url userinfo must not survive either: {stored}"
        );
        assert!(
            stored.contains("[REDACTED]"),
            "the redaction marker is missing: {stored}"
        );
    }

    #[test]
    fn truncate_error_caps_the_persisted_text_on_a_char_boundary() {
        let long = "密".repeat(MAX_PERSISTED_ERROR_LEN + 50);
        let truncated = truncate_error(&long);
        assert_eq!(truncated.chars().count(), MAX_PERSISTED_ERROR_LEN + 1);
        assert!(truncated.ends_with('…'));
        assert_eq!(truncate_error("short"), "short");
    }

    // --- Pin release (spec E9): re-saving the fallback outranks an older manual switch ---

    #[tokio::test]
    async fn re_saving_the_fallback_releases_the_manual_pin() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        insert_provider(&db, "claude", "p2");
        let fired: FiredLog = Arc::new(Mutex::new(Vec::new()));
        let svc = make_svc(db.clone(), fired.clone());

        // Fallback-only setup configured a week ago, then a manual switch yesterday.
        let now = Local::now();
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        backdate_fallback(
            &db,
            "claude",
            &(now - chrono::Duration::days(7)).to_rfc3339(),
        );
        db.set_current_provider("claude", "p2").unwrap();
        db.update_app_schedule_state_manual(
            "claude",
            &(now - chrono::Duration::days(1)).to_rfc3339(),
        )
        .unwrap();

        let pinned = svc.evaluate_for_app("claude").await.unwrap();
        assert!(
            !pinned.fired,
            "the manual switch must pin first: {pinned:?}"
        );
        assert_eq!(pinned.skipped_due_to.as_deref(), Some("manual_pin"));

        // Re-saving the same fallback stamps updated_at = now, which is later than the
        // manual switch — the user's escape hatch out of their own pin.
        db.set_fallback_provider("claude", Some("p1")).unwrap();

        let report = svc.evaluate_for_app("claude").await.unwrap();
        assert!(
            report.fired,
            "re-saving the fallback must release the pin: {report:?}"
        );
        assert_eq!(report.reason, "fallback");
        assert_eq!(report.to_provider.as_deref(), Some("p1"));
        assert_eq!(fired.lock().unwrap().len(), 1);
    }

    /// 2026-09-09 is a Wednesday, so both a weekday rule and an every-day rule apply.
    fn wednesday() -> chrono::NaiveDate {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        assert_eq!(d.weekday(), chrono::Weekday::Wed);
        d
    }

    fn multi_window_rule(
        app: &str,
        provider_id: &str,
        priority: i32,
        windows: Vec<TimeWindow>,
    ) -> NewScheduleRuleRequest {
        NewScheduleRuleRequest {
            app: app.into(),
            provider_id: provider_id.into(),
            windows,
            priority,
            enabled: true,
            note: None,
        }
    }

    fn window(dow: Vec<u8>, start: &str, end: &str) -> TimeWindow {
        TimeWindow {
            dow,
            start: start.into(),
            end: end.into(),
        }
    }

    #[test]
    fn next_switch_skips_the_rule_that_is_active_right_now() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "acct-team");
        insert_provider(&db, "claude", "hs-plan");
        let weekdays = vec![1, 2, 3, 4, 5];
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "acct-team",
            0,
            vec![
                window(weekdays.clone(), "18:00", "23:59"),
                window(weekdays, "00:00", "09:00"),
            ],
        ))
        .unwrap();
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "hs-plan",
            0,
            vec![window(all_dow(), "09:00", "18:00")],
        ))
        .unwrap();
        let svc = make_svc_no_record(db);

        // 11:30 sits inside hs-plan's window; the next switch is acct-team at 18:00,
        // not "hs-plan, 0 minutes from now".
        let next = svc
            .next_switch_for("claude", local_at(wednesday(), 11, 30))
            .expect("a switch later today");
        assert_eq!(next.provider_id, "acct-team");
        assert_eq!(next.at, local_at(wednesday(), 18, 0).to_rfc3339());
    }

    #[test]
    fn next_switch_picks_the_earliest_window_not_the_first_rule() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "late");
        insert_provider(&db, "claude", "early");
        // Higher priority sorts first in `list_schedule_rules`, so a first-match scan
        // would wrongly answer "late" here.
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "late",
            10,
            vec![window(all_dow(), "20:00", "23:00")],
        ))
        .unwrap();
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "early",
            0,
            vec![window(all_dow(), "09:00", "18:00")],
        ))
        .unwrap();
        let svc = make_svc_no_record(db);

        let next = svc
            .next_switch_for("claude", local_at(wednesday(), 8, 0))
            .expect("a switch later today");
        assert_eq!(next.provider_id, "early");
        assert_eq!(next.at, local_at(wednesday(), 9, 0).to_rfc3339());
    }

    #[test]
    fn next_switch_breaks_start_time_ties_by_priority() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "winner");
        insert_provider(&db, "claude", "loser");
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "loser",
            0,
            vec![window(all_dow(), "09:00", "18:00")],
        ))
        .unwrap();
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "winner",
            5,
            vec![window(all_dow(), "09:00", "12:00")],
        ))
        .unwrap();
        let svc = make_svc_no_record(db);

        let next = svc
            .next_switch_for("claude", local_at(wednesday(), 8, 0))
            .expect("a switch later today");
        assert_eq!(next.provider_id, "winner");
    }

    #[test]
    fn next_switch_rolls_over_to_the_following_day() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        db.create_schedule_rule(&multi_window_rule(
            "claude",
            "p1",
            0,
            vec![window(all_dow(), "09:00", "18:00")],
        ))
        .unwrap();
        let svc = make_svc_no_record(db);

        let next = svc
            .next_switch_for("claude", local_at(wednesday(), 23, 0))
            .expect("a switch tomorrow");
        assert_eq!(
            next.at,
            local_at(wednesday().succ_opt().unwrap(), 9, 0).to_rfc3339()
        );
    }

    #[test]
    fn next_switch_ignores_disabled_rules() {
        let db = Arc::new(Database::memory().unwrap());
        insert_provider(&db, "claude", "p1");
        let mut req =
            multi_window_rule("claude", "p1", 0, vec![window(all_dow(), "09:00", "18:00")]);
        req.enabled = false;
        db.create_schedule_rule(&req).unwrap();
        let svc = make_svc_no_record(db);

        assert!(svc
            .next_switch_for("claude", local_at(wednesday(), 8, 0))
            .is_none());
    }
}
