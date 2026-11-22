# Orre — Architecture: The app

The app shell, window state, the status bar, and how the app opens the database and applies edits.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## The app

The rules in force, drawn from the records below. Where a later record changed an
earlier one, only the current rule is stated, with the record that set it.

### The shell and navigation

`orre_app::shell` (M3.1; "the app shell and window state", points 1 and 6). `orre`
with no subcommand opens one window: the title bar with eight view tabs (Overview,
Transactions, Trends, Budgets, Net worth, Inbox, Reviews, Rules) and a Settings button,
the body (the view's own left panel, canvas and inspector), and the status bar. There
is no sidebar: the five views of the money share the accounts panel, and Inbox,
Reviews, Rules and Settings have their own lists. ⌘1–⌘8 open the views in tab order,
⌘, opens Settings, ⌘I imports and ⌘N adds a transaction on Transactions (Ctrl on Linux
and Windows). The Settings button toggles back to the view it was opened from.
Transactions, Inbox, Rules and Settings are built (`View::is_built`); the others show
the shell around an empty state ("{View} isn't built yet") with ⌘2 to Transactions.
The shell opens every dialog over everything when a view asks.

### Window state

Key `app.window` in the `setting` table, a JSON `orre_core::setting::WindowState`:
mode (windowed, maximised, full screen), restore bounds in whole logical pixels and the
open view's key. Core refuses an out-of-range write and reads a bad value as an error
("the app shell and window state", point 2). `shell::window_state::WindowStore` saves
500 ms after the window stops moving, resizing or changing view (`SAVE_DELAY`), on a
background thread, and at once on the UI thread when the window closes or the app
quits. On open, a window whose title bar wouldn't show 40×40 px on any display opens
centred at 1440×900, a smaller one grows to the minimum, and an unknown view key opens
Transactions (points 4 and 5; `window_state::restored_bounds`). Any failure, a restore
in progress included, leaves the defaults with a warning on stderr and never stops the
app (point 3).

### The status bar

`shell::status::Status`, built from what the views already hold, so it adds no reads
("the status bar reads the database", which supersedes point 8 of "Settings, AI access
and backups (built)" on the fixture data). An item with nothing true to say is left
out. "AI access off" while the kill switch is off, else "N agents connected" counting
the enabled clients (none enabled: left out); either opens Settings → AI clients.
"Reconciled" needs at least one balance check, and failing checks replace it. The
import line is the newest committed import in an open account, its time reread every
30 seconds (`ShellView::keep_time`). The transaction count, with excluded rows when
there are some. The currency only when every open account shares one. "3 new" is left
out. With the sample data (`transactions::Counts`' `sample`) it keeps Paper's
artboard for snapshots.

### Opening the database

- **One gate.** Every per-call open, in the app and the MCP server, goes through
  `orre_core::backup::gate::open`: it checks the restore lock, runs the caller's
  opener, and checks again, refusing ("A backup is being restored; try again when it's
  done.") and closing what it opened if a restore held the lock at either check. The
  lock is checked for an instant, not held for the call ("one gate for opening the
  database, and the Transactions edit pipeline", points 1 and 2).
- **No connection is held.** The app opens with `Database::open` (creating and
  migrating as the CLI does, with the migration backup) at start-up and again for each
  read or write, through its stores (`AccountStore`, `TransactionStore`, `InboxStore`,
  `ChecksStore`, `RulesStore`, `SettingsStore`, `ImportStore`, `WindowStore`), so a
  restore can swap the file ("the app shell and window state", point 3; "one gate for
  opening the database, and the Transactions edit pipeline", point 5, which updates
  "accounts management", point 6, "Inbox", point 8, and "the import flow"). After a
  restore, Settings and the accounts read the database again.

### Applying edits

Transactions and Rules write through one edit pipeline each
(`TransactionsView::write`/`query` in `transactions/write.rs`, `Rules::write`/`query`
in `rules/write.rs`): a change is a policy (where its error is said, the undo-bar
offer, the event for the shell) and one call to core on the database the pipeline
opens; the pipeline owns the no-database refusal, the reload and the event. Their
stores keep only opening and reading ("one gate for opening the database, and the
Transactions edit pipeline", point 3; `rules.md`, "the Rules view's edit pipeline and
section state"). Accounts, Inbox, Checks, Settings and Import
still forward calls through their store's methods (point 4). A single-row edit on
Transactions shows no undo bar; links, changes to several rows, exclusions and
transfer decisions do.

### What runs off the UI thread

The budget is 50 ms on the UI thread at 100,000 transactions ("performance at 100,000
transactions"; `crates/core/examples/ledger_bench.rs` measures it).

- **Background:** the Transactions register (read at start and after each change, then
  filtered in memory); the Inbox (`Inbox::reload` on `background_spawn`; the shell
  builds it with `Inbox::unloaded`, and `InboxStore::load` previews every pending
  suggestion with `inbox::preview::preview_all`); the accounts (`Accounts::reload`;
  the shell builds them with `Accounts::unloaded` and starts the first read) and the
  Rules view (`Rules::reload`; `Rules::unloaded`, first read when `RulesView` is made),
  each keeping the last read on screen until the next lands ("Accounts and Rules read
  in the background"); everything in the import dialog that touches the disk or
  database (`ImportStore`); backing up, checking a backup and restoring in Settings;
  the delayed window-state save.
- **UI thread:** the Checks and Settings reads; every write (Accounts, Rules, Inbox
  decisions and the rest; each then asks for a background read); Inbox `covered_by` on
  selection; the Rules view's other reads (an apply preview, the suggestions over a
  rule, a change's items, what undoing would do).

### `describe()` stays out of release builds

A view's `describe()` (its rows, selection, dialog and actions as plain data) and
anything that runs a view's actions from outside the window are compiled only for tests
and the `dev-driver` feature (`ui::describe`, each view's `describe` module,
`shell::describe` and `dev_driver`). That feature is off by default, enabled by no other
feature, and a compile error in a release build. Its driver works only against a scratch
`ORRE_DATA_DIR` and listens only on a local socket. No MCP tool, resource or prompt
reaches them, in any build. `describe()` is built from the same functions the view
renders with and the same sanitising (`envelope::shown`), so it shows what a person sees
("views that describe themselves stay out of the release binary", points 1 to 5, and
"the dev driver").

### The dev driver

A debug build with `--features dev-driver`, opened with `ORRE_DATA_DIR` set to a
scratch dir, listens on `$ORRE_DATA_DIR/dev-driver.sock`. `orre dev <command>` (built
with the same feature) sends one line and prints the reply:

```sh
ORRE_DATA_DIR=/tmp/orre-dev cargo run --features dev-driver          # the app
ORRE_DATA_DIR=/tmp/orre-dev cargo run --features dev-driver -- dev describe
#   … dev show inbox | press down a | type tesco | click search | run Approve
```

`describe` prints `ShellView::describe` (`shell::describe`): the open view's
description, the dialog over it (its own `describe()`, or else its labelled controls)
and what has focus. `run <label>` clicks the one visible, enabled control with that
accessible label, inside the dialog when one is open. Where a label repeats, the error
lists the controls' ids for `click <id>`. Keys go through `press`, as the descriptions
list them. A test per view checks that every button its `describe()` lists can be
clicked this way (`dev_driver::tests`).

### Live updates

Not built yet (ROADMAP → "Live updates", LU.1 to LU.6). The app notices a change made by
another process (an MCP client filing suggestions, `orre import`, `undo-import`,
`categorize`) by itself, because SQLite can't push one. Once a second it reads the size
and modified time of the database and its `-wal` file, plus the database file's identity,
and opens nothing. A changed stamp leads to one open through `backup::gate::open` that
fingerprints four topics (Inbox, Ledger, Rules, Settings) and names the ones that differ;
the shell reloads those views (in the background where the view's read already is:
Transactions, the Inbox, Accounts, Rules; Checks and Settings read on the UI thread
today) and keeps the user's place. The reasons,
the fingerprints, the intervals and what was rejected are in "live updates" below.

## Decision records

Oldest first.

### Decision record: the app shell and window state (2026-11-14)

M3.1. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build.
Everything else follows `DESIGN.md` or an earlier record.

1. **Navigation is the title bar's view tabs, not a sidebar.** ROADMAP M3.1 said "sidebar
   navigation", written before the design track. Paper's shell (Rules `B1J-0`, Settings ·
   AI clients `AQ2-0`, and `DESIGN.md` → "Finance screens") navigates with eight view tabs
   in the title bar (Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews,
   Rules) and a Settings icon button before the window controls. The left panel belongs to
   the view: the five views of the money share the accounts panel, and Inbox, Reviews,
   Rules and Settings get their own lists with their screens. Paper wins, so the shell
   (`crates/app/src/shell.rs`) follows it. Keyboard: ⌘1–⌘8 open the views in tab order and
   ⌘, opens Settings (Ctrl on Linux and Windows). Paper draws no view shortcuts; these are
   keyboard paths, not visual values. The tabs are `gpui-base` `Tab`s (tab role, selected
   state, position in the list), not Tab stops. Clicking the Settings button while
   Settings is open goes back to the view it was opened from (Transactions if the window
   opened on Settings); ⌘, only opens it, as the platform shortcut does elsewhere (user).
2. **Window state lives in the `setting` table (user).** Key `app.window`, a JSON
   `WindowState` (`orre_core::setting`): mode (windowed, maximised, full screen), the
   restore bounds in whole logical pixels, and the open view's key. Core checks the range
   (sides 1–32768, origins within ±32768, a view key of at most 32 `[a-z_]`) and refuses an
   out-of-range write; a stored value that doesn't parse reads as an error and the app uses
   its defaults. It isn't exported and no MCP tool reads it. Restoring a backup rolls it
   back with everything else, which is harmless. The rejected option was a separate
   `window.json` in the data dir, which would have kept the GUI off the database until M3.2.
3. **So the GUI opens the database from M3.1 on.** `orre` (no subcommand) opens it with
   `Database::open` at startup, creating and migrating it as the CLI does (with the
   migration backup), and opens it again for each save. Like the MCP server, it holds no
   connection between reads and writes, and opens through `backup::gate` (decision record
   "one gate for opening the database"): during a restore it reads nothing and skips the
   save. The window state is a convenience: any
   failure (no data dir, a database Orre can't open, a bad value) leaves the window at its
   defaults with a warning on stderr (`ORRE_LOG`, as `orre mcp`), and never stops the
   app.
4. **When it's saved.** After the window has stayed put for 500ms following a move, resize
   or view change (one write per drag), on a background thread; and at once, on the main
   thread, when the window closes or the app quits.
5. **How it's restored.** A saved window whose title bar wouldn't show at least 40×40px
   on any display (a display unplugged since) opens centred at 1440×900 instead; one smaller
   than the 1280×800 minimum grows to it; with no displays reported, the saved bounds are
   used. An unknown view key (from a newer build) opens the default view, Transactions,
   the only screen built so far. Maximised and full-screen windows reopen so, with their
   restore bounds.
6. **Views not built yet show the shell and an empty state (user).** Every tab works.
   Until its roadmap item lands, a view shows its shell (the accounts panel on the money
   views, nothing on the others), with the Empty state component (`13L-0`) centred on the
   canvas, as `DESIGN.md` → "Review queues" centres an empty canvas: "{View} isn't built
   yet", a line saying Transactions works on sample data (since M3.3, that Transactions
   works), and the ⌘2 shortcut.

### Decision record: one gate for opening the database, and the Transactions edit pipeline (2026-11-15)

From an architecture review of the Transactions view. Neither change alters what the user
sees, except point 1's extra refusal.

1. **Every per-call open goes through `backup::gate::open`.** It checks the restore lock,
   runs the caller's opener (`Database::open` for the app, `Database::open_existing` and
   `reader()` for the MCP server), and checks the lock again. If a restore holds the lock
   at either check, the call is refused, and whatever was opened is closed first. The MCP
   server already checked before and after opening (decision record "M2: MCP server",
   F.1). The app's seven stores and the window state each had their own copy of the
   check, and checked only before opening, so a connection opened just as a restore took
   the lock could be used across the swap. They now share the server's check. The
   refusal still says "A backup is being restored; try again when it's done.", and a
   lock or open error keeps its own text.
2. **The lock is checked for an instant each time, not held for the call.** `restore`
   already waits up to `RELEASE_WAIT` for connections opened before it took the lock.
   Holding a shared lock for a whole call would make a restore's one-second `LOCK_WAIT`
   fail (`RestoreLocked`) on any call longer than that.
3. **The Transactions view writes through one edit pipeline** (`transactions::write`,
   `TransactionsView::write`/`query`). Each change declares a policy and one call to
   core on the database the pipeline opens. The policy covers where its error is said
   and how it is worded, the undo-bar offer, and the event for the shell. The pipeline
   owns everything else: the sample-data refusal, the reload and the event.
   `TransactionStore` keeps only `open`. Its 28 methods that each opened the database
   and forwarded to one core function are gone, so each call site names core's function
   itself. The single-row rule stays (decision record "splits, tags and notes in the
   inspector", point 1): a single-row edit shows no undo bar; links, changes to several
   rows, exclusions and transfer decisions do. Input checks that need the database
   (the user's note on a new manual row) now run after it is opened.
4. **The other stores keep their methods for now.** Accounts, Inbox, Checks, Rules,
   Settings and Import still forward most calls (49 of their 62 methods). They gain
   nothing from losing those methods until they have a pipeline like Transactions.
5. **Supersedes** "checks the restore lock first" in decision records "the app shell and
   window state", point 3, "accounts management", point 6, "Inbox", point 8, and "the
   import flow" (its stores).

### Decision record: the status bar reads the database (2026-11-17)

On an empty database the status bar still showed Paper's sample ("1 agent connected",
"Reconciled", "Imported 2 min ago from monzo-november.csv", "3 new", "GBP"). Supersedes
"AI access settings", point 8's "the rest of the status bar is still fixture data".
Decided by the user on 2026-11-17 (the recommended option for each). The model is
`shell::status::Status`, read from what the views already hold, so it follows their
reloads and adds no database reads.

1. **An item with nothing true to say is left out**, rather than given wording not
   drawn in Paper.
2. **"N agents connected" counts the enabled clients** (`AiData::enabled`) while AI
   access is on, and is left out when none is enabled. Orre can't see a live
   connection (each client starts `orre mcp` itself), so enabled is what "connected"
   means here. "AI access off" is unchanged.
3. **"Reconciled" needs at least one balance check** (`checks::Review::has_checks`);
   a failing check still replaces it.
4. **The import line is the newest committed import in an open account**, from the
   accounts panel's data: "Imported just now", "2 min ago", "3 hours ago", else "on
   4 Nov", then "from <file>". An undone import doesn't count. The shell reads the time
   every 30 seconds (`ShellView::keep_time`, only in the app) and after each import.
5. **"3 new" is left out** until something defines what it counts (the Inbox tab's
   badge already counts pending suggestions). **The currency shows when every open
   account shares one**, until a home currency exists (M4.6).
6. The sample data (`Counts::sample`) keeps Paper's artboard (`1CU-0`) for snapshots.

### Decision record: views that describe themselves stay out of the release binary (2026-11-18)

U.4, from the UI review (ROADMAP → "UI review — agent-readable views and keyboard
paths"). U.1–U.3 give views a `describe()`: their rows, selection, open dialog and the
actions available, as plain data, so tests assert on what the view shows. U.5 may later
let an agent drive a dev build through it. Both are a way to press a view's buttons
without a person at the window.

1. **`describe()` and anything that runs a view's actions from outside the window exist
   only in tests or a dev feature.** In U.1–U.3 they are `#[cfg(test)]`. If U.5 is built,
   its driver goes behind a Cargo feature that is off by default, never enabled in a
   release build, and not depended on by any other feature. A release binary contains
   neither.
2. **Never reachable from `orre mcp`.** No MCP tool, resource or prompt calls
   `describe()` or a view's actions, in any build. A tool that could press Approve would
   get round "MCP writes go to the inbox only" and "approval happens only in Orre's own
   UI or CLI" (`AGENTS.md` → Invariants).
3. **A dev driver works only against a scratch `ORRE_DATA_DIR`,** never the platform
   data dir, and listens only on a local socket.
4. **`describe()` shows what the view shows.** It is built from the same functions the
   view renders with, and untrusted text goes through the same sanitising
   (`envelope::shown`, the view's `text::plain`), so a test reading it sees what a
   person would, and no raw bank text or rationale escapes the envelope.
5. **L.7's in-GUI MCP falls under this record.** An MCP server inside the running GUI may
   read what the MCP surface already allows, and file suggestions to the inbox; it may
   not call `describe()` or run a view's actions.

### Decision record: performance at 100,000 transactions (2026-11-19)

R.14 (ROADMAP → "Architecture review", also L.30). Performance was unmeasured: the
Accounts and Inbox views read on the main thread, the Inbox ran a rolled-back approval
for each pending suggestion when it opened, `rule::overview` evaluated every rule over
every transaction on each reload, and every MCP call is a synced write to the access
log. The budget set for the work is **50 ms on the UI thread** (median, release build,
100,000 transactions). **(composed)** The numbers below are the measurement; the
decisions marked **(composed)** are the implementer's, for the user to confirm.

**How it was measured.** `crates/core/examples/ledger_bench.rs` builds a deterministic
synthetic ledger in a directory it is given (never the platform data dir; it refuses to
run without one) and times each core call a UI-thread or per-MCP-call path makes, as
the app's store makes it, `backup::gate::open` included: median of several runs after a
warm-up (a call over 10 s is timed once). The ledger: 100,606 transactions over five
years on 5 accounts and 2 pots (current, joint, savings, credit card, cash), the
default categories plus 4 of the user's, 100 rules, transfers (pot mirrors and
suggested pairs) and refund links found by the import commits, 400 pending suggestions
(200 from each of two clients: 240 categorisations, 100 rules, 20 each of merchant
names, transfer pairs and refund links) and 5,100 access-log rows. Everything is made
up, through core's public API (statements committed as imports). Machine: AMD Ryzen 9
3900XT (12 cores, `nproc` 24), 31 GiB, NVMe, ext4, Linux 6.18; rustc 1.99.1, the
default `release` profile (no LTO). To run it:

```sh
devenv shell -- cargo run --release -p orre-core --example ledger_bench -- <empty dir>
```

The first run builds the ledger (about 2 minutes) and measures (about 6 minutes, most
of it the Inbox); a later run in the same directory only measures. Delete the directory
afterwards.

| Call (thread it runs on) | Before | After |
|---|---|---|
| `Database::open` through the gate (UI) | 3.0 ms | 3.1 ms |
| Accounts view load (UI) | 118 ms | 117 ms |
| Inbox: counts, sources, list, subjects (UI) | 17 ms | 17 ms |
| Inbox: the previews of the 400 pending suggestions | 179 s (UI) | 22 s (background) |
| Inbox: one categorisation / transfer / refund-link preview | 33 ms | 2.2 ms |
| Inbox: one rule preview, alone | 1.18 s | 0.20 s |
| Inbox: one merchant-name preview, alone | 707 ms | 465 ms |
| Inbox: selecting a categorisation (`covered_by`) (UI) | 19.8 s | 0.4 ms |
| Rules view: `rule::overview` (UI) | 735 ms | 412 ms |
| Rules view load (UI) | 840 ms | 521 ms |
| Transactions register load (background) | 910 ms | 932 ms |
| `monthly_summary`, 12 months (MCP tool) | 101 ms | 102 ms |
| `net_worth_history`, 12 months (MCP tool) | 54 ms | 53 ms |
| `spending_by_category`, `budget_status`, one month (MCP tools) | 24 ms | 24 ms |
| One access-log record, `synchronous = FULL` (per MCP call) | 2.7 ms | 2.8 ms |
| Pending-file check before it (per MCP call) | 4.5 µs | 3.6 µs |
| Import: preview of 1,000 rows (background) | 14 ms | 13 ms |
| Import: commit of 1,000 rows, backup included (background) | 431 ms | 420 ms |
| Import: the Review step's "After import" (background) | 248 ms | 248 ms |

"Before" is the code at `db6bfec`; "after" is this change, on the same ledger. The
Inbox's 22 s is `InboxStore::load` for all 400; it is 100 rule previews of about 0.2 s
each and a merchant-name preview of about 0.45 s each, and the rest take 2 ms.

1. **The Inbox reads on a background thread.** `Inbox::reload` spawns the read, as the
   Transactions register does; what is shown stays until the new read replaces it, and
   a read asked for while one runs replaces it. Until the first read is back the table
   says nothing (not "Nothing to review"), as the Transactions table does. `Inbox::
   unloaded(store)` makes an Inbox with nothing read, and `InboxView::new` starts its
   first read. `Inbox::from_store` still reads on the calling thread: tests and the
   snapshot use it. **(composed)** The shell has to build its Inbox with `unloaded`,
   or the first read still blocks the window at start-up (`shell.rs` is not part of
   this change; see the hand-off). A decision (approve, reject) still runs on the UI
   thread (writes are not part of R.14) and then reloads in the background; the
   suggestion stays listed until the reload is back, and a second click on it is
   refused by core's recheck like any stale decision.
2. **One ledger read for a batch of previews.** A rule's preview read every transaction
   about nine times (its impact's plan, re-plan and scan, its rows' plan, scan and
   full read, its approval's plan and re-plan), each a read of every row with a
   correlated query per row. `inbox::preview::
   preview_all(db, ids)` reads the ledger once (`rule::History`, kept in `inbox::
   shared::Shared`), finds the rows a rule matches once per definition, and plans from
   that; `preview(db, id)` is the same code with its own read. `InboxStore::load`
   calls `preview_all`. The previews of all 400 suggestions on the benchmark ledger
   were compared before and after (`Debug` output, byte for byte): identical.
3. **Fixed in core, behaviour unchanged.**
   - `Ledger::transactions(ids)`: the `OR` between `transaction_id` and `target_id`
     made SQLite read the whole `ledger_line` view (20 ms at 100,000 rows) for every
     tally of a preview. A line with a `target_id` is on a link whose inflow is its
     transaction, so the query also asks for that, and finds the lines by index (1 ms).
     This took the fixed 33 ms off every preview and is the same rows.
   - `rule::commit_planned_in`: the impact and the Inbox's approval planned a rule
     against history, then `commit_apply_in` planned it again to check it wasn't stale,
     in the same transaction with nothing written between. The second plan is skipped
     there; `commit_apply` and the Rules view's apply still check.
   - A preview of a rule or merchant name no longer reads every transaction in full
     (`all_transactions`, 280 ms) to find its few matches: it matches on the columns
     rules read and reads only the matching rows in full.
   - `covered_by` approved every pending rule in turn (a dry run over the ledger each,
     20 s for a categorisation nothing covers). It now tries only the rules that set
     the category every item wants and match every item's transaction, which settling
     needs; the first rule that settles it is the same one.
   - `RuleSet::matching_and_outcome` gives the Rules table's two answers in one pass
     (it ran every rule's conditions twice per transaction), and `fold` skips its
     three passes for ASCII text, which is almost all bank text. `rule::overview`
     735 ms to 412 ms.
4. **Left, and why.**
   - **Accounts view load (117 ms) and Rules view load (521 ms)** are over the budget
     on the UI thread and need to move to a background read like the Inbox's: they are
     other lanes' views. Reading the transactions the rules see is 100 ms by itself, so
     `rule::overview` can't get under the budget by evaluating faster alone; a faster
     match (one Aho-Corasick pass per field for the `contains` patterns, the dependency
     is already there) would take it to roughly that floor. `account_tree` is 104 ms of
     the Accounts load (balances and reconciliation for 7 accounts); it was not
     profiled further.
   - **The Inbox still takes 22 s to open at 100,000 transactions with 400 pending
     suggestions,** on a background thread. A rule is previewed in 0.2 s because it is
     applied to history twice in a rolled-back transaction (its impact, its approval).
     Cutting that further means previewing only what is on screen or selected, which
     changes what the list shows before the previews are back (for example the "Approve
     all" count): not decided here. A client files at most 200 (`MAX_PENDING_PER_
     SOURCE`), and most pending suggestions are categorisations, which take 2 ms.
   - **The access log: no change.** One record is 2.8 ms with `synchronous = FULL`
     (worst of 101: 6 ms), against 24 to 100 ms for the cheapest and dearest read tool
     (`monthly_summary`), and it isn't on the UI thread. Durability settings and what
     is logged are as they were. Recommendation: keep `FULL` and the
     write where it is.
   - **Register load (0.93 s), the MCP reports (0.1 s) and the import (0.42 s to
     commit, its backup included)** are off the UI thread already. The import's backup
     copies the whole database on each commit, so it grows with the ledger.
5. **Not covered by this measurement.** Clicking through the real window, and frame
   times: only database reads were timed. The Inbox's `covered_by` still runs on the UI
   thread when a categorisation is selected (0.4 ms now).

### Decision record: Accounts and Rules read in the background (2026-11-19)

R.19 (ROADMAP → "Architecture review"), from "performance at 100,000 transactions",
point 4. At 100,606 transactions the Accounts load took 117 ms and the Rules load
521 ms on the UI thread, against the 50 ms budget, at start-up and after every write.
Both now read on a background thread, as the Inbox does. **(composed)** These are the
implementer's decisions, for the user to confirm; the read times themselves are as
measured in R.14 (nothing was re-measured: the same reads run, on another thread).

1. **Both read as the Inbox does.** `Accounts::unloaded(store)` and `Rules::unloaded
   (store)` make a view with nothing read; the shell builds both that way. The shell
   starts the accounts' first read when it is made (`ShellView::with_all`, through
   `Accounts::start_loading`); `RulesView::new` starts the Rules view's.
   `Accounts::reload` and `Rules::reload` read on `background_spawn` (the accounts'
   saved presets, which are files, in the same read). What is shown stays until the
   read lands, and a read asked for while one runs replaces it, so only the newest is
   applied. `from_store` still reads on the calling thread: tests and the snapshot use
   it. A read that fails keeps the last data and says why, as before.
2. **Until the first read lands, an empty view says nothing.** The accounts panel shows
   no "No accounts yet" (Net worth keeps its "—"), and the Rules canvas shows no empty
   state, as the Inbox and Transactions tables do. The status bar already leaves out
   what it has no data for.
3. **Writes stay on the UI thread.** Every change still runs where it is asked for, then
   asks for a read; between the two the view shows the last read. The writes were not
   part of R.14's measurement and a person's edits are single rows.
4. **Accounts between a write and its read.** A new account (and one reopened by the
   close bar's Undo) is selected at once, by ID, so Transactions filters by it at once;
   its row and inspector fill in when the read lands (the inspector counts an account
   as shown only once it is in the data, so its Name and Bank fields fill then, not
   blank). The read that lands was asked for after the write (an earlier one was
   replaced), so it has the account. An account that has gone when a read lands
   (closed, deleted, or after a restore) is let go then, and the views filtering by it
   are told (`AccountsEvent::Selected(None)`); a read that fails keeps the selection
   with the last data. A closed account's row stays until the read lands; a second
   close of it is refused by core.
5. **Rules between a write and its read: nothing the user does is lost or done
   twice.** The view's state assumed each write was read back at once; it now does at
   once only what an input depends on:
   - *A form isn't overwritten by an older read.* A read refills a rule's or category's
     form from what is stored only if the form wasn't changed since the read (or the
     first one it replaced) was asked for (`Rules::form_edits`); otherwise as before.
   - *The same save isn't made twice, and an older one isn't made again.* What was
     saved since the read was asked for is remembered (`Written`): a save of the same
     again is skipped, and a rule, merchant name or category opened again before the
     read lands fills its form from what was saved, not from the read data, which
     still holds the old value (leaving it would otherwise save the old value back).
   - *A move takes its place at once.* After `rule::move_to` the local order is changed
     as core changed it, so ⌥↑⌥↓ presses before the read lands add up and the bar's Undo
     returns the rule to where they began. A move takes the place of the rule shown at
     the target row, looked up in the database at the write, so a rule stored elsewhere
     and not read yet (an Inbox approval) doesn't put it one row off. What each rule
     decides follows with the read.
   - *A deleted row goes at once* (a rule, a name, a category; core refuses to delete a
     group with subcategories), so it can't be selected while the read is pending.
   - *A created rule or category is selected when the read lands.* "Create rule and
     change N", "Create rule only" and "Create category" close their preview or form at
     once (so they can't be clicked twice) and select the new item when it is read,
     unless another was selected in that section meanwhile; "No rules yet" doesn't show
     while the first rule waits. A new merchant name stays open with its fields as
     typed, as before. "Open rule" from Transactions on a rule the view hasn't read
     (before the first read, or one an Inbox approval made while the shell's read is
     in flight) selects that rule when the read lands, not nothing or the first row.
   - An edited rule's row shows its new conditions, and every rule's counts change, when
     the read lands; the inspector shows the edit at once.
6. **Left on the UI thread.** The Rules view's other reads (`refresh_preview`'s
   `rule::preview_apply`/`preview_draft` and `preview_rows`, `touching_rule` on
   selection, a change's items, `change::undo_preview`) and the Checks and Settings
   reads were not measured as loads and are unchanged. An apply preview over the whole
   ledger is likely over the budget at this size (one Inbox rule preview takes 0.2 s);
   not decided here.
7. **Not covered.** Clicking through the real window: the behaviour is checked by
   headless tests (`accounts/tests.rs`, `rules/tests.rs`, `write_tests.rs`,
   `reorder_tests.rs`, `state_tests.rs`) and the snapshots, whose Rules shots are
   unchanged.

### Decision record: right-click menus on the accounts panel (2026-11-20)

M3.P10, asked for by the user in the M3 exit check. The app had no context menu.
Drawn on M3 punch list · states (`IS2-0`, "Context menu · account"; `DESIGN.md` → "M3
punch list"). **No migration.**

1. **One context menu, built on `ui::menu::MenuList`** (`MenuList::context`, drawn at the
   pointer by `ui::menu::at_pointer`): the same keys as the other menus (↑↓, Enter,
   Escape, a click outside), no icons, hairlines between groups, an unavailable item faded
   with its reason. A right-click doesn't change the selection.
2. **Every item runs something the app already does** (`accounts::context_menu`, which
   builds the items per row kind, and `AccountsEvent`, which the shell carries out):
   Show transactions, New transaction…, Import into this account… (selects the account,
   then opens Import, which preselects it), Add balance… (a cash account's count), Edit
   in the inspector, Close account…, Delete account… (with `account::deletion`'s reason
   when refused); a pot's Show transactions and Add balance…; an asset's or liability's
   Add value…; a category's Show transactions and Open in Rules; Uncategorised's Show
   transactions. A cash account has no Import item, as its toolbar has none.
3. **A category's Rename…, New subcategory…, Hide and Delete…** open Rules →
   Categories on that category, as Open in Rules does: the categories editor can't yet be
   driven to start one of them from outside, and no new dialogs were drawn for it. The
   user chose to keep them as shortcuts to Rules (2026-11-21), not to rename or drop
   them.
   Delete… is unavailable ("In use") while the group holds transactions, which every
   group the panel lists does.

### Decision record: the dev driver (2026-11-21)

ROADMAP U.5, deferred on 2026-11-18 and built on 2026-11-21 at the user's request (the
user chose the scope: every view and dialog, with an `orre dev` subcommand as the
client). It follows the record "views that describe themselves stay out of the release
binary", points 1 to 5. The rest was composed during the build, for the user to confirm.

1. **It drives the real window through gpui-kit's test calls.** The feature turns on
   `gpui-kit/test-support`. Its observation is built on public GPUI APIs and works on a
   real window, as a spike showed before the build. A click is a pointer move, press and
   release at the control's centre, so it goes through hit-testing as a person's would.
   No second way to run a view's actions was added.
2. **An action is named by its accessible label, not by a new id.** The deferral
   asked for named commands. The 104 button actions in the descriptions already carry
   the label the window gives the control, so `run <label>` looks it up there, and
   `dev_driver::tests` checks that every enabled button each view describes resolves.
   Where a label repeats, each control has an id of its own for `click <id>`.
   Rejected: an element id on every `describe::Action`, which would have touched every
   call site and every test's expected values, and could drift from what's drawn.
3. **Dialogs are described at the shell** (`ShellView::describe`). A dialog with a
   `describe()` uses it. The others (Import, New transaction, Exclude, Link, the
   undo-import and delete confirmations) list their controls' role and label, never a
   field's value, which can hold raw bank text. Focus is the innermost observed control
   whose focus scope holds it. Hover isn't described.
4. **Guarded at three points.** In `Cargo.toml`, the feature is off by default and no
   other feature names it. In `src/dev_driver.rs`, a `compile_error!` stops a release
   build. In `dev_driver::guard`, the driver refuses to start when `ORRE_DATA_DIR` is
   unset or is the platform data dir (also through symlinks and `..`), and the window
   opens without it. `crates/core/tests/invariants.rs`
   (`the_dev_driver_stays_out_of_release_and_mcp`, `describe_modules_are_test_only`)
   checks the first two points and the gates.
5. **The socket is the user's alone** (mode 0600, in the scratch dir). A stale one is
   replaced and a live one refused. One command per connection, answered from the UI
   thread, which polls every 50 ms (dev builds only).
6. **`orre dev` needs no terminal.** "Approval needs the user's terminal" (2026-11-18)
   keeps an AI client from acting as the user on the real database. The driver can
   press Approve, but only in a dev build on a scratch dir, and checking the app is what
   it's for.

### Decision record: live updates (2026-11-21)

ROADMAP LU.1, decided before any code. Today a view reloads after the app's own changes
and when it is shown (`ShellView::show`), so a change from another process is seen only
when the user moves around. **Nothing here has been confirmed by the user beyond the
roadmap text.** The stamp as point 1 first states it, points 3 and 4 and the poll of
once a second come from the roadmap. The rest is **(composed)** for the user to
confirm: the platform identity (1), the topics and their fingerprints (2), rejecting
file-notification crates (5), the intervals and the handling of the app's own writes
(6), and where it runs and how a reload behaves (7 and 8).

1. **Detection is by the files.** `orre_core::watch` keeps a stamp and reads it once per
   tick: for the database and for its `-wal` file (`db::sidecar(path, "-wal")`), the
   size and modified time, and for the database alone its identity. A file that isn't
   there is a valid state of the stamp, not an error: SQLite removes `-wal` when the
   last connection closes, and every call here is a short-lived connection, so the
   file comes and goes. The identity is the device and inode on Unix
   (`std::os::unix::fs::MetadataExt`). Stable std gives no file index on Windows, so
   there it falls back to the creation time (`Metadata::created`); Windows is L.10.
   Every commit from any process touches `-wal`, or the database file when the last
   connection checkpoints, so every change moves the stamp, as finely as the file
   system's modification times go: two commits in one tick that leave both files'
   sizes as they were read as one stamp. With nanosecond times (ext4, APFS, NTFS)
   that doesn't happen in practice; on a coarse file system (exFAT, some network
   ones) the second commit is seen at the next change. The read opens nothing, so
   "no connection is held" ("Opening the database") stands.
   - **An unchanged stamp ends the check.**
   - **A changed stamp** leads to one open through `backup::gate::open`, with
     `Database::open_existing` and its `reader()`, as the MCP server opens: it never
     creates or migrates, and the shell migrated at start-up. The reader runs the
     queries of point 2, compares the results with the last ones and returns the
     topics that differ. The watcher then reads the stamp again. It keeps that one
     only when it is the stamp from before the open; otherwise it keeps the one from
     before, so the next tick opens and compares again. That covers both a write
     another process made while the check read (keeping the later stamp would absorb
     it unseen) and the check's own close, which can checkpoint and remove `-wal`.
     A check whose open leaves the files as they were settles it, so the cost is one
     extra open after a change.
   - **A new database identity** (a restore renames a new file over the database,
     `backup::restore`) reports every topic.
   - **A held restore lock** (`backup::restoring`, which the gate checks) skips the
     check and keeps the old stamp, so the next tick catches up.
   - **A checkpoint with no data change** moves the stamp, runs the fingerprints and
     reports nothing.
2. **Four topics, one fingerprint each.** **(composed)** A fingerprint is the result of
   the queries below, kept in memory and compared for equality (hashed when long, and
   never logged, so "no transaction content in logs" holds). The first read after the
   watcher starts is its baseline and reports nothing; the shell takes it before the
   views' first reads start, so a write landing between the two is reported. Each must be cheap at 100,000
   transactions and must not read a table that changes on every MCP call or window
   move.
   - **Inbox** (the Inbox view and its tab badge): the newest suggestion and the
     pending count, which catch a filing, an approval and a rejection.
     `SELECT coalesce(max(id), 0), (SELECT count(*) FROM suggestion WHERE status =
     'pending') FROM suggestion`. `id` is the primary key; `suggestion_status
     (status, created_at)` covers the count. Also `count(*)` and `max(id)` of `report`:
     an MCP client's saved report, and one deleted below the newest.
   - **Ledger** (Transactions, Accounts and Checks):
     - `SELECT coalesce(max(id), 0) FROM "transaction"`, the primary key: an import
       adds rows. And `count(*)` of it, the one scan of a large table here (an index,
       milliseconds at 100,000 rows, only on a changed stamp): deleting a manual row
       below the newest moves no id and records no batch.
     - `SELECT count(*), coalesce(max(id), 0), coalesce(sum(status = 'undone'), 0) FROM
       import_batch`, and the same on `change_batch`: an import, `undo-import`, and
       every bulk change (`orre categorize` records a `bulk_edit` batch, a rule apply a
       `rule_apply` one and a duplicate merge a `duplicate_merge` one; an Inbox
       approval of a categorisation is a batch too, "Decision record: Inbox";
       `change::undo` marks a batch `undone`). Both tables hold one row per import or change, so a scan is small and
       needs no index.
     - `SELECT count(*), coalesce(max(id), 0) FROM balance_snapshot`: an imported or
       entered balance, and a removed one. `count(*)` reads the smallest index
       (`balance_snapshot_batch`).
     - `SELECT count(*), coalesce(max(id), 0), (SELECT count(*) FROM duplicate_candidate
       WHERE status = 'pending') FROM duplicate_candidate`: the Checks queue
       (`duplicate_candidate_status` covers the pending count).
     - `count(*)`, `max(id)` and the confirmed and rejected counts of
       `transaction_link`: deciding a suggested link moves no id and no count.
     - The same for `transfer_group`: an Inbox approval of a transfer pair confirms
       or adds a group and records no change batch.
     - Every column of every `account` row, each through `quote()`, joined with
       `char(31)`, the rows ordered by `id` and joined with `char(30)` in one
       `group_concat`: an account opened, renamed, closed or locked by `orre accounts`
       or a restore. The table is a few dozen rows.
   - **Rules**: the same `group_concat` shape over `rule` (`rule_priority`), `rule_action`,
     `merchant_alias` and `category`, every column but `created_at`. These tables hold
     tens to hundreds of rows. The column lists are in `orre_core::watch`, and a test
     there compares each with the live schema, so a migration that adds a column
     fails it until someone puts the column in a list or in its left-out list.
   - **Settings** (Settings → AI clients, and the status bar's agent count): the same
     shape over `mcp_client` (`name`, `enabled`, `access_level`, `status`; not
     `last_seen_at`, which every MCP call writes) and over `setting` (`key`, `value`)
     without the key `app.window`, which the window's own saves write on every move.
   - **What catches an edit to an existing row.** `transaction` has no `updated_at`, so
     a category, name or note edited in place moves no fingerprint. Most writers
     outside the app that edit existing transactions do it through a `change_batch`
     (`orre categorize`, an Inbox approval of a categorisation, a rule apply, a
     duplicate merge) or an import batch, which the Ledger fingerprint reads. Two
     Inbox approvals don't: a transfer pair writes `transfer_group` (in the Ledger
     fingerprint above), and a merchant alias recomputes `merchant_display` on the
     rows it matches, which moves only the Rules fingerprint (`merchant_alias`). So a
     Rules change has to reload the views that show merchant names too (LU.3). A
     manual row deleted below the newest moves the `count(*)`. The app's own
     single-row edits are read back by the app itself. A writer that
     edits a transaction in place outside a batch has to move a fingerprint, or its
     change isn't seen until the user moves around. **No migration is proposed.**
     LU.6 says if its measurement calls for an index.
   - **What no topic sees:** budgets, schedules, exchange rates, tags' links to
     transactions, and the MCP access log. Each is one more query when a view needs
     it (LU.3).
   - **Which views reload on which topic** is LU.3's. The Rules view's counts are of
     the ledger, so LU.3 decides whether a Ledger change also reloads Rules; a Rules
     change reloads Transactions (merchant names, above).
3. **Rejected: a held connection polling `PRAGMA data_version`.** Restore renames a new
   file over the database, so a held connection keeps reading the old file and never
   learns of the new one. On Windows it would also block that rename (L.10). It would
   also end "no connection is held", which is what lets a restore swap the file.
4. **Rejected: a change-journal table filled by triggers.** A trigger on `suggestion`
   would write a second table on every MCP insert, against "MCP writes go to the inbox
   only": the trigger would be the write. A trigger on `transaction` would cost a write
   per imported row. It is also a migration, a one-way door, to learn what a `stat`
   and a few indexed reads already show.
5. **Rejected: an OS file-notification crate (`notify`, inotify).** **(composed)** It is
   a new dependency ("keep dependencies few; justify new ones", `AGENTS.md`), with a
   different back end per platform (inotify, FSEvents, ReadDirectoryChangesW) and
   quirks on `-wal` and `-shm` files that come and go. A restore's rename, a watch
   limit or a dropped event would still need the stamp to catch up, so the poll would
   stay and the crate would add a second path to test. The stamp is two `stat` calls
   a second.
6. **When it polls.** **(composed)** Every second while the window is active, so a
   change shows within about two seconds (the exit criterion: a tick to see the stamp
   move, then one open and read, well under a second); every five seconds while it
   isn't, with a check at once when it becomes active again. The timer doesn't poll
   while a restore runs in the app; when the restore is done, the watcher is rebased on
   every topic, which also takes the restored file's identity, and then the views the
   restore reloads are the reads.
   - **The app's own writes.** After an app write the stamp moves and the watcher would
     report the topic the app is already reloading, which at 100,000 transactions is
     not cheap (the Inbox's first read takes 22 s, "performance at 100,000
     transactions"). So before the app starts a reload of a topic it rebases that
     topic: the watcher reads its fingerprint again and keeps it. A change from another
     process that lands before the rebase is in the reload's read, which comes after.
     One that lands after the rebase is reported. So a reload can be redundant, and
     none is missed. The window's own saves (`app.window`) are out of the Settings
     fingerprint, so they cost one stamp tick and no reload.
7. **Where it runs.** **(composed)** `orre_core::watch`, with no GPUI, so it is tested
   like the rest of core (LU.2). A timer on the shell, like `ShellView::keep_time`
   (LU.3), runs the check on a background thread and hands the changed topics to the UI
   thread. Reading the stamp and the fingerprints never runs on the UI thread, which
   stays within the 50 ms budget. The timer starts only in the app: tests and the
   snapshot example don't run it, as they don't run `keep_time`.
8. **A reload nobody asked for keeps the user's place** (LU.4). **(composed)** It must
   not lose a selection, a scroll position, filters, a search, an inline edit, or an
   open dialog or form. A row that has gone (undone, decided elsewhere, or an approval
   in flight) leaves the list without the selection jumping to another row: what
   happens to the selection is each view's rule. Every view lets a row that has gone
   go and selects nothing in its place, except Settings, whose lists always select
   their first row (`select_defaults`) (Accounts already did, point 4 of "Accounts and
   Rules read in the background"). For the Inbox that matters most: `A` approves the
   selected suggestion with no confirmation, so a reload must never select one the
   user didn't pick. The user's own decision still moves on to the next suggestion,
   before the read.
   Transactions keeps the item at the top of the list by identity, so rows arriving
   above it don't move the view. Rules keeps a form that holds valid unsaved typing,
   whenever the typing was (`Rules::form_edits` covered only typing after the read
   was asked for). Settings reloads as many access-log pages as were loaded, and at
   most one page more when calls logged since have pushed the selected call down; past
   that the selection falls back to the newest call rather than reading page after
   page on the UI thread. Each view's test on
   its `describe()` shows it.
9. **Cost: to be measured (LU.6).** An idle tick (two `stat` calls) and a fingerprint
   check at 100,000 transactions, on `ledger_bench` or a new example. The numbers go
   here, with the machine, when LU.6 is done. Not measured yet.

The user chose nothing in this record beyond the roadmap text; every other point was
composed for the user to confirm.
