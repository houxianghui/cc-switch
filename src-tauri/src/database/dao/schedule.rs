//! 定时切换 DAO
//!
//! 包装 schedule_rules / app_fallback_providers / app_schedule_state / schedule_switch_log 四张表。
//! 所有方法挂在 `impl Database` 上，与项目其它 DAO 文件保持一致。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::schedule_rules::{
    NewScheduleRuleRequest, ScheduleRule, ScheduleRulePatch, SwitchLogEntry, TimeWindow,
};
use rusqlite::{params, ToSql};

/// The three ledger timestamps of an `app_schedule_state` row, in column order:
/// `(last_window_start_at, last_manual_switch_at, last_scheduled_switch_at)`.
pub type AppScheduleStateRow = (Option<String>, Option<String>, Option<String>);

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}

fn row_to_rule(row: &rusqlite::Row) -> Result<ScheduleRule, rusqlite::Error> {
    let windows_json: String = row.get("windows_json")?;
    let windows: Vec<TimeWindow> = serde_json::from_str(&windows_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(ScheduleRule {
        id: row.get("id")?,
        app: row.get("app")?,
        provider_id: row.get("provider_id")?,
        windows,
        priority: row.get("priority")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        note: row.get("note")?,
    })
}

impl Database {
    pub fn create_schedule_rule(
        &self,
        req: &NewScheduleRuleRequest,
    ) -> Result<ScheduleRule, AppError> {
        req.validate().map_err(AppError::Message)?;
        let conn = lock_conn!(self.conn);
        let now = now_iso();
        let id = crate::schedule_rules::new_rule_id();
        let windows_json =
            serde_json::to_string(&req.windows).map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO schedule_rules
                (id, app, provider_id, windows_json, priority, enabled, created_at, updated_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
            params![
                id,
                req.app,
                req.provider_id,
                windows_json,
                req.priority,
                req.enabled as i64,
                now,
                req.note,
            ],
        )
        .map_err(|e| AppError::Database(format!("insert rule: {e}")))?;
        Ok(ScheduleRule {
            id,
            app: req.app.clone(),
            provider_id: req.provider_id.clone(),
            windows: req.windows.clone(),
            priority: req.priority,
            enabled: req.enabled,
            created_at: now.clone(),
            updated_at: now,
            note: req.note.clone(),
        })
    }

    pub fn get_schedule_rule(&self, id: &str) -> Result<Option<ScheduleRule>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT * FROM schedule_rules WHERE id = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        match rows.next() {
            Ok(Some(row)) => Ok(Some(
                row_to_rule(row).map_err(|e| AppError::Database(e.to_string()))?,
            )),
            Ok(None) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn list_schedule_rules(&self, app: Option<&str>) -> Result<Vec<ScheduleRule>, AppError> {
        let conn = lock_conn!(self.conn);
        let (sql, want_filter): (&str, bool) = match app {
            Some(_) => (
                "SELECT * FROM schedule_rules WHERE app = ?1 \
                 ORDER BY priority DESC, updated_at DESC, id DESC",
                true,
            ),
            None => (
                "SELECT * FROM schedule_rules \
                 ORDER BY app, priority DESC, updated_at DESC, id DESC",
                false,
            ),
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mapped: Result<Vec<ScheduleRule>, rusqlite::Error> = if want_filter {
            stmt.query_map(params![app.unwrap()], row_to_rule)?
                .collect::<Result<Vec<_>, _>>()
        } else {
            stmt.query_map([], row_to_rule)?
                .collect::<Result<Vec<_>, _>>()
        };
        mapped.map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn update_schedule_rule(
        &self,
        id: &str,
        patch: &ScheduleRulePatch,
    ) -> Result<ScheduleRule, AppError> {
        patch.validate().map_err(AppError::Message)?;
        // Whitelist of mutable columns; every column here is opted in by a non-None patch field.
        let mut sets: Vec<String> = Vec::new();
        let mut bind: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(ref p) = patch.provider_id {
            sets.push("provider_id = ?".into());
            bind.push(Box::new(p.clone()));
        }
        if let Some(ref ws) = patch.windows {
            let s = serde_json::to_string(ws).map_err(|e| AppError::Database(e.to_string()))?;
            sets.push("windows_json = ?".into());
            bind.push(Box::new(s));
        }
        if let Some(p) = patch.priority {
            sets.push("priority = ?".into());
            bind.push(Box::new(p));
        }
        if let Some(e) = patch.enabled {
            sets.push("enabled = ?".into());
            bind.push(Box::new(e as i64));
        }
        if let Some(ref n) = patch.note {
            sets.push("note = ?".into());
            bind.push(Box::new(n.clone()));
        }
        sets.push("updated_at = ?".into());
        bind.push(Box::new(now_iso()));
        bind.push(Box::new(id.to_string()));
        let sql = format!("UPDATE schedule_rules SET {} WHERE id = ?", sets.join(", "));

        let conn = lock_conn!(self.conn);
        let bind_refs: Vec<&dyn ToSql> = bind.iter().map(|b| b.as_ref()).collect();
        conn.execute(&sql, bind_refs.as_slice())
            .map_err(|e| AppError::Database(format!("update rule: {e}")))?;
        drop(conn);

        self.get_schedule_rule(id)?
            .ok_or_else(|| AppError::Message("rule not found".into()))
    }

    pub fn delete_schedule_rule(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM schedule_rules WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(format!("delete rule: {e}")))?;
        Ok(())
    }

    pub fn get_fallback_provider(&self, app: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let res: Result<Option<String>, rusqlite::Error> = conn.query_row(
            "SELECT fallback_provider_id FROM app_fallback_providers WHERE app = ?1",
            params![app],
            |row| row.get::<_, Option<String>>(0),
        );
        match res {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(format!("get fallback: {e}"))),
        }
    }

    /// `updated_at` of the app's fallback row, only when a fallback is actually set.
    ///
    /// The scheduler uses it as the "when did the fallback decision begin" baseline
    /// for the manual-pin ledger in a rules-free configuration.
    pub fn get_fallback_updated_at(&self, app: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let res: Result<String, rusqlite::Error> = conn.query_row(
            "SELECT updated_at FROM app_fallback_providers
             WHERE app = ?1 AND fallback_provider_id IS NOT NULL",
            params![app],
            |row| row.get::<_, String>(0),
        );
        match res {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(format!("get fallback updated_at: {e}"))),
        }
    }

    pub fn set_fallback_provider(
        &self,
        app: &str,
        provider_id: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO app_fallback_providers (app, fallback_provider_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(app) DO UPDATE SET
                fallback_provider_id = excluded.fallback_provider_id,
                updated_at = excluded.updated_at",
            params![app, provider_id, now_iso()],
        )
        .map_err(|e| AppError::Database(format!("set fallback: {e}")))?;
        Ok(())
    }

    pub fn get_app_schedule_state(
        &self,
        app: &str,
    ) -> Result<Option<AppScheduleStateRow>, AppError> {
        let conn = lock_conn!(self.conn);
        let res: Result<AppScheduleStateRow, rusqlite::Error> = conn.query_row(
            "SELECT last_window_start_at, last_manual_switch_at, last_scheduled_switch_at
             FROM app_schedule_state WHERE app = ?1",
            params![app],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        );
        match res {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(format!("get state: {e}"))),
        }
    }

    pub fn update_app_schedule_state_window(
        &self,
        app: &str,
        window_start_at: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO app_schedule_state (app, last_window_start_at, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(app) DO UPDATE SET
                last_window_start_at = excluded.last_window_start_at,
                updated_at = excluded.updated_at",
            params![app, window_start_at, now_iso()],
        )
        .map_err(|e| AppError::Database(format!("update state window: {e}")))?;
        Ok(())
    }

    pub fn update_app_schedule_state_manual(&self, app: &str, at: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO app_schedule_state (app, last_manual_switch_at, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(app) DO UPDATE SET
                last_manual_switch_at = excluded.last_manual_switch_at,
                updated_at = excluded.updated_at",
            params![app, at, now_iso()],
        )
        .map_err(|e| AppError::Database(format!("update state manual: {e}")))?;
        Ok(())
    }

    pub fn update_app_schedule_state_scheduled(&self, app: &str, at: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO app_schedule_state (app, last_scheduled_switch_at, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(app) DO UPDATE SET
                last_scheduled_switch_at = excluded.last_scheduled_switch_at,
                updated_at = excluded.updated_at",
            params![app, at, now_iso()],
        )
        .map_err(|e| AppError::Database(format!("update state scheduled: {e}")))?;
        Ok(())
    }

    pub fn append_switch_log(
        &self,
        app: &str,
        provider_id: &str,
        fired_at: &str,
        reason: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO schedule_switch_log (app, provider_id, fired_at, reason) VALUES (?1, ?2, ?3, ?4)",
            params![app, provider_id, fired_at, reason],
        )
        .map_err(|e| AppError::Database(format!("insert log: {e}")))?;
        Ok(())
    }

    /// Write the three tick-health settings rows in a **single** statement.
    ///
    /// Three separate `set_setting` calls each take and release the connection
    /// mutex, so `get_schedule_health` could read the new failure count next to
    /// the previous tick's error text (spec AC7). One INSERT is one transaction.
    pub fn set_schedule_tick_health(&self, writes: [(&str, &str); 3]) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2), (?3, ?4), (?5, ?6)",
            params![
                writes[0].0,
                writes[0].1,
                writes[1].0,
                writes[1].1,
                writes[2].0,
                writes[2].1,
            ],
        )
        .map_err(|e| AppError::Database(format!("set tick health: {e}")))?;
        Ok(())
    }

    /// The app's newest switch-log row, as `(provider_id, fired_at)`.
    ///
    /// The scheduler needs it because additive-mode apps have no `is_current`
    /// column to compare a target against.
    pub fn last_switch_log_entry(&self, app: &str) -> Result<Option<(String, String)>, AppError> {
        let conn = lock_conn!(self.conn);
        let res: Result<(String, String), rusqlite::Error> = conn.query_row(
            // `fired_at` is RFC3339 carrying a *local* UTC offset, so this ordering is
            // lexical rather than chronological: it is approximate across an offset
            // change (a DST transition, or the user travelling timezones).
            "SELECT provider_id, fired_at FROM schedule_switch_log
             WHERE app = ?1 ORDER BY fired_at DESC LIMIT 1",
            params![app],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );
        match res {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(format!("get last switch log: {e}"))),
        }
    }

    /// Trim `schedule_switch_log` to the newest 1000 rows of a single app.
    ///
    /// The subquery must stay **uncorrelated**: the previous global form
    /// (`id NOT IN (SELECT ... WHERE sl.app = schedule_switch_log.app ...)`)
    /// re-ran per outer row and took ~3 s on a 9 000-row table while holding the
    /// connection mutex, which blocks every other query in the app. This shape
    /// lists the over-cap ids for one app through
    /// `idx_schedule_switch_log_app_fired` and deletes them by rowid.
    pub fn prune_switch_log_for_app(&self, app: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            // `fired_at` is RFC3339 carrying a *local* UTC offset, so "newest 1000" is a
            // lexical rather than chronological cut: it is approximate across an offset
            // change (a DST transition, or the user travelling timezones).
            "DELETE FROM schedule_switch_log WHERE id IN (
                SELECT id FROM schedule_switch_log
                WHERE app = ?1
                ORDER BY fired_at DESC
                LIMIT -1 OFFSET 1000
             )",
            params![app],
        )
        .map_err(|e| AppError::Database(format!("prune log: {e}")))?;
        Ok(())
    }

    /// The newest switch-log rows, joined to the provider name for display.
    ///
    /// `LEFT JOIN` rather than an inner one: `schedule_switch_log` deliberately has
    /// no foreign key to `providers`, so a row survives the provider being deleted
    /// and the history stays honest about what actually fired. `provider_name` is
    /// `None` in that case.
    pub fn list_switch_log(
        &self,
        app: Option<&str>,
        limit: u32,
    ) -> Result<Vec<SwitchLogEntry>, AppError> {
        let conn = lock_conn!(self.conn);
        // `fired_at` is RFC3339 carrying a *local* UTC offset, so this ordering is
        // lexical rather than chronological: it is approximate across an offset
        // change (a DST transition, or the user travelling timezones).
        let sql = "SELECT l.app, l.provider_id, p.name, l.fired_at, l.reason
                   FROM schedule_switch_log l
                   LEFT JOIN providers p ON p.id = l.provider_id
                   WHERE (?1 IS NULL OR l.app = ?1)
                   ORDER BY l.fired_at DESC
                   LIMIT ?2";
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::Database(format!("prepare switch log: {e}")))?;
        let rows = stmt
            .query_map(params![app, limit], |row| {
                Ok(SwitchLogEntry {
                    app: row.get(0)?,
                    provider_id: row.get(1)?,
                    provider_name: row.get(2)?,
                    fired_at: row.get(3)?,
                    reason: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(format!("query switch log: {e}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("read switch log: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_windows() -> Vec<TimeWindow> {
        vec![TimeWindow {
            dow: vec![1, 2, 3, 4, 5],
            start: "09:00".into(),
            end: "18:00".into(),
        }]
    }

    fn insert_provider(db: &Database, app: &str, id: &str) {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta, is_current)
             VALUES (?1, ?2, ?3, '{}', '{}', 0)",
            params![id, app, format!("{app}-{id}")],
        )
        .unwrap();
    }

    fn db() -> Database {
        Database::memory().unwrap()
    }

    #[test]
    fn create_and_get_rule() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        let req = NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "p1".into(),
            windows: sample_windows(),
            priority: 5,
            enabled: true,
            note: Some("workday".into()),
        };
        let rule = db.create_schedule_rule(&req).unwrap();
        assert_eq!(rule.app, "claude");
        assert_eq!(rule.priority, 5);
        let got = db.get_schedule_rule(&rule.id).unwrap().unwrap();
        assert_eq!(got.id, rule.id);
    }

    #[test]
    fn update_rule_patches_fields() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        let rule = db
            .create_schedule_rule(&NewScheduleRuleRequest {
                app: "claude".into(),
                provider_id: "p1".into(),
                windows: sample_windows(),
                priority: 0,
                enabled: true,
                note: None,
            })
            .unwrap();
        let patched = db
            .update_schedule_rule(
                &rule.id,
                &ScheduleRulePatch {
                    priority: Some(10),
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(patched.priority, 10);
        assert!(!patched.enabled);
    }

    #[test]
    fn update_rule_rejects_invalid_windows() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        let rule = db
            .create_schedule_rule(&NewScheduleRuleRequest {
                app: "claude".into(),
                provider_id: "p1".into(),
                windows: sample_windows(),
                priority: 0,
                enabled: true,
                note: None,
            })
            .unwrap();

        // dow 7 can never match a real weekday, and an inverted window never fires:
        // both must be rejected on the update path too, not only on create.
        for bad in [
            TimeWindow {
                dow: vec![7],
                start: "09:00".into(),
                end: "18:00".into(),
            },
            TimeWindow {
                dow: vec![1],
                start: "18:00".into(),
                end: "09:00".into(),
            },
        ] {
            assert!(db
                .update_schedule_rule(
                    &rule.id,
                    &ScheduleRulePatch {
                        windows: Some(vec![bad]),
                        ..Default::default()
                    },
                )
                .is_err());
        }
        assert!(db
            .update_schedule_rule(
                &rule.id,
                &ScheduleRulePatch {
                    priority: Some(1001),
                    ..Default::default()
                },
            )
            .is_err());

        // The rejected patches must not have been persisted.
        let stored = db.get_schedule_rule(&rule.id).unwrap().unwrap();
        assert_eq!(stored.windows, sample_windows());
        assert_eq!(stored.priority, 0);
    }

    #[test]
    fn delete_provider_cascading_rules_removes_rules_and_provider() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        insert_provider(&db, "claude", "p2");
        let _r1 = db
            .create_schedule_rule(&NewScheduleRuleRequest {
                app: "claude".into(),
                provider_id: "p1".into(),
                windows: sample_windows(),
                priority: 0,
                enabled: true,
                note: None,
            })
            .unwrap();
        let _r2 = db
            .create_schedule_rule(&NewScheduleRuleRequest {
                app: "claude".into(),
                provider_id: "p1".into(),
                windows: sample_windows(),
                priority: 1,
                enabled: true,
                note: None,
            })
            .unwrap();
        let n = db.delete_provider_cascading_rules("claude", "p1").unwrap();
        assert_eq!(n, 2);
        let list = db.list_schedule_rules(Some("claude")).unwrap();
        assert!(list.is_empty());
        assert!(db.get_provider_by_id("p1", "claude").unwrap().is_none());
        assert!(db.get_provider_by_id("p2", "claude").unwrap().is_some());
    }

    #[test]
    fn delete_provider_cascading_rules_with_no_rules_still_deletes_provider() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        let n = db.delete_provider_cascading_rules("claude", "p1").unwrap();
        assert_eq!(n, 0);
        assert!(db.get_provider_by_id("p1", "claude").unwrap().is_none());
    }

    #[test]
    fn cascade_delete_of_a_universal_child_removes_its_rules() {
        let db = db();
        insert_provider(&db, "claude", "universal-claude-u1");
        db.create_schedule_rule(&NewScheduleRuleRequest {
            app: "claude".into(),
            provider_id: "universal-claude-u1".into(),
            windows: sample_windows(),
            priority: 0,
            enabled: true,
            note: None,
        })
        .unwrap();

        let cascaded = db
            .delete_provider_cascading_rules("claude", "universal-claude-u1")
            .unwrap();

        assert_eq!(cascaded, 1);
        assert!(db.list_schedule_rules(Some("claude")).unwrap().is_empty());
        assert!(db
            .get_provider_by_id("universal-claude-u1", "claude")
            .unwrap()
            .is_none());
    }

    #[test]
    fn rename_remaps_schedule_rules_onto_the_new_id() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        for priority in [0, 1] {
            db.create_schedule_rule(&NewScheduleRuleRequest {
                app: "claude".into(),
                provider_id: "p1".into(),
                windows: sample_windows(),
                priority,
                enabled: true,
                note: None,
            })
            .unwrap();
        }
        // ProviderService::update saves the new row before retiring the old one.
        insert_provider(&db, "claude", "p1-new");

        db.delete_provider_remapping_schedule_refs("claude", "p1", "p1-new")
            .unwrap();

        let rules = db.list_schedule_rules(Some("claude")).unwrap();
        assert_eq!(rules.len(), 2, "a rename must not drop the user's rules");
        assert!(rules.iter().all(|r| r.provider_id == "p1-new"));
        assert!(db.get_provider_by_id("p1", "claude").unwrap().is_none());
        assert!(db.get_provider_by_id("p1-new", "claude").unwrap().is_some());
    }

    #[test]
    fn rename_preserves_the_fallback_pointer() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        insert_provider(&db, "claude", "p1-new");

        db.delete_provider_remapping_schedule_refs("claude", "p1", "p1-new")
            .unwrap();

        // Deleting the old row without repointing first lets `ON DELETE SET NULL`
        // clear the user's fallback choice; this assertion pins that shut.
        assert_eq!(
            db.get_fallback_provider("claude").unwrap(),
            Some("p1-new".to_string())
        );
    }

    #[test]
    fn rename_leaves_other_providers_rules_alone() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        insert_provider(&db, "claude", "p2");
        insert_provider(&db, "codex", "p1");
        for (app, provider_id) in [("claude", "p1"), ("claude", "p2"), ("codex", "p1")] {
            db.create_schedule_rule(&NewScheduleRuleRequest {
                app: app.into(),
                provider_id: provider_id.into(),
                windows: sample_windows(),
                priority: 0,
                enabled: true,
                note: None,
            })
            .unwrap();
        }
        insert_provider(&db, "claude", "p1-new");

        db.delete_provider_remapping_schedule_refs("claude", "p1", "p1-new")
            .unwrap();

        let claude = db.list_schedule_rules(Some("claude")).unwrap();
        assert_eq!(claude.len(), 2);
        assert!(claude.iter().any(|r| r.provider_id == "p1-new"));
        assert!(claude.iter().any(|r| r.provider_id == "p2"));
        let codex = db.list_schedule_rules(Some("codex")).unwrap();
        assert_eq!(codex.len(), 1);
        assert_eq!(codex[0].provider_id, "p1");
    }

    #[test]
    fn fallback_set_and_get() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        assert_eq!(
            db.get_fallback_provider("claude").unwrap(),
            Some("p1".into())
        );
        db.set_fallback_provider("claude", None).unwrap();
        assert_eq!(db.get_fallback_provider("claude").unwrap(), None);
    }

    #[test]
    fn on_delete_provider_nulls_fallback() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        // simulate provider deletion (FK ON DELETE SET NULL)
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM providers WHERE id='p1' AND app_type='claude'",
                [],
            )
            .unwrap();
        }
        assert_eq!(db.get_fallback_provider("claude").unwrap(), None);
    }

    #[test]
    fn fallback_updated_at_tracks_only_a_set_fallback() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        assert_eq!(db.get_fallback_updated_at("claude").unwrap(), None);
        db.set_fallback_provider("claude", Some("p1")).unwrap();
        assert!(db.get_fallback_updated_at("claude").unwrap().is_some());
        db.set_fallback_provider("claude", None).unwrap();
        assert_eq!(db.get_fallback_updated_at("claude").unwrap(), None);
    }

    #[test]
    fn prune_switch_log_keeps_rows_under_the_cap() {
        let db = db();
        for day in 1..=5 {
            db.append_switch_log(
                "claude",
                "p1",
                &format!("2026-09-0{day}T09:00:00+00:00"),
                "rule",
            )
            .unwrap();
        }
        db.append_switch_log("codex", "p2", "2026-09-01T09:00:00+00:00", "fallback")
            .unwrap();
        // Exercises the retention SQL: under the 1000-rows-per-app cap nothing is dropped.
        db.prune_switch_log_for_app("claude").unwrap();
        assert_eq!(log_count(&db, None), 6);
    }

    fn log_count(db: &Database, app: Option<&str>) -> i64 {
        let conn = db.conn.lock().unwrap();
        match app {
            Some(a) => conn
                .query_row(
                    "SELECT COUNT(*) FROM schedule_switch_log WHERE app = ?1",
                    params![a],
                    |r| r.get(0),
                )
                .unwrap(),
            None => conn
                .query_row("SELECT COUNT(*) FROM schedule_switch_log", [], |r| r.get(0))
                .unwrap(),
        }
    }

    fn seed_switch_log(db: &Database, app: &str, rows: usize) {
        for i in 0..rows {
            db.append_switch_log(
                app,
                "p1",
                // Strictly increasing so "newest 1000" is unambiguous.
                &format!(
                    "2026-09-08T{:02}:{:02}:{:02}+00:00",
                    i / 3600,
                    i / 60 % 60,
                    i % 60
                ),
                "rule",
            )
            .unwrap();
        }
    }

    #[test]
    fn prune_switch_log_trims_one_app_to_the_cap() {
        let db = db();
        seed_switch_log(&db, "claude", 1200);
        db.prune_switch_log_for_app("claude").unwrap();
        assert_eq!(log_count(&db, Some("claude")), 1000);

        // The rows that survived must be the newest ones.
        let conn = db.conn.lock().unwrap();
        let oldest: String = conn
            .query_row(
                "SELECT MIN(fired_at) FROM schedule_switch_log WHERE app = ?1",
                params!["claude"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(oldest, "2026-09-08T00:03:20+00:00");
    }

    #[test]
    fn prune_switch_log_leaves_other_apps_untouched() {
        let db = db();
        seed_switch_log(&db, "claude", 1200);
        seed_switch_log(&db, "codex", 1200);
        db.prune_switch_log_for_app("claude").unwrap();
        assert_eq!(log_count(&db, Some("claude")), 1000);
        assert_eq!(log_count(&db, Some("codex")), 1200);
    }

    #[test]
    fn switch_log_lists_newest_first_with_provider_names() {
        let db = db();
        insert_provider(&db, "claude", "p1");
        db.append_switch_log("claude", "p1", "2026-09-01T09:00:00+08:00", "rule")
            .unwrap();
        db.append_switch_log("claude", "p1", "2026-09-02T09:00:00+08:00", "rule")
            .unwrap();
        db.append_switch_log("codex", "p9", "2026-09-03T09:00:00+08:00", "fallback")
            .unwrap();

        let all = db.list_switch_log(None, 10).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].fired_at, "2026-09-03T09:00:00+08:00");
        // `p9` was never inserted into `providers`, so the join yields no name and the
        // row still surfaces — history must not vanish with the provider.
        assert_eq!(all[0].provider_name, None);
        assert_eq!(all[0].reason, "fallback");
        assert_eq!(all[1].provider_name.as_deref(), Some("claude-p1"));

        let claude = db.list_switch_log(Some("claude"), 10).unwrap();
        assert_eq!(claude.len(), 2);
        assert!(claude.iter().all(|e| e.app == "claude"));

        assert_eq!(db.list_switch_log(None, 1).unwrap().len(), 1);
    }
}
