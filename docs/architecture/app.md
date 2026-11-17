# Coffer — Architecture: The app

The app shell, window state, the status bar, and how the app opens the database and applies edits.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

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
   `WindowState` (`coffer_core::setting`): mode (windowed, maximised, full screen), the
   restore bounds in whole logical pixels, and the open view's key. Core checks the range
   (sides 1–32768, origins within ±32768, a view key of at most 32 `[a-z_]`) and refuses an
   out-of-range write; a stored value that doesn't parse reads as an error and the app uses
   its defaults. It isn't exported and no MCP tool reads it. Restoring a backup rolls it
   back with everything else, which is harmless. The rejected option was a separate
   `window.json` in the data dir, which would have kept the GUI off the database until M3.2.
3. **So the GUI opens the database from M3.1 on.** `coffer` (no subcommand) opens it with
   `Database::open` at startup, creating and migrating it as the CLI does (with the
   migration backup), and opens it again for each save. Like the MCP server, it holds no
   connection between reads and writes, and opens through `backup::gate` (decision record
   "one gate for opening the database"): during a restore it reads nothing and skips the
   save. The window state is a convenience: any
   failure (no data dir, a database Coffer can't open, a bad value) leaves the window at its
   defaults with a warning on stderr (`COFFER_LOG`, as `coffer mcp`), and never stops the
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
   access is on, and is left out when none is enabled. Coffer can't see a live
   connection (each client starts `coffer mcp` itself), so enabled is what "connected"
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
