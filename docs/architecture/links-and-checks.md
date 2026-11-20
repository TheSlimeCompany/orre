# Orre — Architecture: Links and review queues

Change batches, refunds, reimbursements and shared costs, possible duplicates, balance checks and lock dates, export.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Links and checks

What the user reviews and links after an import, and how many rows change at once.
"M1 wave 4" below is "M1 wave 4 — change batches, rule actions, links, duplicates,
balance checks and export".

### Change batches

`orre_core::change` (V8). A change to user-facing fields on stored transactions is one
`change_batch` with a `change_item` per transaction and `Field` (category, merchant,
user note, tags, exclusion, split, share, `reimbursable`, links), holding the value
before and after. Imported source fields are never among them. The kinds
(`ChangeKind`) are `rule_apply` (`rule::commit_apply`), `duplicate_merge`
(`duplicate::merge`) and `bulk_edit`: the user's category edits
(`inbox::own::recategorize`) and the inspector's splits, tags, notes, names, links and
shares (`edit`). Transfer-pair decisions and merchant name aliases aren't batches.

- **Recording.** A `Recorder` sets a field (`set`) or watches core's own code write it
  (`track`), reads the value back and keeps old and new; `finish` writes the batch, or
  nothing when nothing changed. It runs in the caller's DB transaction, so a preview is
  the same code rolled back (`change::dry_run`) ("M1 wave 4", point 1; "refunds,
  reimbursements and settlements (built)", point 2).
- **Undo** (`change::undo`, `undo_preview`) restores items newest first, and only where
  the field still holds the batch's value; the rest, and values that can't be written
  any more, are counted as changed since (`UndoSummary::changed_since`) ("rules", point
  6). Undoing a merge puts its candidates back to `pending`. Change history is
  `change::history` and `history_items`.
- **What can't be written** (`change::unwritable`): a category on a split row, a split
  or share on linked money in, links that no longer fit, the reason `superseded`, and
  any exclusion change on a superseded row. Other writers don't ask `category::assign`
  ("rules skip rows that take no category of their own", point 4).

### Refunds, reimbursements and shared costs

`orre_core::link` (V9). A `transaction_link` ties money in to a purchase (`refund`,
`reimbursement` of a purchase claimed back) or to a person (`settlement`). One link per
pair in any status; only `confirmed` counts. A `rejected` pair keeps its row and is
never suggested again, and unlinking is rejecting ("M1 wave 4", point 10). Both sides
are in one currency, and a link is capped by what the money in has left, what the
purchase can still take (`inflow_left`, `purchase_left`) and what the person owes
(`owed_to`).

- **How it counts.** A confirmed refund or reimbursement adds `ledger_line` rows on the
  money in's date, in the purchase's category, so a late refund leaves the purchase's
  month alone ("refunds, reimbursements and shared costs", point 1; "M1 wave 4", point
  11). Confirmed refund money in takes no category of its own (`category::assign`).
  Spending is the user's share (`personal_minor`); balances stay gross.
- **Sharing** (`link::Sharing`: just me, shared with a person, claim back) is for money
  out only, per line on a split purchase. Linked money in can't be split or shared, and
  a linked row's amount and currency can't change.
- **Suggestions** (`suggest_for_batch` at import, `suggest_all`) skip money in that is
  excluded as `duplicate` or `superseded`, in a transfer, shared, split, or already has a
  suggestion. A refund: a purchase on the same account with the same `merchant_display`
  (ASCII case ignored) within `REFUND_WINDOW_DAYS` (90) with enough left. Otherwise a
  reimbursement: a set of the `REIMBURSEMENT_CANDIDATES` (12) newest claims that sums to
  the payment exactly, the newest within `REIMBURSEMENT_WINDOW_DAYS` (60) ("M1 wave 4",
  point 14). They show in the inspector and Owed to you; an agent's `refund_link` goes to
  the Inbox.
- **Edits** (`edit::link`, `confirm_links`, `reject_links`, `set_sharing`,
  `rename_person`) are one `bulk_edit` batch each, recorded on the `links` field; undo
  restores a set of links only where it still fits ("refunds, reimbursements and
  settlements (built)", point 1). Owed to you is `link::owed::queue`.
- Locks don't apply, and share labels never leave the app.

### Possible duplicates

`orre_core::duplicate` (V10, V20). `detect_for_batch` (at import) and `scan` raise a
`pending` `duplicate_candidate` for two rows on one account, same currency and non-zero
amount, at most `DuplicateWindow::DEFAULT` apart (3 days, both ends), neither excluded,
not from one import, at least one imported or mirrored. Its `CandidateReason`:
`overlapping_import` (both imported, different `description_normalized`),
`manual_entry` (one manual), or `unmatched_source_id` (both imported, the same day,
amount and text, source IDs differing; a row the same import matched by source ID is no
counterpart) ("a source ID is checked against the row's content"). A pair recorded in
any status isn't raised again. Pairs are taken closest first, then by agreeing names,
then IDs ("M1 wave 4", point 17).

A merge (`merge_preview`, `merge`) is one `duplicate_merge` batch that excludes the
duplicate as `duplicate`, never deleting it, and moves what the user set on it to the
kept row where that row has nothing ("M1 wave 4", point 18). The default keeps a row in
a transfer, then one with a source ID, then imported over manual, then the earlier
import (`default_keep`). `dismiss` is final for that pair; `reopen` is its undo. The
Inbox's CHECKS shows the queue through `orre_core::checks` (`checks::merge_effect`:
the merge planned and rolled back, a locked one as if unlocked) ("review queues
(built)", points 1–3).

### Balance checks and locks

`orre_core::reconciliation`. An account's balances (the opening balance, then
snapshots) are ordered by date. The first is the starting point; each later one is
checked against the last passing one plus the transactions between, with rows excluded
as `duplicate` or `superseded` left out and a split row counted once. A `valuation`
sets the balance and is never a check. A failure names its window and is never the base
for the next check
("review queues", point 1; "M1 wave 4", point 20).

`reconciliation::lock` sets `account.reconciled_through` only to the date of a passing
check with no failure on or before it; moving it later is allowed, earlier is refused,
and `unlock` clears it entirely ("review queues", point 2; "M1 wave 4", point 21).

**A lock guards balances only** ("a lock guards balances only"; the statement is in the
`reconciliation` module docs). On or before the lock date, nothing may change which rows
count in the account's balance, or the amount, date or account of such a row. For
exclusions the one predicate is `exclusion::moves_balance(old, new)`: one side leaves
the balance (`duplicate`, `superseded`) and the other doesn't. Refused there:

- an import's new row or file balance (flagged and left out,
  `FileBalanceStatus::Locked`), a bank's correction (`CorrectionBlocked::Locked`), and
  undoing an import that touches the period;
- creating or deleting a manual row, or changing its account, date, amount or currency
  (`manual`);
- an exclusion that `moves_balance`, whether single-row (`exclusion::exclude`,
  `include_again`) or in a change batch or its undo (`Recorder::set`, `track`, `undo`;
  previews ask `change::locked_for`): a rule's `duplicate` exclusion, a merge, undoing
  either;
- an entered balance (`account::add_balance`), closing the account before the lock
  (`CloseLocked`) or deleting a locked one, and a pot mirror row (`PotLocked`).

Allowed there: category, merchant name, a manual row's description and time, notes,
tags, splits, shares, links, transfer-pair decisions, schedules, and exclusions between
reasons that keep the row in the balance (`bank_error`, `disputed`, `other`).

### Export

`orre_core::export`, `orre export` and the app's export dialog: one row per
`ledger_line` row, split lines and link parts with their own IDs, rows of a transaction
summing to its amount. Filters AND together, a filter's values OR. Money is an exact
decimal string beside its `*_minor` integer, with no floats; JSON is versioned
(`orre.export`, `schema_version` 1). Notes (`include_notes`) and private tags
(`include_private_tags`) are opt-in columns that come last; share labels are never
exported ("M1 wave 4", points 2 and 22). `Options::include_excluded` is on for the CLI
and the dialog's toggle ("Settings, AI access and backups (built)", point 7). A file is
created `0600`, an existing one is replaced atomically only with `--force`, and the
database and its side files are refused. The columns are in the module's docs and
`docs/EXPORT.md`.

## Decision records

Oldest first.

### Decision record: review queues (2026-11-10)

The open questions from the D6.4 designs (`DESIGN.md` → "Review queues"), decided by the
user on the designer's recommendation:

1. **The oldest balance is the starting point, not a check.** An account's first
   `balance_snapshot` is where Orre starts counting, and every later one is checked
   against it plus the transactions between them. A `valuation` snapshot sets the
   balance and is never a check.
2. **A period is locked only through a passing check.** `reconciled_through` can be set
   only to the date of a passing check, and only when every check up to that date
   passes. Unlocking clears it entirely; there's no partial unlock.
3. **A merge keeps the best-sourced row.** By default it keeps the row with a source ID,
   then an imported row over a manual one, then the one imported first. The user can
   swap the rows before merging.
4. **The near-duplicate window is 3 days.** It's a default, not an invariant (see
   "Defaults, not invariants").
5. **A dismissed pair stays dismissed.** A `duplicate_candidate` with `status =
   dismissed` is never raised again for those two transactions. A later import that
   brings a third match creates a new pair.

### Decision record: refunds, reimbursements and shared costs (2026-11-10)

The open questions from the D6.5 designs (`DESIGN.md` → "Refunds, reimbursements and
shared costs"), decided by the user on the designer's recommendation:

1. **A refund counts on its own date.** It's negative spending in the purchase's category
   (`ledger_line`), in the month the money came back. The purchase's month doesn't
   change, so a closed month stays as it was when a late refund arrives.
2. **Link suggestions are deterministic and live in the inspector.** Orre suggests:
   - a *refund* when money comes in on the same account under the same
     `merchant_display` as a purchase in the previous 90 days that has at least that
     much left to refund;
   - a *reimbursement* when money comes in that equals one outstanding claim, or a set
     of them, to the penny, within 60 days of the newest.

   Suggestions are `transaction_link` rows with `status = suggested`, shown in the
   inspector and on Owed to you, not in the Inbox. A rejected pair is never suggested
   again. The AI's `refund_link` suggestions stay in the Inbox (D6.6).
3. **Spending is your share by default.** Spending, budgets, Overview, Trends and MCP
   reads use `my_share_minor`. A purchase being claimed back counts as £0 while it's
   outstanding, and switching it to "Just me" counts it in full again. Balances, net
   worth and the Transactions table are always gross. A gross toggle on reports can come
   later.
4. **An overpayment is capped.** A link's `amount_minor` is at most what the purchase can
   still take or what the person owes. The rest of the incoming money stays unlinked and
   counts by its own category.
5. **A link needs one currency in v1.** Both sides must be in the same currency;
   cross-currency refunds wait for L.28.

### Decision record: M1 wave 4 — change batches, rule actions, links, duplicates, balance checks and export (2026-11-13)

M1.24–M1.27 and M1.29. Migrations `V8__change_batch.sql`, `V9__transaction_link.sql` and
`V10__duplicate_candidate.sql`; M1.24, M1.27 and M1.29 needed none (V11 stays free).
Points 2 and 3 were decided by the user on 2026-11-12.

**Shared**

1. **Change batches** (`orre_core::change`, V8). A `Recorder` sets a field, reads the
   value back (a timestamp filled in, tags sorted, a derived name recomputed) and keeps
   old and new; `finish` writes the batch and one item per transaction and field, or
   nothing when nothing changed. It runs in the caller's DB transaction, so a preview
   is the same code in a rolled-back transaction (`change::dry_run`). `undo` restores
   items newest first, and only where the field still holds the batch's value (rules
   decision point 6); a category counts as still holding when its category and source
   are unchanged (a deleted rule's cleared `rule_id` doesn't count, rules point 7), a
   derived name when it is still derived. A value that can't be written any more (its
   category deleted, the row split since) is counted as changed since. A split is set
   only on a row with no category or share of its own, so a recorder clears those first
   and undo puts them back after the lines go.
2. **Export leaves notes and private tags out by default.** `--include-notes` adds the
   user's note, split lines' notes, the exclusion note and the bank's imported note (in
   Monzo's export that is text the user wrote); `--include-private-tags` adds a separate
   `private_tags` column and allows filtering by a private tag. Share labels are never
   exported.
3. **A rule's exclusion counts as a change under a lock, whatever its reason.** Any
   exclusion change made through a change batch on a row dated on or before its
   account's `reconciled_through` is refused, and so is undoing one there: rule applies,
   merges and their undos all need an unlock first. This amends rules decision point 8
   (which flagged only `duplicate`). Single-row exclusions (`exclusion::exclude`) still
   meet the lock only into or out of `duplicate`. *Amended 2026-11-19 by the user:* a
   lock guards balances only, so in a change batch too only an exclusion into or out of
   a reason that leaves the balance (`duplicate`, `superseded`) meets it; a rule's
   `bank_error` or `disputed` exclusion and its undo go through. See "Decision record: a
   lock guards balances only".

**A — rule actions (M1.24, `orre_core::rule`)**

4. **Rules win field by field, tags included.** Category and split are one field (the
   first rule to set either takes it); merchant, tags, note, exclusion and share are one
   rule each, so a second matching `add_tag` rule adds nothing.
5. **Shares** are `amount × bp / 10000`, rounded half away from zero, which keeps the
   amount's sign and stays within it. 10000 bp stores `NULL` ("all of it"); the share
   label is left as it is.
6. **Split lines** round down in absolute value, then the leftover minor units go one
   each to the largest remainders, ties to the earlier line: the lines sum exactly and
   keep the parent's sign. A row isn't split when a line would get 0, when it is in a
   suggested or confirmed transfer, or when it is money in with a suggested or confirmed
   link (B); it keeps its category and the import goes on (`CommitSummary::
   splits_skipped`). With a split, the rule set's share goes on each line; without one,
   on the row (never on linked money in).
7. **Import uses the same planner as Apply**, writing through a `change::Recorder` that is
   never finished, so the import records no change batch. Nothing the user set is changed
   at import.
8. **Apply** (`preview_apply` → `commit_apply`) plans one rule against every transaction
   it matches, evaluating the whole current rule set so it changes only the fields it
   wins. Split rows are skipped and listed. Values the user set (a `user`/`ai_approved`
   category, a user-set name, any existing note, exclusion or share, and a category or
   share a split would clear) change only on rows passed as included; including a row
   with nothing user-set is refused. The commit re-plans and refuses a stale preview,
   and refuses while any change it would make meets a lock (point 3). One `rule_apply`
   batch per apply; tags it created stay after undo, without their links.
9. Notes and shares have no "set by" column, so a note or share a rule set at import
   counts as user-set on a later Apply.

**B — refunds, reimbursements and shared costs (M1.25, `orre_core::link`, V9)**

10. **One link per pair, in any status.** A rejected row stays, so the pair is never
    suggested again; unlinking is `reject` and its undo `confirm`. Only confirmed links
    count, in the caps and in `ledger_line`.
11. **Netting is extra `ledger_line` rows.** A confirmed refund or reimbursement adds a
    row on the inflow (its date and account) in the purchase's category, share
    (pro rata, truncated toward zero), label and `reimbursable`; the inflow's own row
    keeps the unlinked rest (0 when fully linked). Against a split purchase the link is
    divided over the lines by largest remainder. A settlement adds a row with no
    category and a share of 0. A transaction's rows still sum to its amount, so balances
    read the same rows. `in_reports` drops excluded rows and confirmed transfers (a
    link part also when its purchase is); `personal_minor` is the user's own spending or
    income: `my_amount_minor`, or 0 when not in reports or reimbursable. A linked purchase
    is at most 3,037,000,499 minor units so the pro-rata products stay integers.
12. **Reimbursable means the whole purchase is claimed**: it counts as 0 of the user's
    spending, outstanding or repaid; set back to "just me", it counts in full and repaid
    amounts net against it like refunds. "Shared" and "claim back" exclude each other
    and apply to money out only; a split purchase is shared per line.
13. **Owed to me** per label and currency is Σ(`my_amount_minor − amount_minor`) over the
    label's rows in reports and not reimbursable; a settlement subtracts because its
    share is 0. Labels are trimmed, at most 64 characters, compared exactly.
14. **Suggestions** (`suggest_for_batch` at import, `suggest_all`): money in that isn't a
    duplicate, a transfer, split, shared or already suggested. A refund: a purchase on
    the same account with the same `merchant_display` (ASCII case ignored) within 90
    days up to the refund, with enough left; an exact remaining amount wins, then the
    newest. Otherwise a reimbursement: the 12 newest outstanding claims on or before the
    payment, every subset tried, summing to the payment exactly with the newest within
    60 days; fewest claims, then newest, win.
15. A linked row's amount and currency can't change (`manual::update` refuses), and
    linked money in can't be split or shared.

**C — possible duplicates (M1.26, `orre_core::duplicate`, V10)**

16. **A pair** is two rows on one account, same currency and non-zero amount, at most
    3 days apart (`DuplicateWindow::DEFAULT`, both ends included, at most 31), neither
    excluded, not from the same import batch, at least one imported or mirrored. Two
    imported rows must differ in `description_normalized` (identical text is the exact
    dedupe's job or a genuine repeat), except an `unmatched_source_id` pair (V20): the
    same day, amount and text where the source IDs differ (decision record "a source ID
    is checked against the row's content"); a manual row pairs whatever its text. A pair
    already recorded in any status isn't raised again.
17. **Deterministic**: smallest gap first, then pairs whose display names agree, then
    IDs; each row in at most one new pair per run. Transfers and split rows take part.
18. **A merge** is one `duplicate_merge` batch (one candidate or many) that excludes the
    duplicate as `duplicate` in place and marks the candidate `merged` with `kept`. The
    default keeps a row in an active transfer, then one with a source ID, then imported
    over manual, then the earlier import (review queues point 3); the user can swap.
    User-set values on the duplicate (category, split, share, name, note, tags,
    `reimbursable`) move to the kept row where it has none, as items in the same batch.
    A stale preview, a non-pending candidate, rows that no longer match and conflicting
    requests are refused; so is a locked duplicate (point 3).
19. **Undoing a merge** puts every candidate in it back to `pending`, even one whose
    exclusion changed since (it can then be dismissed). Deleting a transaction deletes
    its candidates. Undoing the import of a kept row leaves its merged duplicate excluded;
    `UndoPreview::merged_into` says so, and undoing the merge includes it again.

**D — balance checks, lock dates and export (M1.27, M1.29)**

20. **Every balance is checked** (`orre_core::reconciliation`): the opening balance
    first on its day, then snapshots by ID, then by date. The first is the starting
    point; each later one is checked against the last passing one plus the transactions
    between (only `duplicate` exclusions leave the sum; a split row counts once). A
    valuation sets the balance and restarts the count. A failure names the window
    `(since, date]`, the stated and computed amounts and the difference, and is never
    the base for the next check.
21. **Locking**: `lock` only on the date of a passing check with no failed check on or
    before it; moving the lock later is allowed, earlier is refused; `unlock` clears it
    entirely. `account::add_balance` now refuses a date on or before the lock. Still to
    guard when they arrive: editing or deleting an entered balance, the opening balance
    or date, closing an account, deleting one.
22. **Export** (`orre_core::export`, `orre export`) is one row per `ledger_line` row:
    split lines carry the parent's `transaction_id` with their `split_id`/`line_no`, and
    link parts their `link_id`. Nothing is filtered by status. Filters AND together, a
    filter's values OR; dates inclusive; a group's category key includes its
    subcategories; tags match ignoring case. Money is an exact decimal string beside its
    `*_minor` integer; JSON numbers are integers only; JSON is `{schema: "orre.export",
    schema_version: 1, includes, rows}`, CSV has a header row and quotes only when
    needed. Opt-in columns come last, so the default header doesn't change. A file is
    created 0600, an existing one is refused without `--force` (which replaces it
    atomically), and the database itself is refused as the output. The column table is
    in the `export` module's docs.

Open: CSV cells beginning with `=`, `+`, `-` or `@` are left as they are (a spreadsheet
may read them as formulas); `external_id` is always exported; share labels may want an
opt-in flag later; share labels compare case-sensitively.

### Decision record: review queues (built) (2026-11-14)

M3.12. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build.
Everything else follows `DESIGN.md` → "Review queues", the "Decision record: review
queues" above and "Decision record: M1 wave 4" (C and D). No migration.

1. **The queues live under CHECKS in the Inbox (user).** As D6.4 draws them
   (`8SE-0`, `98K-0`): the Inbox's left panel gains CHECKS between INBOX and KIND, and
   selecting a row there shows that queue in the Inbox's canvas and inspector. The
   Reviews tab stays for saved AI reports (M4.7). `crate::checks` (the state,
   `ChecksStore` opening the database per call, the two queues, the lock and unlock
   dialogs); the shell swaps the Inbox's canvas while a queue is open.
2. **Core reads, core decides (`orre_core::checks`).** A read model over
   `duplicate` and `reconciliation`, with no writes of its own: each pending pair with
   both rows, why it was flagged and its default keep; each account's checks with the
   balance counted from, the transactions in the window and the pair that explains a
   failure; the dates an account can be locked through; and the accounts not checked.
   Merging is still `duplicate::merge_preview` then `merge` (one `duplicate_merge` batch
   per pair; a stale or locked plan is refused), dismissing `duplicate::dismiss`,
   locking `reconciliation::lock`/`unlock`.
3. **"If you merge" is the merge rolled back.** `checks::merge_effect` runs the merge's
   own plan (`duplicate::plan`, now `pub(crate)`) in a rolled-back transaction and
   reads, before and after, the pair's `ledger_line` spending and the account's balance
   checks: the row excluded, the values moved, the spending each category gains or
   loses, Orre's count on the next check's day, and every check whose result changes.
   A pair "explains" a failing check when this merge makes it pass. A duplicate in a
   locked period is planned with the lock lifted inside the rolled-back transaction, so
   the inspector shows what an unlock would allow; `MergeEffect::locked` says so, and
   the commit still refuses (M1 wave 4, point 3).
4. **The values a merge moves are shown (user).** "If you merge" has a Moves row when
   the duplicate's category, split, share, name, note, tags or claim move to the kept
   row (M1 wave 4, point 18), so nothing changes that the preview didn't name.
5. **Dismissing can be undone from the bar (user).** "Not duplicates" shows the D6.3 bar
   with Undo, which reopens the pair (`duplicate::reopen`). A merge's Undo is
   `change::undo` of its batch, which puts the pair back to pending. Both reopen the pair.
6. **Selection (user).** Opening Possible duplicates opens its first pair; opening
   Balance checks selects the first failing check. A passed check's inspector shows
   "This check" without the explanation; a starting point says Orre counts from it;
   with nothing selected, one caption. Every check's inspector ends with the account's
   lock (Unlock…, or Lock through … when a later date is lockable).
7. **Left out until their items (user).** "Show in Transactions", "Show N transactions"
   (Transactions has date presets, not a window) and "Edit balance…" (core can't edit a
   balance yet) aren't drawn; their captions stay.
8. **Hooks outside the queues (user).** The status bar's "Reconciled" becomes "N
   balance checks failing" with a `danger` dot while one fails, and opens Balance
   checks; the Accounts panel's flagged dot opens Balance checks on that account's
   failing check. The account inspector is unchanged.
9. **Composed, not drawn.** Pots with nothing to check join "Not checked" ("Pots follow
   their transfers"), as do accounts with no balance and asset/liability accounts.
   A refusal from core is said under the toolbar in caption type, `danger`. The
   wording for flags and sources the artboards don't show: "one added by hand", "bank
   IDs differ", "Different bank IDs", "Imported from {file}", "From a pot transfer in an
   import", "Statement (CSV)", "Opening balance". The empty Balance checks queue uses
   the empty Possible duplicates layout with the 16px scales.

### Decision record: refunds, reimbursements and settlements (built) (2026-11-14)

M3.11. Migration `V17__change_item_links.sql` (V16 is M3.13's).
Marked **(user)**: decided by the user on 2026-11-14 when asked before the build (each
was the recommended option). Everything else follows `DESIGN.md` → "Refunds,
reimbursements and shared costs", the "Decision record: refunds, reimbursements and
shared costs" above and "Decision record: M1 wave 4" (B).

1. **Every link edit is one undoable change batch (user).** `change_item` gains a
   field, `links` (V17 rebuilds the table to widen its CHECK; nothing references it,
   so foreign keys stay on, and every row, ID and the sequence carry over). Its value
   is the set of links whose money in is the item's transaction, any status, sorted by
   pair (`change::LinkValue`, without ID or date); every link has exactly one inflow,
   so each is recorded once. Writing a set reconciles pair by pair, everything that
   lowers the confirmed totals first, so V9's caps hold at each step; a link keeps its
   row where the schema lets it change in place (unlink and its undo keep the ID).
   Undo restores the set only where it still holds, and only when it still fits: the
   write is tried in a savepoint and the schema's own triggers decide, so a set that
   no longer fits counts as changed since. Links aren't guarded by locks.
2. **Core's own code writes, the recorder watches.** `edit::link`, `confirm_links`,
   `reject_links` (unlink, "Not a refund", "Not this"), `set_sharing` and
   `rename_person` run `link`'s checked functions inside a `bulk_edit` batch through
   `Recorder::track` (read the named fields, run, read again, record what changed).
   A link records the `links` of every payment with a link to the purchase too, so a
   suggestion it drops comes back on undo. A share records `share`, `reimbursable`
   and `split`; a rename every share, split line and settlement with the name.
3. **The read models are core's.** `reporting::linking` reads the register (no
   database per keystroke): how a row counts (`counts_as`: the category cell's
   "· refund", "· refunded", "· shared", "· to claim", "· claimed", "Repays 2 claims",
   "Repays Sam"), the link dialog's candidates (same merchant first, then newest, at
   most 50, blocked ones listed with the reason; a suggested pair stays listed and
   linking it confirms the suggestion), people a settlement can repay, payments for a
   person, the default amount (what both sides can take) and why a refund was
   suggested. `RegisterRow` gains its links (suggested and confirmed, both sides),
   `share` (my share, person, claim, what is left to link) and `in_transfer`; the
   register carries what each person owes. `link::owed::queue` is Owed to you.
4. **A person's costs are the outstanding ones (user).** What they paid back
   (settlements, refunds of shared costs) clears their oldest costs first, so the list
   and "2 since 30 Oct" are what is still owed and sum to it; a part-paid cost shows
   the rest. "Settled everything up to" names their last payment's date. Someone who
   owes nothing (or whom the user owes) isn't listed.
5. **The table says how a row counts (user).** Amounts stay the bank's; the category
   cell gains a `text-muted` suffix, or "Repays …" in `text-secondary` with the marker
   lane empty (`ui::transaction_row::CategoryCell::suffix`, `repays`). A linked
   refund's category cell and inspector Category show its purchase's category,
   read-only with the 10px link, until it's unlinked.
6. **Undrawn states, composed (user).** "Link…" on a purchase opens the same dialog
   listing money in on or after it (Refund, and Reimbursement when it's claimed back).
   "Link a payment…" on a person opens it with the kind fixed to Settlement, listing
   money in, newest first. A settlement from money in lists people as 28px rows: the
   lock, the name, "owes" and the amount. The Owed by select is the tag menu's
   pattern: people A–Z with the lock, and "Add “…”" for a typed new name. Also
   composed: a payment's reimbursement card in the inspector ("Does this repay your
   claims?"), the strip's one- and many-claim wording, the empty dialog list's
   captions, a person's empty inspector caption, the undo bar's "Unlinked the …" and
   "Orre won't suggest that … again", and the Links caption on money in.
7. **Shared waits for a person.** Choosing Shared fills Yours with half the amount
   and saves nothing until a person is chosen; then Yours saves on blur or Enter.
   Just me and Claim back save at once. A split purchase is shared line by line in
   core, which no drawing covers yet, so its Shared segment is disabled. A share
   change shows no undo bar (the M3.3 rule for one row); a link, an unlink or a
   rejected suggestion does.
8. **Where things live.** `transactions/links.rs` (the Share and Links sections),
   `link_dialog.rs` (the dialog, opened by the shell on `TransactionsEvent::
   LinkRequested`), `linking.rs` (the words), `link_fixture.rs` (D6.5's data, written
   through core); Owed to you is `checks/owed.rs`, a third `Queue` under CHECKS, its
   Name field owned by `ChecksView`. Shared additions: `Glyph::Owed`/`OwedLarge`,
   `ButtonVariant::Ghost`, `Segmented::fill`, `SelectField::private`/`amount`. Share
   labels never reach MCP (the surface is unchanged).
9. **Change history shows a link edit as a count (merge with M3.13).** M3.13's Change
   history (`change::history`) reads the `links` field as `Shown::Links`, the number of
   confirmed links on the money in ("Not linked", "1 link", "3 links"); suggested and
   rejected links don't count, as everywhere else. Not drawn in D6.7; composed at
   merge. Migrations: M3.13's is V16, this one V17. Transactions has one
   `TransactionsEvent` (rule prompt and links alike).

### Decision record: a source ID is checked against the row's content (2026-11-18)

Review finding R.3. The exact dedupe trusts a source ID: a row with an ID never matches by
fingerprint, and near-duplicate review skipped two imported rows with the same
`description_normalized`. A bank that regenerates its IDs (`FITID`) on each download, or a
file imported again through a mapping without its ID column, therefore stored every
overlapping row twice with nothing flagged. Only a balance check would notice, and
Monzo-style exports have none.

1. **A third candidate reason, `unmatched_source_id`.** Two `imported` rows on one
   account, from different import batches, pair when they have the same date (exactly,
   not within the window), currency, amount and `description_normalized`, at least one
   has a source ID and the IDs differ (a missing one differs from any), neither is
   excluded, and the pair isn't recorded in any status. Mirrored rows are left out: they
   follow their main row, which pairs on its own. Rows where neither has an ID stay out:
   the fingerprint's occurrence index owns those (two coffees on one day are two rows).
   Point 17's ordering applies unchanged; at 0 days apart these pairs come first.
2. **Guard: a row the file repeats isn't a counterpart.** A stored row the same import
   matched by source ID is that file row, so it can't also be a new row's duplicate:
   stored coffee X, then a download with X and a second coffee Z on the same day, dedupes
   X and pairs nothing. The import passes the rows its plan matched by source ID to
   `duplicate::detect_for_batch`. With regenerated IDs nothing matches, so every
   overlapping row pairs. `scan` has no import to guard against and pairs every such row;
   a pair that is really two transactions is dismissed once and stays dismissed.
3. **Nothing is dropped or altered.** The second copy is stored and flagged, then merged
   (excluded as `duplicate`, undoable) or dismissed by the user. A genuine repeat whose
   stored twin isn't in the file still goes to review, which is the safe side. A
   preview-time flag was rejected: the duplicate review already has merge, dismiss and
   undo.
4. **Schema: V20** rebuilds `duplicate_candidate` to widen the reason's CHECK, with its
   rows, IDs, indexes and triggers as they were. No other table references it, so the
   rebuild runs with foreign keys on.

### Decision record: a lock guards balances only (2026-11-19)

Review finding R.8. `account.reconciled_through` was said to guard balances only, but the
guards disagreed: a change batch refused any exclusion change (the 2026-11-12 amendment,
"it changes reported totals") while category edits, which change totals too, went
through; a single-row exclusion was refused only into or out of `duplicate`; and a manual
row's merchant or description edit was refused though it moves no balance.

**The meaning (user, 2026-11-19).** A lock freezes the account's balance through the lock
date. On or before `reconciled_through` (the old or the new date), nothing may change
which rows count in that account's balance, or the amount, date or account of such a
row. Allowed there: category, merchant name, a manual row's description, its time within
the day, notes, tags, splits, shares, links, transfer-pair decisions, and exclusions
between reasons that keep the row in the balance. This reverses the 2026-11-12
amendment (rules decision point 8; "M1 wave 4", point 3). The statement lives once in
core, in the `reconciliation` module docs; for exclusions the one predicate is
`exclusion::moves_balance(old, new)`: one side leaves the balance (`duplicate`,
`superseded`) and the other doesn't.

Each guard follows from it:

1. **Import** (`import`): a new row on or before the lock is flagged and left out, a file
   balance there is left out (`FileBalanceStatus::Locked`), a bank's correction of a row
   on either side of the lock is refused (`CorrectionBlocked::Locked`), and undoing an
   import with a row, a replaced row or a balance there is refused. Unchanged.
2. **Manual rows** (`manual`): creating, deleting, or changing the account, date, amount
   or currency of a row on or before the lock (old or new date, old or new account) is
   refused. **Changed:** the merchant, the description and the time within the day are
   now editable there, like the category already was.
3. **A single-row exclusion** (`exclusion::exclude`, `include_again`): refused only when
   it moves the row into or out of the balance (`moves_balance`). Unchanged in effect;
   it now calls the shared predicate.
4. **Change batches** (`change`: rule applies, duplicate merges, bulk edits, and their
   undo): `Recorder::set`, `Recorder::track` and `undo` refuse a change only when
   `Value::moves_balance(old, new)` holds on a locked row, comparing the value the field
   holds with the one written or restored. **Changed:** a rule's `bank_error`,
   `disputed` (or `other`) exclusion, a change between those reasons, and the undo of
   either, now go through inside a lock; into or out of `duplicate` is still refused,
   and so is undoing a merge there. `Field::guarded_by_lock` (field-wide) is gone.
   Previews ask the same question: `change::locked_for(conn, transaction, &value)`
   feeds `PlannedChange::locked` (so `ApplyPreview::locked_rows` and the commit's
   `ApplyError::Locked` agree with the write) and `MergePreview::locked`.
   `change::locked(conn, transaction, field)` stays for a caller that doesn't know the
   value yet (`checks::merge_effect`; a merge always excludes as `duplicate`, so the
   two agree there).
5. **Entered balances** (`account::add_balance`, cash counts): none on or before the
   lock. Unchanged.
6. **Closing an account** (`account::close`): never before the lock (`CloseLocked`), and
   a locked account can't be deleted. Unchanged.
7. **The pot mirror** (`sub_account`): an import that would add a mirrored row on or
   before a pot's lock fails (`PotLocked`). Unchanged.
8. **Schedules** (`schedule`) create no rows, and skipping or matching an occurrence
   changes no row, so none of it moves a balance. **Changed (follow-up, 2026-11-19):**
   skipping, matching, their undo and deleting a schedule now go through on any date,
   and a new schedule matches rows on or before the lock as well as later ones. This
   amends "Decision record: M2: MCP server", D points 5 and 7, which were written under
   the old meaning.
9. **Links, transfer-pair decisions, splits, shares, tags, notes and names** were never
   guarded and still aren't.
