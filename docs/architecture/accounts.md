# Coffer — Architecture: Accounts

Accounts and their balances: pots and other sub-accounts, accounts the user keeps by hand, assets and liabilities, closing and deleting, cash counts.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Decision records

Oldest first.

### Decision record: accounts you keep yourself (2026-11-10)

The open questions from the D6.9 designs (`DESIGN.md` → "Accounts you keep yourself"),
decided by the user on the designer's recommendation:

1. **A valuation can carry a private note.** `balance_snapshot` gains an optional `note`
   (a new migration). It's user text, treated like `user_note`, and isn't sent over MCP
   by default.
2. **Valuation accounts hold valuations only.** `asset` and `liability` accounts hold
   `valuation` snapshots and no transactions. A payment toward one stays on the account
   that paid it, as spending or a transfer the user categorises, and net worth catches
   up at the next valuation. Linking payments to a loan balance can come later.
3. **Closing sets `closed_on`.** `account.closed_on` (a date; a new migration) applies to
   every kind of account. Closing a valuation account also writes a £0 `valuation`
   snapshot on that date, which reopening removes. The closing date must be later than
   the account's last valuation, so the £0 never replaces a value the user entered and
   reopening loses nothing; the close dialog offers the day after the last valuation as
   the earliest date. An account with transactions keeps D6.1's close-only rule.
4. **Only an account without transactions can be deleted.** A valuation account or a
   new empty one can be deleted together with its snapshots. Deleting can't be undone,
   so the confirmation says how many values go with it.
5. **A cash count is a checked balance.** A count is a `balance_snapshot` with source
   `manual`, checked like any other balance (the first is the starting point; Review
   queues, point 1). "Add and save" writes the snapshot and the difference transaction
   in one DB transaction. Coffer never posts a difference on its own.

### Decision record: sub-accounts (2026-11-11)

Decided while building M1.17 (`coffer_core::sub_account`, `V4__sub_account_key.sql`) and
wiring it into `import::commit`. The preset format is under "Presets".

1. **Pots are found by key, never by name.** `account.sub_account_key` (V4) holds the
   bank's ID for the pot, only on pots, unique per parent, and
   read-only once set. A key the parent has no pot for creates one: kind `pot`, the
   parent's currency and institution, named from the latest-dated row that names it (the
   later row on a tie), else after the key. Existing pots keep their names, so a pot
   renamed at the bank stays one account. The same key under another parent is another
   pot. A pot the user created by hand has no key and isn't matched; the import creates a
   keyed pot beside it (linking the two is for the accounts screen, M3).
2. **The mirror runs inside the import's DB transaction,** after the batch's rows are
   stored and before rules and transfer detection, which both see the mirrored rows.
   Each mirrored row is the parent row seen from the pot: same batch, date and time,
   opposite amount and original amount, the parent's other source fields, note and
   category, `origin = 'mirrored'`. The preview plans it (`ImportPreview::sub_accounts`:
   pots to create, existing and closed pots, rows to mirror), and the plan is part of the
   stale check.
3. **The mirrored row keeps the parent's `external_id` and `fingerprint`,** so the pot's
   dedupe indexes hold at most one mirror per parent row; mirroring a row twice fails.
4. **The pair is `confirmed` from the start** (`transfer::record_confirmed_pair`): the
   bank says it's a transfer. Detection skips both rows, since they are grouped.
5. **All or nothing.** Pot transfers into an account that isn't current or savings, or a
   pot locked on or before one of its rows, fail the import, in the preview already;
   half a transfer is never imported. A closed pot still takes its rows, and the preview
   says so. A matching row without a pot ID is flagged (`RowProblem::SubAccountKeyMissing`).
6. **Counts and undo.** `CommitSummary::transactions` and the category counts are the
   file's rows; `mirrored` and `pots_created` are counted apart. `BatchInfo::transactions`
   includes mirrored rows. Undo deletes the mirrored rows with the batch and dissolves
   their groups; `UndoPreview::transfers` counts only pairs with a row outside the batch,
   since both sides of a pot pair go. Created pots stay (the user may have entered an
   opening balance), and importing the file again finds them by key. Re-importing the
   same file mirrors nothing, because its rows are already imported. Files imported
   before M1.17 keep their pot rows without a pot side; undo and import them again to
   mirror them.
7. **Pots aren't a default import target.** `coffer import` without `--account` no
   longer counts pots: their rows come with the parent's export, and once an import has
   created pots the parent must stay the only candidate. A pot still takes a file named
   with `--account`.
8. **Amended 2026-11-13 (M1.9, user decision): Monzo pots are keyed by name.** Point 1
   assumed Monzo's export gives a pot ID (`pot_…` in `Description`). Checked against a
   real export, it doesn't: `Description` is empty on every pot transfer, and only
   `Name` says which pot it is. Rather than flag every pot transfer, the Monzo preset
   uses `key = Name`. The cost: renaming a pot in the Monzo app makes a new pot in
   Coffer at the next import (its earlier rows stay on the old one), and two pots with
   the same name are one. The core is unchanged: it still matches `sub_account_key`
   exactly and never renames a pot; the key is just a name for this bank. A preset whose
   export has a stable ID keeps using it. If Monzo adds pot IDs, the preset moves to
   them in a new decision, since existing keys would no longer match.
9. **Pot transfers can carry their own category (2026-11-13, user decision).**
   `[sub_account_transfer] category` gives every matching row that category ahead of
   `category_map` (stored as `preset_map`, like the map's). Monzo files pot transfers
   under "Transfers", which also holds payments to and from people, so the map can't
   tell them apart; the Monzo preset sets `transfer.savings`.

### Decision record: accounts management (2026-11-14)

M3.2. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build.
Everything else follows `DESIGN.md` → "Accounts panel" or an earlier record.

1. **A preset per account (user).** The inspector's Preset select ("used by the next
   import") is stored on the account: `account.preset_id` (V15), the preset's stable ID
   (`uk-monzo`), the same text `import_batch.preset_id` records. Only current, savings and
   credit card accounts have one (a `CHECK`): cash, asset and liability accounts are kept
   by the user, and a pot's rows arrive with its parent's imports. Core doesn't check the
   ID against the presets (they live in `coffer-import`, and user presets come in M3.5);
   the app and `coffer accounts add --preset` offer only presets they have, and an
   import whose preset has gone falls back to detection. The import flow (M3.4) is what
   reads it; `coffer import` doesn't yet.
2. **Editing is core's (`account::update`).** Name, bank, kind, currency and preset change
   together through `AccountEdit`; the name is trimmed and can't be blank, a blank bank
   or preset is none, and a kind that isn't imported keeps no preset. What the account
   holds (`account::usage`) decides the rest, checked before the schema's triggers so
   the error says why: the currency is fixed once the account has transactions,
   balances, pots or a rule with amount bounds, and always for a pot (it follows its
   parent); a pot stays a pot and nothing becomes one (imports make pots); an account
   with transactions can't become an asset or liability; one with balances can't move
   in or out of the valuation kinds; one with pots stays current or savings. Account
   edits aren't change batches: they touch no transaction.
3. **The opening balance is the first entered balance.** The New account dialog's
   optional Balance is a `manual` `balance_snapshot` written in the same DB transaction
   as the account (`account::create_with_balance`), so a refused balance adds no
   account. It is the account's starting point, as "Decision record: import commit,
   dedupe and undo", point 6 already has it; `account.opening_balance_minor` stays for
   the checks that read it, and nothing writes it. "Add balance…" records more
   (`account::add_balance`, one per day, after the lock date).
4. **The panel's numbers** (`reporting::account_tree`): open accounts in creation order,
   each pot under its parent; the latest balance at the end of each account's own last
   active day; an account other than a pot with no stated balance shows "No balance"
   and is left out of net worth (its sum from zero is right only if it started empty),
   while a pot counts from its transfers; net worth per currency, never converted, a
   liability always subtracting (as `net_worth_history`); `danger` only for a current,
   savings, cash or pot account below zero; flagged when a balance check failed.
5. **"Close account…" waits for M3.17 (user).** It isn't in the M3.2 line, closing a bank
   account still needs its lock-date guard (M1 wave 4, point 21), and M3.17 brings the
   close dialog for valuation accounts. The inspector has no footer until then.
6. **The app holds no connection (as M3.1).** `accounts::store::AccountStore` opens the
   database for each read or write, creating and migrating it as the CLI does, through
   `backup::gate`, which checks the restore lock before and after opening. Reads and writes are a person's accounts, so they run on the
   main thread; the panel and inspector reread everything after a change. A database
   that can't be read leaves the panel empty with the reason in `danger`.
7. **Where the account inspector shows (user).** On Transactions, while an account is
   selected and no transaction is: clicking an account clears the transaction
   selection. Since M3.3 the table is filtered by it too; on the views without an
   inspector, selection only highlights the row.

### Decision record: asset and liability accounts, closing and deleting (built) (2026-11-14)

M3.17. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build.
Everything else follows `DESIGN.md` → "Accounts you keep yourself" (D6.9), "Decision
record: accounts you keep yourself" above and "Decision record: accounts management".
**No migration**: `account.closed_on` and `balance_snapshot.note` are in V1.

1. **A valuation is an entered balance (`account::add_balance`).** `EnteredSource` gains
   `Valuation` (`for_kind` picks it for asset and liability accounts); an asset or
   liability takes only valuations and other accounts none (`HoldsValuations`,
   `NotValued`, checked before the schema's trigger so the error says why). It keeps the
   entered-balance rules: one per account and day, none on or before a lock (valuation
   accounts can't be locked) and, new for every kind, none after `closed_on`
   (`AfterClosing`). `create_with_balance` writes a valuation as a new valuation account's
   first value. Reporting already read a valuation as the balance (`reporting::balance`),
   so net worth and `account_tree` needed nothing new.
2. **A liability's value is at or below zero, and core never flips a sign.** Core refuses
   a positive liability valuation (`LiabilityAboveZero`). The app's "Owed" field is
   unsigned and refuses a minus; the app negates it and its caption says what will be
   stored ("Coffer stores it as −£212,640.00", `FS6-0`). An asset's Value is unsigned too.
3. **Closing (`account::close`)** sets `closed_on` and deletes nothing. Its open pots close
   with it on the same day. An asset or liability also gets a £0 valuation that day. The
   earliest day (`closing(..).earliest`): the day after the last valuation for a
   valuation account, so the £0 never replaces a value; for any other kind the day of its
   (or a closing pot's) last transaction, balance or opening balance, so no row is dated
   after the account closed. **Lock-date guard** (M1 wave 4, point 21): never before
   `reconciled_through` (`CloseLocked`), since locked days' net worth would change;
   closing on the lock date changes none of them. Closed accounts already left the panel
   and every month-end after `closed_on` (`account_tree`, `net_worth_history`).
4. **Reopening (`account::reopen`)** clears `closed_on` on the account and on the pots that
   closed with it (same day; a pot closed earlier on its own stays closed), and deletes
   the £0 valuation closing wrote: the zero `valuation` on the closing day, which the date
   rule makes the one closing wrote. A pot whose parent is closed can't reopen alone
   (`ParentClosed`). **Where (user):** the undo bar after closing ("House closed", Undo
   reopens it), and `coffer accounts reopen <id|name>` later; `coffer accounts close
   <id|name> [--on YYYY-MM-DD]` too, and `accounts list` gains a CLOSED column. No panel
   UI for closed accounts.
5. **Deleting (`account::delete`, user: built now)** is for an account added by mistake:
   refused (`Undeletable(DeleteLock)`) while it has transactions, imports (an undone
   batch still names it), pots, rules or schedules, or a lock. Its entered balances or
   valuations go with it, in one DB transaction; `deletion` counts them first, for the
   confirmation. Not undoable. "Delete account…" shows on any kind whenever core allows
   it (user), so on a new empty bank account too.
6. **The app (`crates/app/src/accounts/`).** The panel's ASSETS AND LIABILITIES section
   always shows, its `+` opening New account set to Asset (user: label and `+` only when
   empty). New account for an asset or liability: Value (Owed) with its date and a Note;
   no Bank. Add balance is also Add valuation (composed, user) and the drawn liability
   version. While an asset or liability is selected, Transactions shows
   `valuations::ValuationsView` instead of its table (the hook is one branch in
   `TransactionsView::render`): the toolbar, the step-line chart (a GPUI path; the scale is
   `valuations::Chart`, tested against Paper's positions) and the table. ⌘N there and in
   the panel adds a valuation; on other accounts the key goes on (M3.10's). The inspector
   (no rows under Valuations: the canvas lists them, as `BSI-0` draws it) ends with the
   footer. The Close dialog (drawn for an asset; composed for the other kinds, user) and
   the Delete dialog (composed, user) open over the shell. Words for a liability:
   "Balances", "Add balance", "Owed" (user).
7. **Open.** An import into a closed account is still allowed (the pot mirror already
   writes to a closed pot, M1.17); refusing rows dated after `closed_on` at import is left
   for a later decision. Editing or deleting a single valuation isn't built (not drawn;
   point 21 still lists it).

### Decision record: cash counts (built) (2026-11-15)

M3.10, the cash half. Follows `DESIGN.md` → "Accounts you keep yourself" (D6.9) and
"Decision record: accounts you keep yourself", point 5. **No migration.**

1. **A cash count is a `manual` entered balance on a cash account**, checked like any
   balance; the first is the starting point (review queues, point 1). Other kinds are
   refused (`account::count`).
2. **What Coffer expects** is the last passing balance (or the starting point) plus the
   transactions after it, up to and including the count's day; a failed count is not a
   base.
3. **"Add −£5.00 and save"** writes the count and a manual "Cash count difference" row in
   Other on the count's day, in one DB transaction (`manual::create_in`), opened
   `IMMEDIATE` so the difference is worked out from what the database holds when it
   writes. If the user has deleted Other (M3.7), the row is left uncategorised rather than
   refused. Coffer never posts a difference on its own: Enter and "Save count only" add
   nothing.
4. **Guards** are add-balance's and manual-create's: no count or difference on or before
   the lock date or after closing; a negative count is refused (composed).
5. **The dialog** is the cash variant of Add balance, opened through the same
   `AccountsEvent::AddBalanceRequested`. Its title, captions, the "Starting point" row and
   the first count's single "Save count" are composed.
6. **Open.** The expectation ignores a count already entered after the chosen day.
