# LU.5 — showing what arrived

Status: designed (2026-11-22), not built. Blocked by LU.3 (the shell wiring).

Source: `docs/ROADMAP.md` → "Live updates", LU.5. The design is `docs/DESIGN.md` →
"Live updates: showing what arrived", drawn in the Orre Paper file on
Transactions · what arrived (LU.5) (`JJY-0`) and Live updates · LU.5 states (`JYE-0`).
DESIGN.md is the spec for looks, copy and timings; this brief is how to build it.

## Problem

With LU.3 the app reloads views when another process changes the database (an MCP
client filing suggestions, `orre import`, `undo-import`, `categorize`, `inbox approve`),
but the change lands silently. A user looking at Transactions doesn't know 9 rows
just arrived, and the Inbox badge only changes its number.

## What to build

1. **Arrivals, in the shell.** When LU.3 hands the shell a set of changed topics,
   (LU.3's `shell/live.rs`, being built in another thread) compare what the views held before the reload with what they hold after:
   - Inbox: pending suggestion ids, grouped by `suggestion.source_client` (display name
     from the `mcp_client` row). New ids are arrivals.
   - Ledger: transaction ids in the loaded data. New ids → "new"; ids whose
     user-facing fields differ → "changed"; ids gone → "removed" (and whether an
     `import_batch` went to `undone`, for the "import undone" copy).
   Derive this from the reloaded data, not from a new query on the watcher: `core`'s
   watcher only names topics. Keep the diff logic out of GPUI code (a plain function over
   two snapshots, unit-tested), per AGENTS.md → "Keep business logic out of `app`".
   Only reloads nobody asked for count: the app's own writes already show an undo bar.
2. **The arrival line.** `Status`'s `new` field (`crates/app/src/shell/status.rs`) stops being the
   sample's "3 new" and becomes the arrival: lead (sparkle / molten dot / none), copy,
   and click target. `StatusItem` needs a `text-secondary` text colour and a sparkle lead
   (`ui/status_bar.rs`). Copy table and the 24-character name cut: DESIGN.md. Snapshot
   and sample data: keep a sample arrival so `cargo run --example snapshot` matches
   `JJY-0` ("3 new transactions").
3. **Badge unseen state.** `ui::badge` gains an unseen variant (`moldavite` fill,
   `on-moldavite` text). The shell tracks "unseen" per arrival; the Inbox tab's badge uses
   it.
4. **Row marks.** Transactions' table and the Inbox list take a set of marked ids from
   the shell and draw the 6px molten dot in the body's 8px left inset (absolutely
   positioned, so nothing reflows). Accounts, Rules and Settings: no marks.
5. **Seen and the clock.** An arrival is seen when its view is on show in a focused
   window, or when the line is clicked (which opens the view, changing nothing in it).
   Seen starts one 10s hold; then the line, marks and unseen fill fade out over 100ms.
   Unseen has no limit. Same kind + same source while unseen adds to the count; a
   different arrival replaces the line. Use the shell's existing timer pattern
   (`ShellView::keep_time`); tests and snapshots don't run timers, so make the clock
   injectable (as the status bar's "now" already is).
6. **Undo-import.** Removed rows go at once; the selection keeps what's still there and
   never gains a row. If every selected row went, the inspector shows the "This
   transaction was removed" Empty state until the next selection (copy in DESIGN.md,
   using the data loaded before the reload). An inline edit on a removed row closes; a
   dialog about one stays open with its primary disabled and the caption.
   Check this against LU.4's tests: LU.4 says inline edits and dialogs survive a reload;
   a removed row is the one exception, and needs its own test.
7. **Reduced motion.** Fades are 0ms under `App::reduce_motion`; the 10s hold isn't motion.

## Rules kept

- Nothing takes focus, scrolls, or changes selection, filters, search, an inline edit or a
  dialog (LU.4). No toast, overlay, OS notification or sound.
- No new tokens (DESIGN.md → "Tokens"); molten = new, moldavite + sparkle = an AI client.
- Transaction content isn't logged at `info` or above (the arrival copy has counts, a
  client's display name and a file name; keep them out of logs anyway).
- No migration and no new MCP behaviour: MCP still writes only `suggestion` and `report`.

## Acceptance

- Unit tests for the snapshot diff: new, changed, removed, suggestions grouped by client,
  counts adding while unseen, a different arrival replacing the line.
- `#[gpui_kit::test]` on the shell's `describe()`: an outside import lands mid-task and
  the status line, row marks and badge show; selection, scroll, filters and an open inline
  edit are unchanged (extends LU.4's tests). Undo-import removing the selected row shows
  the inspector note; a removed row's inline edit closes.
- Timings: seen → 10s → gone, driven by an injected clock; unseen never times out.
- `cargo run --example snapshot` matches `JJY-0`.
- In the running app (`orre dev`, scratch `ORRE_DATA_DIR`): `orre import` in a second
  terminal shows "N new transactions" and marked rows within about 2s; an MCP client's
  suggestions turn the Inbox badge unseen.
- fmt, clippy `-D warnings`, `cargo test` pass. Tick LU.5 in `docs/ROADMAP.md`.

## Out of scope

- Marks in Accounts, Rules, Settings, Budgets, Overview and Trends (they just reload).
- A history of arrivals, or arrivals from the app's own writes.
- LU.6 (cost).
