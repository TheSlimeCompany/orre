# Coffer — Architecture: MCP

The MCP server: tools by access level, the gate, the access log, untrusted text, and safe report rendering.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## MCP surface

`coffer mcp --client <name>` (`crates/mcp`, rmcp over stdio). Setup for Claude Code,
Claude Desktop and Codex: [`mcp-setup.md`](../mcp-setup.md). Decisions: "Decision record:
M2: MCP server (2026-11-13)".

**Tools by level** (a client sees and can call the tools at its level and below; a call
above it is refused `refused_level`):

| Level | Tools |
|---|---|
| `aggregate` | `list_categories`, `list_reports`, `monthly_summary(from, to)`, `spending_by_category(period)`, `budget_status(month)`, `net_worth_history(from, to)`, `save_report(period, title, body_markdown)` |
| `pseudonymous` | also `list_accounts`, `search_transactions(filters, limit)`, `uncategorized_transactions(limit)`, `recurring_payments()` |
| `standard` | also `list_rules`, `merchant_history(merchant)`, `propose_categorizations`, `propose_rule`, `propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link` |

Reads go through `ledger_line`, so split purchases count per category, and a confirmed
refund nets against its purchase's category. Amounts are `{minor, currency, home}` with
integer minor units; `home` stays `null` until home-currency conversion (M4.6), and totals
are listed per currency, never added across currencies. Every tool takes an optional
`purpose` string, which is logged and never interpreted. Added with M4.8:
`cash_flow_forecast`.

**What each level withholds**:

- **Merchants.** At `pseudonymous` every merchant is a token, `M-` plus 12 hex digits of
  HMAC-SHA256 of the trimmed, lower-cased display name under the per-database
  `mcp.pseudonym_key`: stable across calls, different between databases. Without a key a
  `pseudonymous` call is refused.
- **Descriptions.** Bank text appears only at `standard`, only when asked for
  (`include_description`), and only as `description_untrusted`. Below `standard` the
  field holds a fixed `[hidden: …]` text: bank text names the merchant and would undo the
  tokens.
- **Text search.** `search_transactions`' free-text `query` and its tag filter need
  `standard`: below it, counting the rows a text matches would reveal what lies behind a
  token. Dates, accounts, categories and amounts filter at `pseudonymous`.
- **Account names.** At `aggregate`, `net_worth_history` gives each account a `label`
  (its kind and number, e.g. `pot 2`) and no name; names appear from `pseudonymous` up,
  since a name can name a person.
- **Names in bank text.** A merchant or description that is a person's name reaches a
  client only at `standard`, which is full access by design (user, 2026-11-13).
- **Never, at any level:** account numbers, sort codes and IBANs (a merchant holding one
  is `null`, a description holding one is `[withheld: account details]`), notes (the
  bank's and the user's), share labels, private tags, merchant addresses and
  attachments. Public tags only at `standard`.

**Inbox writes** (never modify transactions/rules directly): `propose_categorizations`,
`propose_rule` (actions `set_category`, `split_template`, `set_merchant`, `add_tag` only),
`propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link`, `save_report`.
Payloads are validated against the database when filed and again when approved; an
identical pending suggestion returns the existing ID (`already_pending`). Caps: 200
pending suggestions and 500 saved reports per client; only the user deletes reports
(`coffer inbox delete-report`), never a tool. The source is always the calling client,
never an argument.

**Prompts**: `monthly_review(month)` — patterns, anomalies, subscription creep, budget
variance and suggestions, ending with `save_report`. Its steps name only tools registered
at the client's level; `month` must parse as `YYYY-MM` before it enters the text.

**Access control** (M2.11). On every call the server reads the kill switch
(`mcp.enabled`) and the client's row: switched off → `refused_off`; unknown (recorded as
`requested`), requested, ignored or revoked → `refused_not_enabled`; a tool above the
level → `refused_level`. A lower level applies to the next call, and the server sends
`notifications/tools/list_changed` (also from a 5-second poll, so an idle client hears).
Only the CLI or app enables a client or changes a level.

**Access log** (M2.12): tools write only `suggestion` and `report`. Separately, the
server appends exactly one `mcp_access_log` row per tool call as its own audit record,
whatever the outcome; if it can't be written, the call fails and no data goes out. The
AI's inputs to it are the untrusted `purpose` (stored without invisible characters) and
the arguments, redacted by the tool's schema: numbers, booleans, dates and schema enum
values are kept, any other string becomes `{"length":N,"type":"string"}`, undeclared keys
are only counted (`"_undeclared": N`). `result_hash` is SHA-256 of the result's canonical
JSON (compact, keys sorted). `coffer ai-access log` shows the log. A call refused because
the database can't be opened (a restore holds its lock: `refused_restoring`; migrations
pending or the lock unreadable: `refused_unavailable`) can't be written there: it waits
in `<database>-access-pending` and goes into the log, at its own time, the next time the
database is opened to write (decision record "M2: MCP server", F.8). The one exception,
returning no data: getting a prompt.

**Restores.** The server opens the database for each call and keeps no connection
between calls. `coffer restore` holds the file lock on `<database>-restoring` while it
swaps the files; the server checks it before and after opening and refuses the call while
it's held, so the next call reads the restored file; the refused calls go into the
restored database's log, and so do the rows the replaced database logged after the
backup was taken (F.9). The lock goes with the restore's process, so a crashed restore
blocks nothing (decision record "M2: MCP server", F.1).

Transaction descriptions are untrusted input (prompt-injection surface); inbox-only
writes are the containment. In addition:

- **Text envelope** (`coffer_core::envelope`, M2.13). `description_untrusted` is
  sanitised in four steps: control, zero-width, bidi, tag and BOM characters are
  stripped and fullwidth letters folded to ASCII; the whole field is withheld, as
  `[withheld: N characters that read like instructions]`, when it matches a documented
  pattern list (role markers such as `system:`, "ignore previous/above…", a URL with a
  scheme, a `www.` host, a markdown or HTML link, a long base64 run); bare domains are
  made inert (`NETFLIX[.]COM`); the length is capped. The count goes to
  `mcp_access_log.withheld_count`, and the app's "Withheld from AI" filter runs the same
  check (`envelope::reads_like_instructions`). A display merchant that reads like
  instructions is withheld at `standard` too.
- **Impact preview.** The inbox shows what approving each suggestion would change (e.g. how
  many past transactions a rule would recategorise), so an overly broad rule or a fake
  transfer pair planted by injected text is visible before approval.
- **Reports.** Saved reports render as a markdown subset: no HTML, no images, links shown as
  plain text, so a report can't trigger a network request. `list_reports` marks them
  `ai_authored`, since stored AI text is untrusted when read back, and strips control,
  zero-width and bidi characters from the period, title and body (decision record,
  2026-11-10); the stored text is unchanged. `coffer inbox` shows them the same way.
- **Client setup.** [`mcp-setup.md`](../mcp-setup.md) recommends a client profile with
  Coffer as the only data source and no web or fetch tools.

## Decision records

Oldest first.

### Decision record: safe report rendering and withheld text (2026-11-10)

The open questions from the D6.11 designs (`DESIGN.md` → "Safe report rendering"),
decided by the user on the designer's recommendation:

1. **Emphasis renders as weight 500.** `*emphasis*` in a saved review is 500 in
   `text-strong`. Coffer doesn't bundle an italic face for reviews alone.
2. **The withheld count lives on the access log.** `mcp_access_log` gains
   `withheld_count` (a new migration): how many descriptions that call withheld. The
   Reviews inspector sums it over a review's calls. The app's "Withheld from AI" filter
   re-runs the same deterministic check over raw descriptions, so the log never records
   which transactions were withheld, in keeping with "never stores returned data".
3. **The marker is a string with a count.** A withheld `description_untrusted` reads
   `[withheld: N characters that read like instructions]`. The field stays a string, and
   a client can't mistake the marker for bank text.
4. **Reports read back are stripped and marked, not withheld.** `list_reports` returns
   `body_markdown` with control, zero-width and bidi characters removed and
   `ai_authored: true`. Nothing inside a report is withheld: the marker covers the whole
   body. The stored body is never changed; the app strips the same characters when it
   renders a review and counts them.

### Decision record: M2: MCP server (2026-11-13)

M2 was built in four parallel blocks (B access, log and text safety; C read tools and
reporting; D recurring payments and schedules; E the inbox) on Wave 0's spine, then
integrated. Marked **(user)**: decided by the user on 2026-11-13, either directly or by
reviewing each implementation default one by one after the real-data check (all kept
as built). Everything else follows an earlier decision record or invariant.

**A. The user's decisions.**

1. **Names at `standard` are accepted (user).** `standard` is full access by design: a
   payee who is a person can appear under their name as `merchant`, and in
   `description_untrusted`. Below `standard` a name never appears (tested with a row
   whose bank text is only a name, `crates/mcp/tests/person_names.rs`).
2. **No account names at `aggregate` (user).** `net_worth_history` gives each account a
   `label`, its kind and its number among accounts of that kind by ID (`current 1`,
   `pot 2`), and `name: null`; names appear from `pseudonymous` up.
3. **No text search below `standard` (user).** `search_transactions` refuses `query` (and
   the tag filter, the other user-text match) below `standard` with a clear error; date,
   account, category and amount filters stay.
4. **Descriptions are hidden below `standard` (user).** `Call::untrusted` returns
   `[hidden: descriptions name merchants, so they aren't shared at this access level]`
   below `standard`, since bank text names the merchant a token hides.
5. **Block D's schedule schema (user):** `start_date` instead of `next_date`, the new
   `recurring_dismissal` table, and the correction to the lock claim (D7 below).
6. **Wave 1's defaults are accepted (user):** CLI names `coffer ai-access`, `coffer
   budgets`, `coffer schedules`, `coffer inbox`; pseudonym tokens are HMAC-SHA256; the
   tool levels as built (the table in "MCP surface"); `Amount.home` stays `null` through
   M2.

**B. Access control, the access log and the text envelope (M2.11–M2.13).**

1. The kill switch is on unless set off; MCP stays off until a client is enabled
   (user).
2. The pseudonym key is 32 bytes from SQLite's `randomblob` (no RNG crate), created by
   `coffer ai-access enable`, never by the server; without one a `pseudonymous` call is
   refused with a message saying to enable the client again (user).
3. Tokens are `M-` plus 12 hex digits (48 bits: about 1.8 × 10⁻⁷ chance of any collision
   at 10,000 merchants), of the trimmed, lower-cased display name (user).
4. Client lifecycle: enabling an enabled client keeps its level; `revoke` sets the level
   back to `aggregate`; `ignore` also revokes; a name can be enabled before the server has
   seen it (user).
5. An unknown name is recorded as `requested` on any request, even with the kill switch
   off, so the user sees who asked (user).
6. A 5-second poll sends `tools/list_changed` to an idle client; it only reads (user).
7. Redaction: numbers, booleans, dates and schema enum values are stored as themselves;
   any other string, digits-only included, as `{"length":N,"type":"string"}`; undeclared
   keys only counted, `"_undeclared": N` (user).
8. The result hash is SHA-256 of compact JSON with keys sorted at every level (user).
9. The envelope withholds the whole field on any pattern match; URLs with a scheme, `www.`
   hosts and markdown/HTML links are withheld and bare domains made inert
   (`NETFLIX[.]COM`), which settles "made inert" against "withheld"; fullwidth letters
   are folded, full NFKC and homoglyphs are out of scope; `Call::merchant` withholds a
   display name that reads like instructions at `standard` too (user).
10. `coffer ai-access log` shows the redacted arguments (cut at 120 characters) and the
    cleaned purpose (cut at 60), never the result hash (user).
11. Retention: failing to delete old log rows is a warning, and the server starts anyway
    (user).

**C. Read tools and reporting (M2.2, M2.14 reads).**

1. Money counts by its category's kind: money in under a spending category (refunds
   included) lowers spending and isn't income; transfer categories count as neither;
   uncategorised money counts by the sign of the user's share; a claimed-back purchase
   counts as nothing (user).
2. Spending, budgets and the row tools' `my_amount` use the user's share; balances and
   net worth are gross (user).
3. Net worth: liabilities always lower the total, even entered positive; a closed account
   drops out of months ending after its closing day; an account with no stated balance is
   summed from zero and flagged `unanchored` (user).
4. Budgets: a month's budget overrides the standing one; a budget counts spending only in
   its own currency and lists the rest beside it; rollover follows the group (reported,
   applied from M4.9); budgets only on spending categories; `coffer budgets set` defaults
   to the one currency all accounts share and otherwise needs `--currency` (user).
5. Row limits: 50 by default, at most 200, with `truncated` (user).
6. Account-details check (`reporting::rows::has_account_identifier`): a merchant holding a
   sort code, 8+ digit run or IBAN becomes `null`, a description holding one
   `[withheld: account details]`; dates, amounts and short card references like `*1234`
   aren't caught; `withheld_count` still counts only text withheld as instructions
   (user). Since 2026-11-16 the check runs inside `Call::merchant`
   (`crates/mcp/src/registry.rs`), the only maker of a `MerchantLabel`, so it covers
   every merchant the server returns (transaction rows, `recurring_payments`' series
   and schedules, `list_rules`' `set_merchant`) at every level, before pseudonymising,
   on the name as stored and with invisible characters stripped. `Call::merchant`
   returns `Option<MerchantLabel>`, and every merchant field in a tool's output is
   nullable. Descriptions are still checked in the row tools. A standing order's
   display name can be its whole bank text, so before this a recurring series could
   carry an account number at `standard`.
7. `list_rules` shows public tags plus a count of private ones, `set_note` as the action
   only, the share action as its percentage only (user).
8. `list_reports` is newest first, every report `ai_authored: true`, text stripped of
   invisible characters (decision record 2026-11-10, point 4). Since 2026-11-18 it lists only the
   calling client's own reports saved at its current level or below (decision record "a
   client reads only its own reports").
9. A refund of a split purchase comes back as one row per line category, as the ledger
   nets it.

**D. Recurring payments and schedules (M2.6, M2.10).**

1. **`start_date`, not `next_date` (user).** A schedule stores its first tracked
   occurrence; occurrence *k* is `Cadence::nth(start, k)`, counted from the start (a
   schedule on the 31st falls on 28/29 February and 31 March again). Stored matches and
   skips name occurrence dates, so the dates must be fixed.
2. **`recurring_dismissal(series_key, dismissed_at)` (user)** records "Not recurring".
3. `schedule_match` records who matched (`auto`/`user`) and when; a transaction fulfils
   at most one occurrence of any schedule; triggers keep a match on the schedule's account
   and an occurrence never both matched and skipped.
4. Series keys are an unkeyed FNV-1a hash of account, merchant, direction, currency,
   cadence and position, stable as payments arrive; kept out of MCP output (a guessed
   merchant could be checked against one). The CLI shows them (user).
5. Matching runs at import (that import's rows, active schedules) and when a schedule is
   created or confirmed (the account's earlier rows after its lock date). The window for
   occurrence *k* is `[nth(k) − tolerance_days, nth(k+1) − tolerance_days)`; it compares
   the whole amount, the normalised merchant and the sign; excluded rows and transfers
   never match automatically; the lower schedule ID wins a tie; `update` and `resume`
   match nothing, so an unmatched payment stays unmatched (user).
6. "Imports cover the date" means the account's latest imported or mirrored transaction,
   excluded ones included; manual rows don't count. `today` is passed in, read from
   SQLite's clock (user).
7. **Lock dates apply to the occurrence's date (user: the correction).** Skip, unskip,
   match and unmatch are refused on or before the lock, and deleting a schedule with a
   match or skip there. A payment arriving late but within its window can fulfil an
   occurrence dated on or before the lock, so an import's matches *can* touch a locked
   period: the earlier assumption that they never do was wrong.
8. Ended is final; a paused schedule can still be skipped, matched and edited; an ended
   one refuses skips, matches and edits, but its matches and skips can be removed and it
   can be deleted (user).
9. A new schedule's tolerance: days per cadence (weekly 1, fortnightly and four-weekly 2,
   monthly 3, quarterly 5, yearly 7), at most under half the shortest gap; amount: the
   series' spread or a tenth of its typical amount, whichever is larger (user).
10. Detection groups by account, normalised merchant, direction and currency; chains
    each cadence within the day tolerance, skipping at most one payment (never two in a
    row, at most one per four); fixed amounts within 2% (each price held twice, at most
    `max(1, n/4)` changes, typical = latest); otherwise variable within a factor of two of
    the median (one extra payment needed, typical = median of the last 6); refuses a
    series with similar amounts around it or one chance would give 1/32 of the time;
    reports only the current run of a restarted payment. Full rules in `recurring.rs`
    (user).
11. Known limits: two series from one merchant at the same amount aren't reported;
    detection runs on every `recurring_payments` call and isn't benchmarked on large
    ledgers; on synthetic noise about 1 ledger in 300 gives a false series.

**E. The inbox (M2.3, M2.7, M2.14 writes).**

1. At most 200 pending suggestions per source; a duplicate doesn't count (user).
2. Caps: 1–100 categorisation items, rationale 1,000 characters, display name 100 (no
   control characters), rejection reason 500 and not blank, report title 200, period 32,
   body 100,000, title and period not blank (user).
3. One pending suggestion per `(kind, canonical payload)` from any source; filing an
   identical one returns the existing ID and the first proposer stays the source. The
   canonical form sorts items, orders actions and writes out defaults (user).
4. An agent's rule may only use `set_category`, `split_template`, `set_merchant` and
   `add_tag`; `exclude`, `set_note` and `my_share` are refused (user).
5. Each approval is one `BEGIN IMMEDIATE` transaction that re-checks the payload, applies
   it and marks it approved. A categorisation is one `bulk_edit` batch with source
   `ai_approved`, skips targets that take no category of its own (decision record "what
   a transaction can take"), and overwrites user-set categories (the user approved those rows
   explicitly, and the preview counts them); setting a split line's category rewrites
   that transaction's lines. A transfer pair confirms detection's own group or creates a
   confirmed one, refused if either side is linked. A link is confirmed at the payload's
   amount or what both sides can still take (user).
6. No approval meets a lock today: `reconciled_through` guards only exclusions, which no
   suggestion makes; `ApproveError::Locked` is a safeguard.
7. Settling is narrow: a pending categorisation is settled by an approved rule only when
   every item names a transaction the rule's batch gave that category; split-line items
   and partial overlaps stay pending (user).
8. Coffer's own rule suggestions: after 3 or more unsplit rows with one `merchant_display`
   get the same category from the user (counted from the current state, so undoing an
   edit lowers the count), Coffer files `merchant is <cleaned merchant>` →
   `set_category`, only when every counted row's merchant folds to one text, with a fixed
   rationale and no merchant text. It files nothing when an identical suggestion is
   pending or was ever decided (a rejection is permanent), a stored rule has exactly this
   definition, or the source is at its cap (user).
9. "Approved" means "was applied": undoing its batch or deleting its rule leaves it
   `approved`.

**F. Integration.**

1. **Restores and running servers.** The server opens the database per call and keeps no
   connection between calls, so an idle server never blocks `coffer restore` (which
   refuses a database another connection holds). From the checked backup until the
   restored database is open, `restore` holds the operating system's exclusive file lock
   (`File::try_lock`, std, no dependency) on `<database>-restoring`
   (`backup::restore_lock_path`), and waits up to 5 seconds for a running call to close its
   connection. The lock, not the file, means "restoring": the file stays, empty between
   restores, and is never deleted, so no process can lock a file that has left the
   directory while another locks a new one under the same name. The server checks with a
   momentary shared lock before and after opening and refuses while the lock is held, with
   "Coffer is restoring a backup"; those refusals are logged afterwards, through the
   pending file (F.8). A second restore can't take the lock and is refused (after
   retrying for a second, past the server's momentary checks). A check made before the
   restore holds the lock sees no restore, which is sound because `restore` changes
   nothing until it holds the lock and then waits for earlier connections; that's why
   there is no grace period and no create-then-rename (which `std`'s replacing rename
   can't do safely on Windows). **A crashed or killed restore** loses its lock with its
   process, so it blocks nothing: the database is complete at every step (untouched before
   the atomic rename, the synced backup after it, a migration all or nothing), and
   `open_existing` refuses one left at an older schema with its usual "run a `coffer`
   command" message. The restore writes a marker (start time, backup) into the file while
   it holds the lock and empties it at the end, so `coffer restore` finds a stopped one,
   warns, and goes ahead, backing the current database up first as always. A lock file
   the server can't open or lock is refused with a message naming the file. File locks are
   `flock`/`LockFileEx`; on a network file system they may be missing (restore refuses,
   MCP refuses with that message) or local to one machine. No network and no
   process-to-process messages (user).
2. **`monthly_review`** is listed only to enabled clients; its text is built from the
   tools at the client's level, so it never names one the client can't call; getting it
   isn't logged, since it returns no data (user).
3. **Saved reports are capped at 500 per client**; the 501st is refused with a clear
   error, counted and inserted in one transaction (user). The error tells the client
   that the user can make room with `coffer inbox delete-report`, and names nothing of
   the user's. `coffer inbox reports` notes any client at the cap with the command that
   clears it (user).
4. **`coffer inbox approve 1 2 3` is all or nothing** (`inbox::approve_all`, one
   transaction). A repeated ID is refused before anything is tried; a suggestion that an
   earlier approval in the same list settled is skipped, not an error, and the CLI says
   which rule settled it (user). `reject` still decides each ID on its own.
5. **`coffer inbox reports` and `coffer inbox report <id>`** show saved reports: the list
   has no bodies; a report is printed as plain text, each body line prefixed `  | ` so it
   can't pass for Coffer's own output, labelled as written by the client and unverified.
   The CLI strips exactly what `envelope::strip_invisible` strips (user).
   **`coffer inbox delete-report <id>... [--yes]`** and
   **`coffer inbox delete-report --client <name> [--before <YYYY-MM-DD>] [--yes]`**
   delete saved reports (`inbox::delete_reports`): the user's only way to free a
   client's allowance, so the cap is never a dead end. Without `--yes` they list what
   would go (ID, period, client, date, title, stripped and labelled as above) and change
   nothing, as `coffer restore` and `coffer schedules delete` do; nothing is asked on
   stdin. IDs are all or nothing in one transaction: an unknown or repeated ID deletes
   none. `--before` is the start of that day in UTC, as `created_at` is stored; a client
   with no matching reports deletes nothing and isn't an error. No backup is taken first
   (deleting reports changes no money data); a report stays in backups taken before it
   was deleted until they rotate out. Nothing else refers to a report by ID: no foreign
   key, suggestion payload, `applied_json` or change batch names one, and the access log
   keeps only a hash of `save_report`'s result; `report.id` is `AUTOINCREMENT`, so a
   deleted ID is never reused. No MCP tool or server code path deletes a report: the
   tool × level matrix checks that no call changes or removes an existing suggestion or
   report, and that the server's source never calls core's deletion (user).
6. **`coffer categorize <transaction-id>... <category-key>`** sets categories as the user
   through `inbox::own::recategorize`, so Coffer's own rule suggestions fire from the CLI
   until the GUI's inline edit (M3). It refuses a repeated ID, an unknown category or
   a row that takes no category of its own (split, a confirmed transfer leg, a linked
   refund's money in; decision record "what a transaction can take"), and prints the change batch and any rule suggestion it filed (user).
7. The tool × level matrix (`crates/mcp/tests/matrix.rs`) lists every tool with its level
   and arguments; a new tool fails it until added.
8. **Calls refused while the database can't be opened are logged too** (exit criterion:
   every call is in the log). The log is in the database, so a call refused while a
   restore holds the lock (`refused_restoring`), while the lock can't be checked or while
   `open_existing` fails, e.g. with migrations pending (`refused_unavailable`), is
   recorded in a pending file next to the database, `<database>-access-pending`
   (`access_log::pending`), and moved into `mcp_access_log` with its own time the next
   time the database is opened to write: any server's next call or listing (whichever
   gets there, after its restore-lock check), `coffer ai-access`, and `restore` once the
   restored database is open. Neither existing outcome fitted: `error` means a tool failed, and the
   `refused_*` ones are the gate's (user).
   - A line is the log row's entry and nothing more: client, tool, the call's time,
     outcome, the cleaned purpose (cut to 500) and the redacted arguments, as JSON. No
     result hash (nothing was read). The file is `0600`; each append is synced. Several
     servers append and flush under std's exclusive file lock (`File::lock`) on the file,
     so no new dependency (user).
   - No duplicates: each line gets a UUIDv7 (the `uuid` crate already in core) stored as
     `pending_id`, unique (V14); the flush inserts every line in one transaction with
     `ON CONFLICT (pending_id) DO NOTHING`, commits, then empties the file. A crash
     between the two re-inserts lines that are all ignored, so neither the step order nor
     a marker in the database is needed. Keeping them only in memory was rejected: a
     client that exits before the database is back would lose them (user).
   - Bounded: at most 1,000 entries and 1 MiB. Calls past either are counted per client,
     tool and outcome in `<database>-access-pending-dropped` (replaced whole by a rename),
     and each count becomes one summary row (`summarised_calls`, timed at the first such
     call, no arguments or purpose). The flush renames the counts aside before inserting,
     so counts made after a crash land in a new file with new IDs and aren't lost
     (user).
   - A line cut short by a crash is ended before the next append and skipped on flush,
     like a line the log's schema refuses; the flush counts them and warns. If the pending
     file can't be written either, the call is still refused and the failure goes to the
     server's own log (user).
   - The log is listed newest first by time, then ID, so moved-in rows appear where they
     happened (user).
9. **A restore keeps the access log** (decision record 2026-11-10, point 6). Restoring a
   backup used to replace `mcp_access_log` with the backup's copy, so the rows written
   between the backup and the restore were left only in the pre-restore backup. Now
   `restore`, once the restored database is open and migrated and while it still holds
   the restore lock, copies from the pre-restore backup (a snapshot of the replaced
   database, only read) every log row newer than the restored log's newest, keeping its
   time, then deletes rows past the 12-month retention, in one transaction
   (`access_log::carry_forward`); then it flushes the pending file (F.8). The replaced
   database may be at an older schema: before V11 it has no log, before V14 its rows have
   no `pending_id` or `summarised_calls`. Rows at or before the backup's newest time are
   the backup's own. A `pending_id` already present isn't copied twice. A failed carry
   doesn't fail the restore, which is done by then: `coffer restore` says the rows are
   in the pre-restore backup (user).

### Decision record: approval needs the user's terminal (2026-11-18)

Review finding R.1: `coffer inbox approve` took no confirmation and no terminal check.
Claude Code and Codex have a shell by default, so a prompt-injected client could file a
suggestion over MCP and then approve it itself through the CLI, getting around "MCP writes
go to the inbox only".

1. **Commands that act as the user need an interactive terminal.** They refuse to run
   unless stdin is a terminal (`std::io::IsTerminal`), with an error and a non-zero exit.
   The check is one match in `crates/app/src/cli/terminal.rs`, applied by `cli::run_with`
   before any command runs; the binary passes `Terminal::detect()`.
2. **There is no bypass.** No flag and no environment variable: an agent would use it.
3. **Which commands.** Approving what an AI client filed (`inbox approve`); widening AI
   access (`ai-access enable`, `on`, and `level` when it raises a client's level or the
   client isn't enabled); writing transactions, rules, categories, accounts, budgets or
   schedules as the user (`import`, `undo-import --yes`, `categorize`, `accounts
   add|close|reopen`, `budgets set|remove`, every `schedules` command that writes);
   `restore`; and `inbox delete-report --yes`, so a client can't remove the reports it
   saved.
4. **What stays usable without a terminal.** Everything read-only, `backup`, `export`,
   `mcp`, `inbox reject`, and the ways to contain AI access: `ai-access revoke`, `ignore`,
   `off`, and lowering a level. A script can still contain a client; it can't widen one.
   The check gates the write, not the preview: `import --dry-run`, and `undo-import`,
   `schedules delete` and `inbox delete-report` without `--yes`, read and print and write
   nothing, so they don't act as the user. `restore` without `--yes` stays gated: its check
   stages a copy of the backup in the data directory.
5. **A speed bump, not a boundary.** A process with a shell can fake a terminal (`script`)
   or open the SQLite file directly. Containment depends on the client having no shell, or
   a sandbox that denies it the data directory and the `coffer` binary, as "Use a profile
   with Coffer as the only data source" in `docs/mcp-setup.md` sets up. The terminal check
   is defence in depth. "MCP writes go to the inbox only" (`AGENTS.md`) is about what the
   MCP server does; it holds for a client that reaches Coffer only over MCP, and the
   invariant now says so.
6. **Automating the CLI.** A scheduled `coffer import` from a script is refused too, with
   no flag to allow it: a script's import is indistinguishable from an agent's, and a
   crafted file is a way to write transactions. A script can preview one
   (`--dry-run`); committing it is the user's, in a terminal or the app.

### Decision record: a client reads only its own reports (2026-11-18)

Review finding R.2: `list_reports` is an `aggregate` tool and returned every client's
reports. A `standard` client's review, with merchant names and bank text, reached an
`aggregate` client, and one client could plant text for another to read back.

1. **Its own reports only.** `list_reports` returns the reports whose `source_client` is
   the calling client, so no client reads or is steered by another's text.
2. **Saved at its level or below.** `save_report` records the caller's level in
   `report.access_level` (V19), and `list_reports` returns only reports saved at a level no
   higher than the client's current one (`aggregate` < `pseudonymous` < `standard`). A
   client lowered from `standard` to `aggregate` no longer reads back the review it wrote
   while it could see merchant names, and reads it again if raised. A report saved before
   V19 has no level (NULL) and is read as `standard`, the strictest reading.
3. **Filtered in the query** (`reporting::catalog::reports_visible_to`), so `limit` and
   `truncated` count only the reports the client may see.
4. **The user's view is unchanged.** The app and `coffer inbox reports` show every report
   whatever its client or level. MCP still writes only `suggestion` and `report`.
5. **Consequence.** A report from before V19 is invisible below `standard` until the user
   deletes it or raises the client. Rejected: filtering by level alone, which still lets
   one client plant text for another.
