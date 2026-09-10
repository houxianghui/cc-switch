# Scheduled Provider Switching — Design Spec

- **Date:** 2026-09-08
- **Status:** Draft (awaiting user review)
- **Target version:** v3.21.0 (post-3.20.2)
- **Author:** brainstorming session
- **Spec location:** `docs/superpowers/specs/2026-09-08-scheduled-provider-switching-design.md`

---

## 1. Overview

Add the ability to **automatically rotate the active provider for each of the 8 supported CLI tools** (Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes) on a **weekly recurring schedule**. A user authors rules of the form "App X → Provider P during window W" and a background scheduler activates the matching provider at each window boundary. Manual switches made during a window persist for the remainder of that window; the next window resumes the schedule.

**Why now:** the existing single-instance app is a manual switchboard — toggling providers for a long workday (or a discount window) is friction. The feature is the top "automation" request from sponsor-driven feedback and the only one that does not require new external services (no cron library, no cloud sync, no new schema migration beyond a single version bump).

**Non-goals (v1):** cron expressions, cross-midnight windows, per-rule timezones, import/export of schedules, calendar-based one-off rules, "switch when usage exceeds X", sharing rules between users, exposing the scheduler over the deep-link URL scheme.

---

## 2. Goals & acceptance criteria

### 2.1 Goals

- A user can create one or more `(App, Provider)` rules, each with multiple weekly time windows, in under 60 seconds through the UI.
- The scheduler activates the right provider at the right window boundary without further user action, in tests and in the running app.
- The system degrades gracefully: tick failure does not crash; rules that reference deleted providers surface a clear UI signal; manual switches remain authoritative within a window.
- The change is **additive**: existing users with zero rules see no behavior change.

### 2.2 Acceptance criteria

| # | Criterion | Verification |
|---|-----------|--------------|
| AC1 | A user creates a rule, then closes the app, then reopens it inside the same window — the scheduled provider is already active (no manual switch required) | manual smoke + integration test E10 |
| AC2 | A user creates a rule, the window begins, and the scheduler switches the live config within 60s + evaluation time | integration test E3 |
| AC3 | A user manually switches during a window; the next tick in the same window does **not** override; the next window boundary **does** fire | integration test E8 / E9 |
| AC4 | Two enabled rules for the same app overlap; the higher-priority rule wins; equal-priority resolves to the most recently updated | unit test on `resolve_active_rule` |
| AC5 | When no rule covers the current time and a fallback provider is configured, the app is on the fallback; toggling fallback off restores the prior behavior | integration test E11 |
| AC6 | Deleting a provider cascades to delete its rules and nulls any fallback that referenced it | DAO test for `ON DELETE` |
| AC7 | The scheduler tick fails 3 times in a row, the nav badge turns red, and a banner surfaces a "Run now" retry | health endpoint + frontend test |
| AC8 | i18n keys exist in all 4 locales; the locale-coverage test still passes | `pnpm test:unit` + manual scan |
| AC9 | All existing unit, integration, and CI tests still pass (no regressions in provider switching, MCP, deep link, lightweight mode) | full test suite on Linux/Windows/macOS |
| AC10 | Schema v18 → v19 migration is idempotent and forward-only | `migrate_v18_to_v19_is_idempotent` test |

---

## 3. Confirmed design decisions (from brainstorming)

These are the answers the user gave during the brainstorm. They are the **constraints** the implementation must satisfy; any departure requires a new design pass.

| # | Question | Answer |
|---|----------|--------|
| D1 | Rule granularity | `(App, Provider)` — switching a provider switches its default model |
| D2 | Manual switch vs. schedule | Manual switch wins for the remainder of the **current window**; next window boundary resumes the schedule |
| D3 | Time-range shape | One rule may contain **multiple** time windows (e.g., 9-12 and 14-18) |
| D4 | Rule conflict resolution | Higher `priority` wins; equal-priority uses `updated_at DESC`, then `id DESC` |
| D5 | Missed schedules (app closed at boundary) | Do not fire, do not backfill; log a single `warn` |
| D6 | Time with no rule covering the app | A configurable per-app **fallback provider** (or "keep current") |
| D7 | Architecture choice | Approach A: rule-based, integrated with `ProviderService.switch`; **60s tokio tick**; **no new external dependency** |

---

## 4. Architecture

### 4.1 System diagram (new components in **bold**)

```
┌─────────────────────────────────────────────────────────────┐
│                  Frontend (React + TS)                      │
│  ┌─────────────────────┐  ┌─────────────────────────────┐   │
│  │ Schedules page      │  │ lib/api/schedule.ts         │   │
│  │  • rule list        │  │  (typed Tauri wrappers)     │   │
│  │  • add/edit dialog  │  └────────────┬────────────────┘   │
│  │  • fallback section │               │ invoke()            │
│  └──────────┬──────────┘               ▼                    │
│             │                  TanStack Query                 │
│             │                                                │
│             │  /schedules route, 4-locale i18n                │
└─────────────┼────────────────────────────────────────────────┘
              │ Tauri IPC (camelCase commands)
┌─────────────▼────────────────────────────────────────────────┐
│               Backend (Tauri + Rust)                         │
│  ┌──────────────────┐  ┌────────────────────┐                 │
│  │ commands/        │  │  services/         │                 │
│  │  schedule.rs     │──│   schedule.rs      │                 │
│  └──────────────────┘  │   (NEW)            │                 │
│                        │                    │                 │
│                        │  • 60s tick loop   │                 │
│                        │  • evaluate_now()  │                 │
│                        │  • evaluate_one()  │                 │
│                        └────┬───────────────┘                 │
│                             │                                 │
│        ┌────────────────────┼────────────────────┐            │
│        │                    │                    │            │
│  ┌─────▼──────┐  ┌──────────▼──────┐  ┌─────────▼────────┐   │
│  │ DAO:        │  │ ProviderService │  │ SwitchSource     │   │
│  │  schedule   │  │  (switch() now  │  │  enum (extend)   │   │
│  │  fallback   │  │   takes source) │  │  Manual,         │   │
│  │  app_state  │  └────────┬────────┘  │  Scheduled,      │   │
│  │  log        │           │           │  Deeplink,       │   │
│  └─────┬───────┘           │           │  Initial         │   │
│        │                   │           └──────────────────┘   │
│        ▼                   ▼                                 │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ SQLite DB (v19):                                       │ │
│  │  schedule_rules, app_schedule_state,                   │ │
│  │  app_fallback_providers, schedule_switch_log            │ │
│  │  + existing providers, mcp, prompts, skills, ...       │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                              │
│  8 live config files (existing) ←── atomic writes (existing)│
└──────────────────────────────────────────────────────────────┘
```

### 4.2 Module map

**New files:**

- `src-tauri/src/schedule_rules.rs` — types: `ScheduleRule`, `TimeWindow`, `NewScheduleRuleRequest`, `ScheduleRulePatch`
- `src-tauri/src/services/schedule.rs` — `ScheduleService` (tick loop, evaluate_now, evaluate_one)
- `src-tauri/src/database/dao/schedule.rs` — DAO for 4 new tables
- `src-tauri/src/commands/schedule.rs` — IPC handlers (camelCase commands)
- `src/components/schedule/SchedulesPage.tsx` — list view
- `src/components/schedule/AddEditRuleDialog.tsx` — create/edit modal
- `src/components/schedule/RuleCard.tsx` — single-rule card
- `src/components/schedule/FallbackSection.tsx` — per-app fallback
- `src/components/schedule/ProviderSelect.tsx` — app-filtered provider picker
- `src/hooks/useScheduleRules.ts` — TanStack Query hooks
- `src/hooks/useFallbackProviders.ts`
- `src/hooks/useScheduleHealth.ts`
- `src/lib/api/schedule.ts` — typed `invoke()` wrappers
- `tests/components/schedule/*.test.tsx` — frontend tests
- `tests/lib/api/schedule.test.ts`
- `tests/integration/schedule.test.tsx`
- `docs/user-manual/{zh,en,ja,zh-TW}/schedules.md` — user guide (4 locales)
- `docs/superpowers/specs/2026-09-08-scheduled-provider-switching-design.md` — this file
- `docs/superpowers/plans/2026-09-08-scheduled-provider-switching-impl.md` — implementation plan (created by `writing-plans`)

**Modified files:**

- `src-tauri/src/database/schema.rs` — schema v19 with 4 new tables
- `src-tauri/src/database/migration.rs` — `migrate_v18_to_v19`
- `src-tauri/src/database/mod.rs` — bump schema version constant
- `src-tauri/src/database/dao/mod.rs` — export new DAO
- `src-tauri/src/services/mod.rs` — `pub mod schedule;`
- `src-tauri/src/services/provider.rs` — `switch` signature gains `source: SwitchSource`
- `src-tauri/src/commands/mod.rs` — re-export schedule commands
- `src-tauri/src/commands/provider.rs` — existing switch calls use `SwitchSource::Manual`
- `src-tauri/src/commands/deeplink.rs` — deep-link switch uses `SwitchSource::Deeplink`
- `src-tauri/src/lib.rs` — `ScheduleService::start()` after DB init
- `src-tauri/src/tray.rs` — show next switch on tray; new tray menu items
- `src/App.tsx` — new `Schedules` nav entry
- `src/i18n/locales/{zh,zh-TW,en,ja}.json` — `schedule.*` and `nav.schedules` keys

---

## 5. Data model

### 5.1 New tables (schema v18 → v19)

```sql
-- (1) Rules: one row per (App, Provider) rule
CREATE TABLE schedule_rules (
    id              TEXT PRIMARY KEY,        -- UUID v4
    app             TEXT NOT NULL,           -- AppType enum value
    provider_id     TEXT NOT NULL,           -- soft ref to providers.id
    windows_json    TEXT NOT NULL,           -- JSON: [{"dow":[1..5], "start":"09:00", "end":"12:00"}, ...]
    priority        INTEGER NOT NULL DEFAULT 0,
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,           -- ISO8601 local
    updated_at      TEXT NOT NULL,
    note            TEXT                     -- nullable
);
CREATE INDEX idx_schedule_rules_app_enabled ON schedule_rules(app, enabled);
-- No FK on provider_id: providers are keyed by (id, app_type) and the cascade
-- happens at the service layer (Section 7.7). The cascade is enforced by
-- `Database::delete_provider_cascading_rules`, which removes the rules and the
-- provider row in one transaction, not by a foreign key on this table.

-- (2) Runtime state: one row per app, scheduler writes only
CREATE TABLE app_schedule_state (
    app                       TEXT PRIMARY KEY,
    last_window_start_at      TEXT,    -- ISO8601, last window's start that the scheduler acknowledged
    last_manual_switch_at     TEXT,    -- ISO8601, updated by ProviderService.switch when source=Manual
    last_scheduled_switch_at  TEXT,    -- ISO8601, audit
    updated_at                TEXT NOT NULL
);

-- (3) Per-app fallback provider
CREATE TABLE app_fallback_providers (
    app                  TEXT PRIMARY KEY,
    fallback_provider_id TEXT,             -- nullable; null = "keep current"
    updated_at           TEXT NOT NULL,
    FOREIGN KEY (app, fallback_provider_id)
        REFERENCES providers(app_type, id) ON DELETE SET NULL
);

-- (4) Audit log (best-effort, retention policy: keep last 1000 rows per app)
CREATE TABLE schedule_switch_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    app         TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    fired_at    TEXT NOT NULL,
    reason      TEXT NOT NULL  -- 'rule' | 'fallback' | 'rule_change' | 'launch' | 'manual_now'
);
CREATE INDEX idx_schedule_switch_log_app_fired ON schedule_switch_log(app, fired_at DESC);
```

### 5.2 TimeWindow data structure (frontend + backend)

```ts
interface TimeWindow {
  dow: number[];     // 0=Sun, 1=Mon, ..., 6=Sat; non-empty
  start: string;     // "HH:MM", 24h local
  end: string;       // "HH:MM", 24h local; must be > start (no cross-midnight in v1)
}
```

Validated by zod on the frontend and by the backend `create_schedule_rule` command. The backend is the source of truth.

### 5.3 Rule evaluation algorithm (pure function)

```rust
fn resolve_active_rule<'a>(
    rules: &'a [ScheduleRule],
    now: DateTime<Local>,
) -> Option<&'a ScheduleRule> {
    let mut candidates: Vec<&ScheduleRule> = rules
        .iter()
        .filter(|r| r.windows.iter().any(|w| w.matches(now)))
        .collect();
    if candidates.is_empty() { return None; }
    candidates.sort_by(|a, b| {
        b.priority.cmp(&a.priority)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(b.id.cmp(&a.id))
    });
    candidates.into_iter().next()
}
```

`TimeWindow::matches(now)`: `dow.contains(now.weekday().num_days_from_sunday()) && start <= now.time() < end` (half-open).

### 5.4 Decision table for one (app, now) pair

```
active = resolve_active_rule(enabled_rules_for_app, now)
match active:
    Some(rule):
        target = rule.provider_id
        reason = "rule"
    None if fallback_provider_id.is_some():
        target = fallback_provider_id
        reason = "fallback"
    None:
        no-op

if target.is_none(): return no-op

decision_start = match active:
    Some(rule): current_window_start_for(rules, now)
    None:       later_of(previous_window_end_before(rules, now), fallback_updated_at)
persist last_window_start_at = decision_start

if target == current_active_provider: return no-op (same)
if state.last_manual_switch_at >= decision_start:
    # user pinned since the scheduling decision now in force began
    return no-op (manual wins)
# else: fire
provider_service.switch(app, target, SwitchSource::Scheduled)
update last_scheduled_switch_at = now
insert log row (app, target, now, reason)
```

The pin decision compares `last_manual_switch_at` against `decision_start`, which is **recomputed from the rules and the fallback on every tick**, so it does not depend on the persisted `last_window_start_at` — that dependency was an ordering hazard, because a tick that returned early left the stored value stale and the pin could then never release. `last_window_start_at` is still written on every tick that resolves a `decision_start`, but purely as an observable ledger for the UI and for diagnostics. Naming both epoch sources also gives a fallback-only setup an epoch at all, which is what makes the manual pin work there (see edge cases E7 and E8).

---

## 6. Scheduler engine

### 6.1 `ScheduleService`

```rust
pub struct ScheduleService {
    db: Arc<Database>,
    provider: Arc<ProviderService>,
    cancel: Mutex<Option<broadcast::Sender<()>>>,
}

impl ScheduleService {
    pub async fn start(self: Arc<Self>) -> Result<(), AppError>;
    pub async fn stop(&self) -> Result<(), AppError>;
    pub async fn evaluate_now(&self) -> Result<EvaluationReport, AppError>;
    pub async fn evaluate_for_app(&self, app: AppType) -> Result<AppEvalReport, AppError>;
}
```

`EvaluationReport = { apps: Vec<AppEvalReport> }`, `AppEvalReport = { app, fired: bool, reason, from_provider: Option<String>, to_provider: Option<String>, skipped_due_to: Option<String> }`.

### 6.2 Tick loop

```rust
async fn run_tick_loop(self: Arc<Self>, mut cancel: broadcast::Receiver<()>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(e) = self.evaluate_now().await {
                    log::warn!("[schedule] tick evaluation failed: {e}");
                }
            }
            _ = cancel.recv() => { break; }
        }
    }
}
```

- **60s tick** — coarse enough that provider switching is well within tolerance
- **`MissedTickBehavior::Skip`** — system suspend/resume does not replay ticks
- **Failure is logged, not propagated** — one bad tick does not kill the loop
- **Runs in lightweight mode** — the scheduler does not depend on the main window

### 6.3 `SwitchSource` extension to `ProviderService::switch`

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwitchSource { Manual, Scheduled, Deeplink, Initial }

impl ProviderService {
    pub async fn switch(
        &self,
        app: AppType,
        provider_id: &str,
        source: SwitchSource,           // NEW
    ) -> Result<(), AppError> {
        // existing atomic-write logic unchanged
        if source == SwitchSource::Manual {
            self.dao.update_manual_switch_at(app, Utc::now()).await?;
        }
        if source == SwitchSource::Scheduled {
            self.dao.update_scheduled_switch_at(app, Utc::now()).await?;
        }
        Ok(())
    }
}
```

| Caller | source | When |
|--------|--------|------|
| Frontend `useProviderActions` | `Manual` | User clicks "Enable" on a provider card or selects it in the tray menu |
| `ccswitch://` deep link import-and-switch | `Deeplink` | Importer successfully applies a provider from a URL |
| Startup default-provider import (`import_default_config` on first launch) | `Initial` | The very first import when the DB has no provider for the app |
| `ScheduleService` tick / launch | `Scheduled` | Tick loop or `start()` evaluation |

`Initial` does **not** write `last_manual_switch_at` — first launch should not lock the scheduler. Only `Manual` writes that field.

This signature change is **intentional hard break** — the Rust compiler forces all call sites to update, eliminating silent behavior drift.

### 6.4 Tauri commands (camelCase per project convention)

```rust
#[tauri::command] pub async fn list_schedule_rules(app: Option<AppType>) -> Result<Vec<ScheduleRule>, AppError>;
#[tauri::command] pub async fn create_schedule_rule(new: NewScheduleRuleRequest) -> Result<ScheduleRule, AppError>;
#[tauri::command] pub async fn update_schedule_rule(id: String, patch: ScheduleRulePatch) -> Result<ScheduleRule, AppError>;
#[tauri::command] pub async fn delete_schedule_rule(id: String) -> Result<(), AppError>;
#[tauri::command] pub async fn get_fallback_provider(app: AppType) -> Result<Option<String>, AppError>;
#[tauri::command] pub async fn set_fallback_provider(app: AppType, provider_id: Option<String>) -> Result<(), AppError>;
#[tauri::command] pub async fn evaluate_schedule_now(app: Option<AppType>) -> Result<EvaluationReport, AppError>;
#[tauri::command] pub async fn get_next_scheduled_switch(app: AppType) -> Result<Option<NextSwitch>, AppError>;
#[tauri::command] pub async fn get_schedule_health() -> Result<ScheduleHealth, AppError>;
```

Errors are `AppError` (existing pattern); frontend uses i18n keys to render messages.

---

## 7. Provider deletion cascade

`ProviderService::delete` gains a hook that:
1. Deletes all `schedule_rules` rows where `(app, provider_id)` match (`dao.delete_rules_for_provider`).
2. Lets `app_fallback_providers.fallback_provider_id` go NULL via `ON DELETE SET NULL`.
3. Returns the count of deleted rules so the frontend can show a toast: "Deleted 3 schedule rules that referenced this provider."

UI:
- The list view shows rules referencing a missing provider with a `⚠ Provider missing` badge; opening the editor forces the user to re-pick a provider.
- Fallback section shows the same badge for nulled-out fallbacks.

---

## 8. Frontend

### 8.1 Navigation

`App.tsx` gains a new entry: **"Schedules"** (icon: `CalendarClock` from `lucide-react`) immediately after the "Providers" entry. The entry shows a red dot badge if `useScheduleHealth().consecutiveFailures >= 3`.

### 8.2 List page layout

```
┌─────────────────────────────────────────────────────────┐
│ Schedules                       [+ Add Rule] [↻ Run now]│
├─────────────────────────────────────────────────────────┤
│ Active Rules                          (3 enabled / 5)   │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ ● Claude Code  →  Provider A                         │ │
│ │   Mon-Fri 09:00-12:00, 14:00-18:00  · prio 10  [on] │ │
│ │   "workday rules"   [Edit] [Delete]                  │ │
│ └─────────────────────────────────────────────────────┘ │
│ ...                                                     │
├─────────────────────────────────────────────────────────┤
│ ▾ Per-app Fallback                                     │
│ Claude Code    [Provider C ▾]    (none = keep current) │
│ Codex          [— none — ▾]                            │
│ ...                                                    │
└─────────────────────────────────────────────────────────┘
```

- Cards, not table — each rule is a `(App × Provider × windows)` unit; cards read better
- Window summary collapses `[{dow:[1..5], 09-12}, {dow:[1..5], 14-18}]` → `Mon-Fri 09:00-12:00, 14:00-18:00`
- Priority label hidden by default, revealed on hover
- `[↻ Run now]` calls `evaluate_schedule_now()` and shows a summary toast

### 8.3 Add / Edit modal

Fields (all validated by zod):
- App: horizontal radio of 8 `AppType` values
- Provider: filtered dropdown (only enabled, non-deleted providers for the chosen app)
- Time Windows: sub-table with `[+ Add window]` and per-row `[✕ remove]`; each row has 7 dow checkboxes + two time inputs (`HH:MM`)
- Priority: integer input (0–1000)
- Enabled: switch
- Note: text input (optional)

Errors surface inline with i18n keys.

### 8.4 State hooks (TanStack Query)

```ts
useScheduleRules(app?): UseQueryResult<ScheduleRule[]>
useCreateScheduleRule(): UseMutationResult<...>
useUpdateScheduleRule(): UseMutationResult<...>
useDeleteScheduleRule(): UseMutationResult<...>
useEvaluateScheduleNow(): UseMutationResult<EvaluationReport, _, { app?: AppType }>
useNextScheduledSwitch(app): UseQueryResult<NextSwitch | null>
useFallbackProvider(app): UseQueryResult<string | null>
useSetFallbackProvider(): UseMutationResult<...>
useAllFallbackProviders(): UseQueryResult<Record<AppType, string | null>>
useScheduleHealth(): UseQueryResult<{ lastTickAt: string, consecutiveFailures: number }>
```

All mutations invalidate the relevant query keys on success.

### 8.5 i18n

New keys under `schedule.*` and `nav.schedules`. All 4 locales (zh, zh-TW, en, ja) must be updated; the locale-coverage test enforces this.

Key set: `title`, `addRule`, `editRule`, `app`, `provider`, `priority`, `enabled`, `note`, `windows`, `addWindow`, `removeWindow`, `dow.mon..sun`, `summary.daily`, `summary.weekdays`, `summary.weekends`, `summary.complex`, `fallback.title`, `fallback.none`, `health.ok`, `health.degraded`, `nextSwitch`, `error.crossMidnight`, `error.emptyWindows`, `error.emptyDow`, `error.providerMissing`, `runNow`, `runNow.result`, plus `nav.schedules` and `tray.scheduled{Active,Idle,Fallback}`.

### 8.6 Tray integration

`tray.rs`:
- Below the current provider name, add a smaller-font line: `⏰ Scheduled → {next provider} in {rel}` or `🕒 No active rule` or `🕓 Fallback active` (text per i18n)
- The line is computed by a synchronous helper `ScheduleService::next_switch_for(app, now: DateTime<Local>) -> Option<NextSwitch>` that mirrors `evaluate_one` but does not perform any switch. It scans enabled rules + fallback and returns the earliest future boundary. Complexity is O(rules × windows_per_rule); no I/O.
- Refreshed on every tray rebuild (every 30s)
- Tray menu gains two items: "Open Schedules" (show window + route to /schedules) and "Run Scheduler Now" (call `evaluate_schedule_now(None)`)

---

## 9. Edge cases (full list)

Numbered E1–E20; details in brainstorm transcript. Summary:

| # | Scenario | Behavior |
|---|----------|----------|
| E1 | User changes system clock | Next tick re-evaluates; no special handling |
| E2 | DST transition | `chrono::Local` handles; no special code |
| E3 | Rule created mid-window | `evaluate_now` runs on create; fires if priority wins and no manual switch |
| E4 | Rule disabled | `list_enabled_rules` filter; other rules unaffected |
| E5 | Provider deleted | Section 7: cascade delete rules, NULL fallback, UI badge |
| E6 | Tie on priority + updated_at | `id DESC` tie-break |
| E7 | Manual switch outside any window | `last_manual_switch_at` updates. The epoch outside a window is the later of the previous window's end and `fallback_updated_at`, so the manual choice out-ranks the fallback and pins it; with no windows at all the pin releases only when the fallback is re-saved |
| E8 | Manual switch spanning a window boundary | The pin is scoped to one decision: it releases at the **end** of the window it was made in, because the next window's start becomes the new epoch and is later than the manual switch |
| E9 | Manual switch at window start | `manual >= window_start` → manual wins |
| E10 | App started inside a window | `evaluate_now` on startup fires immediately if conditions met. **Not a backfill** — we are not replaying missed boundaries; we are simply evaluating the current state on launch, which is what the scheduler does every tick anyway. Consistent with D5 (no backfill). |
| E11 | App started outside any window | Fallback applied if set; else no-op |
| E12 | Lightweight mode | Scheduler runs; no window dependency |
| E13 | Adjacent windows (12-14 and 14-18) | Half-open interval; 14:00:00 belongs to the second |
| E14 | Zero-length window (`start == end`) | Rejected by validation |
| E15 | Cross-midnight window | v1 rejected; documented for v2 |
| E16 | Provider has unsaved edits | Scheduler reads stored state; UI keeps its own unsaved-edit indicator |
| E17 | Fallback references deleted provider | DB `ON DELETE SET NULL`; UI shows "none" |
| E18 | Double-submit on Save | Mutation fails on second click; UI debounce + backend id check |
| E19 | "Run now" while main window hidden | Invokes directly; result as toast |
| E20 | i18n key missing | locale-coverage test fails in CI |

---

## 10. Testing

**Unit (Rust, no IO):**
- `resolve_active_rule` — all combinations of priority / updated_at / id ties
- `TimeWindow::matches` — half-open, dow boundaries, edge of week
- `current_window_start_for` — cross-day, cross-week
- Frontend utility `summarize_windows` — dow set, multi-window, single-window

**DAO (Rust + real SQLite in `tempfile`):**
- 4 tables CRUD
- `migrate_v18_to_v19` idempotency (run twice → no change)
- `ON DELETE SET NULL` for fallback
- `delete_rules_for_provider` cascade

**Integration (Rust + `tokio::time::pause` + mock `ProviderService`):**
- E3, E8, E9, E10, E11 via virtual time
- `SwitchSource` parameter propagation
- `MissedTickBehavior::Skip` behavior

**Frontend (vitest + MSW + Testing Library):**
- `AddEditRuleDialog` zod validation
- `RuleCard` summary algorithm
- `FallbackSection` state transitions
- `useScheduleRules` query keys + invalidation
- Integration: create rule → mock `evaluate_schedule_now` returns `fired: true` → UI updates

**Manual smoke (pre-merge):**
1. 0:00 across midnight
2. Change system timezone (macOS: turn off "auto timezone")
3. Delete a provider while a rule references it
4. Lightweight mode: rule still applies (verify by reading live config)
5. System suspend 5 min, resume — no replay
6. Fallback toggles only once (no flip-flop)

---

## 11. Migration & release

- Schema version: **18 → 19** (single bump)
- Migration function: `migrate_v18_to_v19(conn: &Connection) -> Result<(), AppError>` in `src-tauri/src/database/migration.rs`
- Forward-only; no data backfill
- Idempotency test in DAO test suite
- Rollback: users on v20 downgrading to v18 will hit the existing `db_version_too_new` recovery path (no new code needed)
- **Default state:** no rules → zero behavior change for existing users
- CHANGELOG entry drafted below; maintainers finalize before release

```
### Added
- **Scheduled Provider Switching**: 在 App × Provider 维度上为 8 个支持的 CLI 工具
  配置"工作时间用 A、下班用 B"的定时切换规则。每条规则可包含多个时间窗（每周几 +
  开始/结束时间），多条规则用 priority 解决冲突。调度精度 60s；窗口外可设 per-app
  fallback provider；手动切换在本窗口内生效、下一窗口重置；关闭期间不追补。
  本机时钟，本地时区。UI：主导航新增"Schedules"页；托盘显示下次切换倒计时。
  SQLite schema v18 → v19。

### Notes
- 本特性默认关闭（无规则 = 无行为），对现有用户零影响
- 跨日窗口（如 22:00-06:00）v1 不支持
```

---

## 12. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| 60s tick + `ProviderService.switch` write fails | Do **not** update `last_scheduled_switch_at` on failure; next tick retries the same rule |
| Source priority confusion across callers | Section 6.3 call-site table; compiler-enforced on `switch` signature |
| Conflict with proxy failover | None: scheduler is plan-level (per minute), failover is request-level (per request); they compose |
| Large rule counts | 8 apps × 100 rules × 14 windows = 11200 comparisons/tick; < 1ms; no optimization needed |
| Per-rule timezone (future) | v1 stores local times; v2 adds `timezone: Option<String>` without breaking v1 data |
| `ProviderService::switch` signature break | Intentional; compiler catches all call sites |

---

## 13. Out of scope for v1 (explicit)

- Cron expressions
- Cross-midnight windows
- Per-rule timezones
- Schedule import/export
- One-off calendar rules
- "Switch when usage > X" rules
- Sharing rules between users
- Deep-link `ccswitch://schedule/...` URL scheme
- Schedule history UI (the log table is for ops queries, not user-visible)

---

## 14. Documentation deliverables

- `docs/user-manual/{zh,en,ja,zh-TW}/schedules.md` — user guide
- `docs/user-manual/README.md` — index updated
- `README.md` / `README_ZH.md` feature list — add a bullet under "Provider Management"
- `CHANGELOG.md` — maintainer-curated entry (draft provided in brainstorm)

---

## 15. Open questions

None at this point. All design-relevant questions were answered during the brainstorm. Implementation-time questions (e.g., exact `dow` constant naming, error message wording, the order of fields in the modal) are within the plan's scope and do not require a spec change.

Two questions the user might still ask and that have a **defended answer** in this spec:

1. **"Why not just compute `next_fire_at` and sleep until then?"** — Because rule changes (create/update/delete), provider edits, and manual switches all need to invalidate the sleep. A 60s tick evaluates O(rules) cheaply and is robust to all of these. Optimization not justified at the data scale we expect.
2. **"Why not use `tokio-cron-scheduler`?"** — Cron is more expressive than the user needs, harder to render in a form, and adds a dependency. The weekly-recurring-with-multiple-windows model covers all the use cases surfaced during the brainstorm.

---

## 16. Approval gate

This spec is **complete and internally consistent**. Next step: user reviews, then `superpowers:writing-plans` produces the implementation plan.
