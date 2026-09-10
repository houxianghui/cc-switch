# Scheduled Provider Switching

> CC Switch can rotate the active provider for any of the 9 supported CLIs on a schedule you define — for example "use my work relay Mon–Fri 09:00–18:00, and my personal key the rest of the time." This guide covers the concepts, the exact switching behavior, and every UI control on the **Schedules** page.

## Who this is for

- You pay for two or more providers and want each one used during the hours it makes sense (a cheap off-peak relay at night, the official endpoint during work hours).
- You share a machine or an account quota with a team and want the switch to happen without anyone remembering to click.
- You want a single "office hours" provider for Claude Code but a different default for Codex — schedules are **per app**, so each CLI has its own rules and its own fallback.

If you only ever use one provider per app, you don't need this feature — keep using the card toggle in **Providers**.

## Concepts

Four terms show up everywhere in the UI. It's worth reading these once.

- **Schedule rule** — a binding `(app, provider, time windows, priority, enabled, note)` that says "use provider X for app Y during these hours." Each rule belongs to exactly one app and points at exactly one provider.
- **Time window** — a `(days of week, start, end)` triple, in **your machine's local time**. Days are picked as Mon–Sun chips; start and end are `HH:MM`. A rule can hold several windows (e.g. weekday mornings _and_ Saturday afternoons).
- **Per-app fallback** — the provider to switch to when **no rule is currently active** for that app. Optional. Set it to `— none — (keep current)` to leave the active provider untouched outside your windows.
- **Manual pin** — if you switch a provider by hand, the scheduler steps aside and will not overwrite your choice until the scheduling decision currently in force is replaced. With a rule active that means the end of the current window; with only a fallback set it means the next time you save that fallback.

A rule in JSON form (this is what gets stored; you never have to type it, but it makes the shape clear):

```json
{
  "app": "claude",
  "provider_id": "a7f3c1e2-...",
  "windows": [{ "dow": [1, 2, 3, 4, 5], "start": "09:00", "end": "18:00" }],
  "priority": 0,
  "enabled": true,
  "note": "Work relay, office hours"
}
```

`dow` is `0` = Sunday through `6` = Saturday.

## How the scheduler works

- **Tick interval: 60 seconds.** The scheduler wakes once a minute, works out what _should_ be active for each app right now, and switches if that differs from the current provider. It also runs one evaluation immediately at app startup, so launching CC Switch inside a window activates the right provider without waiting up to a minute.
- **Resolution order per app:** the highest-priority enabled rule whose window covers "now" wins. Ties are broken by the most recently edited rule. If no rule covers "now", the per-app fallback is used. If there's no fallback either, nothing happens — the current provider stays.
- **Manual wins over the decision in force.** The scheduler compares your last manual switch against the moment the decision now in force began. That moment is the current window's start when a rule is active, and — when no rule is active but a fallback is set — the later of the end of the window that just ended and the time you last saved that fallback. If your manual switch is the more recent of the two, the app is skipped. A manual switch is therefore respected in a fallback-only setup too, and there it does not expire at a window boundary, because there are no windows: it releases when you next change the fallback.
- **Missed ticks are not replayed.** If your machine sleeps for three hours, the scheduler does not fire three hours' worth of catch-up switches — it just evaluates the present moment on the next tick.
- **Every fired switch is logged.** The tray label is a separate mechanism: it is recomputed from your current rules and fallback rather than read back from that log, and only when the tray menu is rebuilt — not on hover.

## Opening the Schedules page

Click the **calendar-clock icon** in the app header. You can also right-click the tray icon and choose **Open Schedules** — that raises the main window and refreshes the Schedules page if it's already open.

## How to add a rule

1. Open **Schedules**.
2. Click **Add rule**.
3. Pick the **App**. Every supported CLI is listed: Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes, Pi.
4. Pick the **Provider**. Only providers already configured for that app are listed.
5. Set the **Time windows**. A new rule starts with a sensible default of Mon–Fri 09:00–18:00. Toggle the day chips, then edit start/end. Click **Add window** for a second block.
6. Optionally set a **Priority** (higher number wins when two rules overlap) and a **Note** for your own reference.
7. Leave **Enabled** on and click Save.

The rule appears as a card, showing a human-readable window summary (`Mon-Fri 09:00-18:00`, `Daily 22:00-23:59`, or `3 windows` when it's more complex than that).

## How to set the fallback

1. Open **Schedules**.
2. In the **Per-app fallback** section, find the app you want.
3. Choose a provider from the dropdown — that provider becomes active whenever no rule covers the current time.
4. Choose `— none — (keep current)` if you'd rather the scheduler leave the provider alone outside your windows.

Fallback saves immediately; there's no separate confirm step.

## How to edit, disable or delete a rule

- **Edit** — use the pencil button on the rule card. The provider, windows, priority, note and enabled flag are all editable. The **App** is fixed on an existing rule; to move a schedule to a different CLI, create a rule there and delete this one.
- **Disable without losing it** — flip the **Enabled** switch on the rule card. A disabled rule is completely inert: it never fires, and it never blocks a lower-priority rule from winning.
- **Delete** — use the delete button on the card. This removes the rule only; the provider itself is untouched.

Disabling is usually what you want when you're testing, since it's reversible with one click.

## How to run the scheduler on demand

You don't have to wait up to 60 seconds to see whether a rule works.

- **Schedules page → Run now.** A toast reports the outcome, e.g. `Fired 2, skipped 6` — "fired" means a switch actually happened, "skipped" means the app was already on the right provider, or was manually pinned, or had nothing to do.
- **Tray menu → Run Scheduler Now.** Same evaluation, without opening the window.

## What to expect while it's running

- **Tray tooltip / menu label** — one of:
  - `Scheduled: <provider> until <time>` — a rule is active right now.
  - `Scheduled: fallback to <provider>` — no rule covers now, the fallback is in charge.
  - `Scheduled: idle (no rule active)` — nothing covers now and there's no fallback.
- **On the page** — a **next-switch hint** appears for every app that has an enabled rule, naming the provider that is queued next and how far off it is; apps with nothing upcoming are simply left out. If switching fails repeatedly (3 or more consecutive failures — a provider's config file can't be written, for example), a red banner appears with the failure count. Every tick records its own outcome, which is what makes that count trustworthy. The banner is your cue to check the provider's own settings.

## Deleting a provider that has rules

Schedule rules point at a provider, so a provider can't be deleted out from under them. When you delete a provider in **Providers**, you will be asked to confirm; once you do, the provider row and every schedule rule that pointed at it are removed together in a single SQLite transaction (a _cascade delete_), so a failure at either step leaves nothing half-deleted. The confirmation dialog does not tell you how many rules are affected, but a toast right after the delete reports how many schedule rules went with it.

If a rule somehow ends up pointing at a provider that no longer exists, its card shows `Provider no longer exists` — use the pencil button to repoint the rule at a live provider, or delete it.

## Limitations in this version

- **No cross-midnight windows.** A window must start and end on the same day. `22:00–02:00` is rejected with `Cross-midnight windows are not supported in v1`. Split it into two windows instead: `22:00–23:59` today and `00:00–02:00` on the following day's chip.
- **Local time only.** Windows are evaluated against your machine's local clock. There's no per-rule time zone, and no DST-aware "shift the window" behavior — after a DST change the window is still `09:00–18:00` on the wall clock.
- **Minute granularity, 60-second resolution.** A switch can land up to a minute after the window boundary.
- **One provider per rule.** There's no round-robin or load-spread within a rule; use several rules with different windows.
- **The manual pin is released by a new decision, not by a timer.** With a rule active it releases at the end of the current window — click **Run now** once the boundary has passed if you don't want to wait for the next tick.
- **A fallback-only setup stays pinned until you re-save the fallback.** If you have a fallback and no rules, one manual switch suppresses the fallback until you save the fallback again: there is no window boundary at which the pin could expire, so there is no time-based release. This is deliberate — an explicit choice outranks a standing default — but it does mean the fallback looks broken until you re-select it in **Per-app fallback** on the Schedules page.

## FAQ and troubleshooting

**The scheduler didn't switch at 09:00 — why?**
Most often the manual pin: you switched the provider yourself at or after the start of the current window, so the scheduler is deliberately yielding. Wait for the window to end, or click **Run now** once it has. If you have no rules and only a fallback, the pin has no boundary to expire at — re-save the fallback to release it. Second most common cause: the rule's **Enabled** switch is off.

**Two rules overlap. Which one wins?**
The one with the higher **Priority**. If priorities are equal, the most recently edited rule wins. Give overlapping rules distinct priorities to make the outcome obvious.

**Do schedules keep running when the window is closed?**
Yes. CC Switch keeps running in the tray, and the tick loop runs with it. If you fully quit the app, nothing is scheduled until you launch it again — at which point it evaluates immediately and catches up to the present moment.

**Do my rules survive a restart?**
Yes, rules and fallbacks are stored in CC Switch's database, not in memory.

**Can I schedule something other than a provider — a model, a proxy setting?**
Not in this version. A rule switches the active provider for an app, which is the same operation as clicking the provider card by hand, so anything bundled into that provider (endpoint, key, model mapping, routing) comes along with it.

**Can two apps share a rule?**
No — each rule targets one app. Create one rule per app. This is deliberate: it keeps "which CLI am I changing?" unambiguous.

**Where's the log?**
Every fired switch is recorded. The tray label is not read back from that log — it is recomputed from your current rules and fallback each time the tray menu is rebuilt. For deeper diagnosis, the app log records tick failures with a `[schedule]` prefix.

## Related

- [Local Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [Keep Codex Remote Control and Official Plugins While Using Third-Party APIs](./codex-official-auth-preservation-guide-en.md)
