# Orre — Architecture: Transactions

The Transactions view and what the user can do to a row: splits, tags, notes, merchant names, manual rows, exclusions, transfer pairs, removal.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Transactions

What a user can do to a row, in core: `manual`, `exclusion`, `split`, `tag`, `note`,
`merchant`, `edit` (the inspector's edits), `transfer` (detection, decisions, marking),
`removal`, `category::assign` and `inbox::own::recategorize`. The Transactions view reads
`reporting::register`; the app's side is `crates/app/src/transactions/`. The records
below are the history; this section is the rule in force.

### The row

- **Origin** (`transaction.origin`): `imported`, `mirrored` (a pot's side of a transfer,
  made by the import: "Decision record: sub-accounts") or `manual`. Every row has a UTC
  `created_at`.
- **Source fields never change** on an imported or mirrored row (a schema trigger):
  account, batch, date, time, amount, currency, merchant, descriptions, external ID,
  fingerprint, source category and type, original amount and the bank's `note`. A wrong
  imported row is fixed by undoing its batch, excluding it, or an import of the bank's
  correction ("Decision record: a bank's correction supersedes the stored row").
- **User-facing fields** can change on any row: category, display merchant, user note,
  tags, split, share and its label, `reimbursable`, exclusion, links and transfer status.
  A manual row's own fields (`manual::ManualTransaction`) can change too.

### Edits, undo and the lock

| Edit | Core | Undo | Inside a lock |
|---|---|---|---|
| Category | `inbox::own::recategorize` | `bulk_edit` change batch | allowed |
| Split, tags, note, display merchant | `edit::set_split`, `remove_split`, `add_tag`, `remove_tag`, `add_new_tag`, `set_note`, `set_merchant` | one `bulk_edit` batch each | allowed |
| Exclude, include again | `exclusion::exclude`, `include_again` | the opposite call, no batch | only if `exclusion::moves_balance` is false |
| Transfer decisions, marking | `transfer::confirm`, `reject`, `unpair`; `inbox::mark_transfer` | `transfer::undo`, `unmark`, no batch | allowed |
| Manual rows | `manual::create`, `update`, `delete` | none | see "Manual rows" |
| Rule applies, merges, bulk edits | `change::Recorder` | `change::undo` | refused only where `Value::moves_balance` holds |

An edit that changes nothing records nothing. A single-row edit shows no undo bar (it is
undone from the change history); link edits, changes to several rows, a new exclusion
(`UndoAction::Include`, since the row leaves the table) and transfer decisions
(`UndoAction::Transfer`) do (`app.md` → "The app" → "Applying edits"). A lock guards balances only ("Decision record: a lock guards
balances only"): on or before `reconciled_through` nothing may change which rows count in
the balance, or such a row's account, date or amount.

### Which rows take a category

- **A row takes a category of its own unless** it is split, a leg of a confirmed
  transfer pair, or the money in of a confirmed refund (`category::assign`,
  `CategoryFacts`; "Decision record: what a transaction can take, decided in core").
  Suggested pairs and links don't count.
- **Who asks:** `recategorize` (the table, the inspector, the bulk-action bar and
  `orre categorize`) refuses such a row (`RecategorizeError::NoOwnCategory`), even with
  the same category. An approved categorisation skips it. Rules skip its category and
  apply their other actions (`rule::SkippedCategory`; "Decision record: two kinds of
  transfer, and marking a suggested pair", point 3). Duplicate merges, removing a split
  and undo don't ask; `manual::update` refuses a category only on a split row.
- **Needs a category** (`CategoryFacts::needs_category`): takes one and has none, or is
  split with a line that has none. The Uncategorised chip
  (`RegisterRow::is_uncategorised`) and the import summary (`needs_category_in_batch`)
  use it. `Ledger::uncategorized`
  (MCP) filters ledger lines instead ("Decision record: removing transactions, and which
  rows need a category", points 3 and 4).
- **The rule prompt** after a category edit shows only when the pattern matches an
  earlier row; "Not now" is remembered per pattern ("Decision record: Transactions
  additions", point 3).

### Splits, tags, notes and names

- **Splits** (`split`): two or more lines, each non-zero with the parent's sign and
  currency, summing exactly to its amount; a line may be uncategorised. Setting a split
  replaces every line and clears the parent's category, `rule_id`, share and label;
  removing it leaves the row uncategorised. `reimbursable`, notes and the exclusion stay
  on the parent. A row in a suggested or confirmed transfer, or the money in of a
  confirmed link, can't be split; an excluded row can. The editor's model is
  `split::draft::SplitDraft`, saved only when balanced.
- **Tags** (`tag`): trimmed, case kept, at most 64 characters, unique ignoring case with
  rule matching's folding. Private tags never leave the app: MCP and exports read
  `tag::public_tags_of` or `without_private`.
- **Notes** (`note`): `user_note` is trimmed, at most 2,000 characters, blank clears; the
  bank's `note` is read-only.
- **Display merchant** (`merchant`): derived from the bank's merchant or raw description
  by `merchant::clean`, then the aliases (exact, longest prefix, first regex). Alias
  changes recompute derived names at once, with no batch. A name a rule or the user set
  (`merchant_display_source`) is never recomputed; a blank user name goes back to the
  derived one. Rules match the cleaned merchant, never the alias.

### Exclusion

`exclusion::exclude` works on any row, with a reason, an optional note (required for
`other`) and a date; excluding again changes the reason and resets the date. Every
excluded row leaves spending, income, budgets and reports; only `duplicate` and
`superseded` also leave the account's balance (`ExclusionReason::leaves_balance`).
`superseded` is set only by an import and cleared only by undoing it, so the user can't
choose it (`ExclusionReason::CHOOSABLE`) or change a superseded row. The Exclude
dialog's reason starts empty. Excluded rows stay visible under the Excluded chip.

### Manual rows

`manual` creates, edits and deletes `origin = 'manual'` rows only, on any account that
holds transactions (never `asset` or `liability`). They carry no source ID or
fingerprint, so near-duplicate review, not the import, pairs one with the bank's row.
Core needs a description; the app's Merchant field is required and is also the
description.

- **Lock:** creating or deleting a row on or before the lock, or changing its account,
  date, amount or currency (old or new date, old or new account), is refused; its time,
  merchant, description and category can change there.
- **Closed account:** no row dated after `closed_on`.
- **Refused edits:** a new account, amount or currency on a row in a suggested or
  confirmed pair (`manual::Error::Transfer`; unpair it first), a new amount or currency
  on either side of a confirmed link (`Linked`), and a new amount or a category on a
  split row (`Split`).
  A `rejected` group the row was in is dissolved.
- **Deleting** goes through `removal::remove`. Only manual rows can be deleted; imported
  rows are excluded instead.

### Transfers

- **Two mechanisms.** A confirmed pair is money between two accounts Orre holds: both
  legs are out of income and spending and take no category. A category of kind
  `transfer` is for one row whose other side Orre doesn't hold; it counts as neither
  (`reporting::ledger::Side::of`). A suggested pair counts by its own categories until
  confirmed, and the table marks its legs with a `warning` ⇄ ("Decision record: two
  kinds of transfer, and marking a suggested pair").
- **Decisions** (`transfer::confirm`, `reject`, `unpair`) change only the group's status,
  re-checked in `BEGIN IMMEDIATE`. Confirming is refused while a row is excluded, split
  or linked. A rejected pair keeps its rows, so detection doesn't suggest it again. A
  pot's mirrored pair can't be unpaired. No lock applies, and an agent's pending
  suggestion for the pair is settled in the same DB transaction
  (`inbox::decide_transfer`).
- **Mark as transfer** (`transfer::markable`, `inbox::mark_transfer`): exactly two rows
  on different accounts, one currency, opposite non-zero amounts, neither in a suggested
  or confirmed pair, excluded, split or linked. A row may leave a rejected group to be
  marked ("Decision record: Mark as transfer and Create rule from the bulk-action bar").

### Removing transactions

`removal::remove` is the one place rows are deleted, for `manual::delete` and
`import::undo`: it dissolves their transfer groups (partners stay as ordinary rows),
clears every table in `removal::REFERENCES` (a test compares it with the schema), restores
a row they superseded, and deletes them, inside the caller's DB transaction.

### The register

`reporting::register::Register` reads every transaction once, with its split lines, pair,
links, tags, source and exclusion, on a background thread; `Register::select` filters,
groups by day and sorts in memory, so typing never reads the database. Day nets leave
out transfers and excluded rows. `RegisterRow` says what a row takes (`splittable`,
`takes_share`, `takes_links`, `no_own_category`), so the app offers only what core
accepts. Search matches the display merchant, the bank's merchant and the raw
description; that text is only matched and shown ("Decision record: the Transactions
view").

## Decision records

Oldest first.

### Decision record: Transactions additions (2026-11-10)

The open questions from the D6.3 designs (`DESIGN.md` → "Transactions additions"),
answered on the designer's recommendation at the user's request:

1. **Exclusion has a reason key, a note and a date.** The reason is one of `duplicate`,
   `bank_error` (the bank's mistake, such as a double charge), `disputed` (a charge
   being contested, including fraud) and `other`. `excluded_note` holds optional user
   text, which is required for `other`; it's treated like `user_note` and isn't exposed
   over MCP by default. `excluded_at` gives the inspector its date. A fixed key keeps
   reports and filters exact, and an aggregate client can count exclusions without
   seeing any text. Exclusion is a field on `transaction`, not a one-item
   `change_batch`: the filter and the inspector read it on every row, and "Include
   again" is the undo. Multi-row exclusions (duplicate merges, a rule applied to
   history) still go through `change_batch`.
   - **Balances depend on the reason.** Every excluded row leaves spending, income,
     budgets and reports. Only `duplicate` also leaves the account's computed balance,
     because a duplicate never happened at the bank. The other reasons record money that
     really moved: it stays in the balance until the correcting refund arrives, which is
     linked to the charge (`transaction_link`). Without this rule, excluding a double
     charge would break reconciliation. Excluding or including a `duplicate` row in a
     locked window is flagged like any other change there.
2. **Every transaction has `created_at`**, not only manual ones. It's a UTC timestamp set
   on insert. It costs one column and also serves "recently added" and support
   questions. Edits need no `updated_at`, because `change_item` and the import batches
   already record them.
3. **The rule prompt needs at least one earlier match.** "Create a rule from this
   edit?" shows after a category edit only if the proposed pattern matches at least one
   earlier transaction, so the merchant has been seen twice. A rule from a single
   sighting is a guess, and prompting on every one-off merchant would teach the user to
   ignore the prompt.
   - **When it doesn't show:** for transfers, split rows, or rows a rule already
     matched (D6.7 handles changing an existing rule).
   - **"Not now"** is remembered for that pattern, so the inline prompt doesn't return.
     The passive route stays: after repeated identical recategorisations, the app files
     a `rule` suggestion to the Inbox (source `orre`; see "Categorization").

### Decision record: splits, tags and notes, manual rows, merchant names (2026-11-12)

M1.20–M1.23 (M1 wave 3). Migrations `V5__transaction_split.sql`, `V6__tags.sql` and
`V7__merchant_alias.sql`; M1.22 needed none. Points 1 and 2 were decided by the user.

1. **A new, edited or deleted merchant alias applies at once, to derived names only.**
   `transaction.merchant_display_source` (V7) is NULL when the name is derived (the
   bank text cleaned, then the aliases), `rule` when a rule action set it (M1.24) and
   `user` when the user did (`merchant::set_user_name`). Alias changes recompute every
   derived row in the same DB transaction, with no batch (rules decision point 5), and
   never touch a `rule` or `user` name. A set name can't be blank.
2. **Manual rows are allowed on every account that holds transactions**: current,
   savings, credit card, pot and cash; never on `asset`/`liability` (accounts decision
   point 2). A manual row has no dedupe keys, so one entered ahead of the bank's export
   is matched later by near-duplicate review, not by the import.
3. **Splits** (`orre_core::split`). Two or more lines, numbered from 1, each non-zero
   with the parent's sign, summing exactly to the parent's amount in its currency; lines
   may be uncategorised. Setting a split replaces all lines (their IDs change) and clears
   the parent's category, `rule_id`, share and share label: each line carries its own.
   `reimbursable`, notes and the exclusion stay on the parent. Removing a split leaves the
   row uncategorised. The schema guards what one row can: while a transaction has lines
   it takes no category or share and its amount and currency don't change (a manual
   row's edit is refused in words first, `manual::Error::Split`). A row in a suggested
   or confirmed transfer can't be split, a group holding a split row can't become one,
   and transfer detection skips split rows. A row in a `rejected` group, or an excluded
   row, can be split. Applying rules to history (M1.24) must skip split rows.
4. **`ledger_line` doesn't filter.** It has every transaction (or its lines), with
   `excluded_reason`, `transfer_status` and `my_amount_minor` (the share, or the whole
   amount) for each report to filter and sum by; the balance, the excluded filter and
   transfer views read the same rows.
5. **Tags** (`orre_core::tag`). Names are trimmed, keep their case, are at most 64
   characters with no control characters, and are unique ignoring case with the same
   folding as rule matching (`Straße` and `STRASSE` are one tag; SQL `NOCASE` backs it up
   for ASCII). `rule::TagList` uses the same `TagName`, so an `add_tag` value repeating a
   name in another case is refused. Adding a tag twice does nothing; deleting a tag
   removes its links. `transaction_tag` is a `WITHOUT ROWID` link table whose rows are
   only inserted or deleted. Private tags never leave the app: anything that does (MCP,
   exports) reads `tag::public_tags_of` / `without_private`.
6. **User notes** (`orre_core::note`). Trimmed, line breaks kept, at most 2,000
   characters (checked in Rust); blank clears. Any origin can carry one; the imported
   `note` stays read-only.
7. **Manual rows** (`orre_core::manual`). Create, read, edit (every field the entry
   holds; other user fields stay) and delete `origin = manual` rows; imported and
   mirrored rows are refused. Locks guard balances only (rules decision point 8): a
   change to the account, date, time, amount, merchant or description, or a create or
   delete, on or before `reconciled_through` (old or new date) is refused; a
   category-only edit there isn't. A row can't be dated after its account's
   `closed_on`. Changing a paired row's account or amount, or deleting it, dissolves its
   transfer group. Deleting clears its split lines and tag links first
   (`manual::release_references`, where later referencing tables join).
   *Amended 2026-11-19 by the user:* a lock guards balances only. Creating or deleting a
   manual row, or changing its account, date, amount or currency, on or before
   `reconciled_through` (old or new date) is refused; its time within the day, merchant,
   description and category can be edited there. See `links-and-checks.md`, "Decision
   record: a lock guards balances only".
8. **Exclusion** (`orre_core::exclusion`) applies to any row, with the reason, note
   and date of "Transactions additions" point 1. Excluding again changes the reason and
   resets the date. Only a change into or out of `duplicate` is refused inside a lock,
   since only `duplicate` moves the balance among the reasons a user can choose;
   `superseded` (set only by an import, cleared only by undoing it: import.md, "a bank's
   correction supersedes the stored row") also leaves the balance. *Amended 2026-11-19:*
   the rule is `exclusion::moves_balance`, and change batches (rule applies, merges, bulk
   edits and their undo) now follow it too, in place of refusing every exclusion change.
   See "Decision record: a lock guards balances only".
9. **Merchant clean-up** (`merchant::clean`) is deterministic and keeps the bank's case
   (title-casing gets McDonald's, IKEA and eBay wrong and can't be undone; comparisons
   ignore case). It strips processor prefixes (`SQ *`, `SUMUP *`, `IZ *`, `ZETTLE_`,
   `PAYPAL *`, `CRV*`, `SP `…), payment words (CONTACTLESS, CARD PAYMENT, APPLE PAY…),
   card references and masked numbers, and trailing dates, times, references and long
   digit runs, repeating until nothing changes; it never empties a name. The source text
   is the bank's merchant, else the raw description. `SP ` also strips a real name that
   starts with "SP "; an alias fixes such a case.
10. **Aliases** match the cleaned text ignoring case and runs of whitespace: `exact`, then
    the longest `prefix` (a trailing space marks a word boundary; ties go to the older
    alias), then the first `regex` by ID (oldest first; no order column yet: adding one
    is a plain `ALTER TABLE`). Regexes are validated like rule patterns. Duplicate
    aliases are refused. `merchant_alias.created_at` gives the inspector its date.
11. **Rules match the cleaned merchant.** `MatchField::Merchant` compares
    `clean(merchant)`, as the rules decision said, so names never change which rules
    match. This changed existing behaviour: a merchant rule written against processor
    noise (`starts_with "SQ *"`) no longer matches; one written against the clean name
    now matches the noisy rows. The description field is unchanged.
12. **Names run before rules in the import commit** (after the pot mirror, so mirrored
    rows are named too), so a rule's `set_merchant` (M1.24) has the last word. Opening a
    database from before V7 names every existing row once, right after the migrations,
    in its own DB transaction (refinery's can't run Rust); if that fails,
    `merchant::recompute_all` repairs it.
13. **Undo and deletes clear what references a transaction.** `import::undo` and
    `manual::delete` delete split lines (`split::clear_batch` / `clear`) and tag links
    (`tag::clear_batch` / `clear`) before the rows. `undo_preview`'s `edited` count
    includes rows with a split, tags or a user-set name.

### Decision record: the Transactions view (2026-11-14)

M3.3. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build
(each was the recommended option). Everything else follows `DESIGN.md` → "Transactions"
and "Data table (Transactions)", or an earlier record.

1. **A read model of its own, in core (`reporting::register`).** The reports' `Ledger`
   keeps only what counts (no transfers, no excluded rows, one line per split), so the
   table reads every transaction once into a `Register`: one `RegisterRow` per
   transaction with its split lines, its confirmed-transfer partners, its exclusion and
   the accounts and categories it names. `Register::select` is pure: it filters, groups
   by day (each day's net leaves transfers and excluded rows out, per currency) and
   sorts. A person's ledger fits in memory, so the app reads the register on a
   background thread at start and after each change, and filters in memory: a
   keystroke in the search field never reads the database. No migration and no FTS5
   index (`research/crates.md` suggested one): substring matching in memory covers it.
2. **Category colours are ranked when the register is read (user).** No colour is
   stored on `category` yet. The six top-level groups with the most spending over the 12
   months to today (the user's share, `personal_minor`) take `chart-1` … `chart-6`, in
   order; their subcategories share the colour; everything else, income, transfers,
   groups with no spending and the `other` group, is the "Other" marker. Amounts in
   different currencies are never added: the ranking uses the currency with the most
   spending lines. M4.1 stores the colours so charts and the table agree.
3. **Category edits are core's `inbox::own::recategorize`**, as `orre categorize`:
   one DB transaction, one undoable `bulk_edit` change batch, category source `user`
   (so repeated identical edits can still file Orre's own `rule` suggestion). The
   table's in-place select (Enter, or a double-click on the category), the inspector's
   Category select and the bulk-action bar's Categorise all take that path; a split row,
   a confirmed transfer or a linked refund's money in isn't offered one, and since
   2026-11-16 core refuses it too (decision record "what a transaction can take") (split lines are set in the split editor,
   M3.9; transfers are M3.8). After a change to several rows the undo bar (`53O-0`)
   offers `change::undo`; a single edit is undone from the change history (M3.13). The
   fixture can't change: an edit there says so under the Category select.
4. **Filters.** The accounts panel's selection is the account filter, and the Account
   chip mirrors it ("All accounts" clears the panel). The Category chip takes one
   category; a group includes its subcategories, and a split row matches when any line
   does. The Date chip's presets (this month, last month, the last 3 months, this year,
   all time) cover whole periods, so a later row this month still shows. "Uncategorised"
   means no category and not a confirmed transfer, or a split line without one; its
   count follows every other filter. Search matches the display merchant, the bank's
   merchant and the raw description, ignoring case; the text is only matched. Excluded
   rows stay hidden until the Excluded chip (M3.10). Escape clears the selection, and
   with nothing selected, the search and every filter.
5. **Sorting stays within each day (user).** A click on a column title sorts each day's
   rows by it (merchant, category, account A–Z; amount smallest first, so the largest
   payments out lead), a second click reverses, a third goes back to newest first. Day
   headers and nets never move.
6. **Composed states, not drawn (user; `DESIGN.md` → "Transactions" → "Built in
   M3.3").** The category menu is the Kind menu with the marker in the icon slot, each
   group under its caps label (its own category first), and a filter field at the top;
   the chip menus are the same menu; a filtered chip and the Uncategorised chip when on
   take the Excluded chip's on look; no match shows the Empty state with "Esc Clear
   filters".
7. **Left for later.** AI suggestions in the table and the inspector's agent card are
   the Inbox's (M3.6, M3.14): approving one is a whole-suggestion change. No row is
   flagged yet: nothing records a per-transaction problem. Mark as transfer, Create
   rule and Import stay inert until M3.8, M3.13 and M3.4. The left panel's CATEGORIES
   list and the status bar still show the fixture (both read the database since: the
   status bar with M3.10 and M3.12, CATEGORIES on 2026-11-17, point 8).
8. **The panel's CATEGORIES (user, 2026-11-17).** Each top-level group that holds
   transactions, with how many, all time and in every account (core's
   `Register::category_counts`, counted as the Category filter keeps rows, so a count
   is what picking it shows; excluded rows left out), most first, Other last, then
   Uncategorised. Clicking a row opens Transactions filtered to it (a group includes its
   subcategories; Uncategorised and a category replace each other); on Transactions the
   picked row clears it. `+` opens Rules → Categories on a new category. Composed from
   existing specs, not drawn.

### Decision record: splits, tags and notes in the inspector (2026-11-14)

M3.9. No migration. Marked **(user)**: decided by the user on 2026-11-14 when asked
before the build (each was the recommended option). Everything else follows `DESIGN.md`
→ "Transactions additions" and the decision record "splits, tags and notes, manual rows,
merchant names".

1. **Each inspector edit is one undoable change batch.** Core's `edit` module
   (`set_split`, `remove_split`, `add_tag`, `remove_tag`, `add_new_tag`, `set_note`,
   `set_merchant`) runs one DB transaction and records one `bulk_edit` change batch
   through `change::Recorder`, as the table's category edits do
   (`inbox::own::recategorize`), so `change::undo` reverts it and the change history
   (M3.13) will list it. A single-row edit shows no undo bar (the M3.3 rule). An edit
   that changes nothing records nothing. A split goes through `split::prepare`, the same
   checks as `split::set` (not a transfer, not linked money in, lines in the amount's
   sign and currency summing to it exactly, the categories exist), and clears the
   transaction's own category and share first in the same batch.
2. **The split editor's model is core's `split::draft::SplitDraft`.** Remaining is the
   amount less the lines typed so far, signed (`−£4.00` short, `+£3.60` over). The sign
   is never typed: a line takes the transaction's sign. "Add line" fills in the
   remainder when the lines are short, and starts empty otherwise. A draft is saved only
   when balanced (two or more lines, none missing or zero, summing exactly), and then on
   leaving an amount field or Enter, choosing a category, or adding or removing a line.
   An unbalanced draft stays through reloads and is dropped when the row is left, so the
   last balanced split stays. **"Split" starts one line with the whole amount in the
   row's category (user)**; lowering it and "Add line" make the common two-way split.
   Lines keep the share, share label and note they were stored with (the register's
   `SplitPart` carries them, in the app only), so an edit never drops what the editor
   doesn't show yet (M3.11).
3. **The register says what the inspector needs.** `RegisterRow` gains the row's tags
   (by name ignoring case), the bank's imported note, whether the user named the
   merchant (`merchant_set`) and whether the row can be split (`splittable`: not in a
   suggested or confirmed transfer, not the money in of a confirmed link). A row that
   can't be split has no Split section rather than a button that would fail. The
   register lists every tag (`Register::tags`, with its privacy) for the tag menu.
4. **Tags (user).** The "+ Tag" menu is the category menu's pattern: every tag A–Z,
   ticked when on the row (choosing one toggles it), a private one with the lock; when
   the typed text isn't a tag's name (ignoring case, with the same folding as tag
   names), "Create “…”" and "Create “…” as private". Privacy is chosen at creation and
   changed later in the tag editor (M3.7). Creating a name another tag has links that
   tag and keeps its privacy. A pill's ✕ on hover removes the tag too (user). Undoing
   the batch unlinks the tag; the tag itself stays.
5. **The display merchant (user: built now, not in M3.13).** A typed name is the
   user's (`merchant_display_source = user`) and no recompute changes it; a blank field
   goes back to the derived name (a rule's name is dropped too). The field is empty for
   a derived name and shows that name as its placeholder.
6. **Text saves on blur, Enter and leaving the row.** Merchant and Note save when the
   field loses focus, on Enter, and when the selection moves on (what was typed is
   kept, as the account inspector does). A field being typed in isn't overwritten when
   the register is read again. The bank's note is read-only, shown above Note only when
   the import carried one. Notes, names and tag names are shown as plain text and only
   ever matched, never acted on.

### Decision record: excluding, the Source section and manual transactions (built) (2026-11-15)

M3.10, the Transactions half. Follows `DESIGN.md` → "Transactions additions" (D6.3) and
"Decision record: Transactions additions" above. Marked **(composed)**: not drawn, composed
from existing specs (review). **No migration.**

1. **Exclusion stays fields on the row**, as "Transactions additions" point 1 decided.
   "Include again" is its undo, and after a new exclusion the undo bar offers it
   (`UndoAction::Include`): the one single-row edit with an undo bar, because the row
   leaves the table while the Excluded chip is off (composed).
2. **The register says where each row came from and why it's excluded.**
   `RowSource::Import { batch, file, imported_on, origin }` or `Manual { created_on }`,
   plus `excluded_note` and `excluded_on`; days are local, read through SQLite's
   `localtime`. `RegisterAccount` carries its currency (a new manual row's).
3. **The status bar** counts transactions that count, plus "· N excluded" when there are
   some; the sample data keeps the artboard's 328.
4. **Undo import…** opens a confirmation composed from `import::undo_preview` and the
   CLI's wording, with a destructive confirm, since undoing removes rows (composed).
5. **The Exclude dialog's Reason starts empty**, so the user always decides whether the
   balance changes (of the reasons it offers, only `duplicate` leaves it; `superseded` is
   never offered). "Edit reason…" reuses the dialog, filled
   in (composed).
6. **Manual rows.** Merchant is required: it is also the row's description, which core
   needs (composed). Date, Amount and Account save through `manual::update`; Account
   offers every account that holds transactions. Only manual rows can be deleted (with a
   confirmation); imported rows are excluded instead.
   *Added 2026-11-15 (review follow-up):* a new account or amount on a row in a suggested
   or confirmed transfer pair is refused (`manual::Error::Transfer`) instead of
   dissolving the pair silently; the user unpairs it or marks it as not a transfer
   first, an undoable decision (M3.8), and the edit then goes through. A `rejected`
   group the row was kept in is still dissolved by the edit (nothing the user sees
   changes). Refusing, like the split and link refusals beside it, keeps the edit one
   DB transaction with nothing to undo behind it. An edit saved as the user leaves the
   row that fails is said by a failure bar with Show (`DESIGN.md`, "Built in M3.10").
7. **⌘N** is the shell's `NewTransaction`: from Transactions, and from the accounts panel
   on an account that holds transactions (a valuation account's ⌘N adds a valuation,
   M3.17). Changes that move a balance emit `TransactionsEvent::LedgerChanged`, so the
   accounts panel and the checks read again; the shell also reloads Transactions after
   any dialog closes (a cash count may add a row).
8. **When "New transaction" would overflow the toolbar**, the search field gives up the
   pixels (composed).
9. **A row the table is asked to show** (a new manual row, a row included again) is
   shown: `Register::filter_showing` clears only the filter parts that hide it, then it
   is selected (composed, review follow-up).

### Decision record: transfer-pair confirmation (built) (2026-11-15)

M3.8. The Transactions inspector's Transfer section is composed from existing specs, not
drawn (user, 2026-11-15): the Inbox transfer pair's preview (D6.6, `EWZ-0`), the Links
section's rows and Orre's refund card (D6.5) and the undo bar (D6.3). Marked
**(composed)**: decided while building. **No migration.**

1. **A decision is a status change on the detected group** (`transfer::{confirm, reject,
   unpair}`): suggested → confirmed; suggested → rejected ("Not a transfer"); confirmed →
   rejected (unpair). Rejected rows stay in the group, so detection never suggests them
   again (composed).
2. **Each decision re-checks** the status and rows in `BEGIN IMMEDIATE` and refuses on a
   mismatch, so a stale screen changes nothing.
3. **Confirming is refused** while either row is excluded, split or linked, as an Inbox
   approval refuses (`Blocked`). Rejecting has no row checks (composed).
4. **A pot's mirrored pair can't be unpaired**: the import made both rows from one bank
   row, so they are one movement of money, not a guess. A pot paired by detection is an
   ordinary pair (composed).
5. **Undo is the inverse status change**: no change batch, so it isn't in Change history.
   It refuses if the pair or its rows changed, and restoring re-checks excluded, split and
   linked rows (composed).
6. **Nothing else changes**: no category, amount or balance, and no lock date applies;
   only income and spending read the status.
7. **The inspector reads the register** (`RegisterRow::pair`, `reporting::pairing`), and
   whether each action is allowed comes from `transfer::availability`, the same checks the
   write runs (composed).
8. **An agent's pending suggestion for the same pair is settled** by the decision, in the
   same DB transaction (`inbox::decide_transfer`): approved when the user confirms,
   rejected ("Decided in Transactions") when they say it isn't a transfer. Unpairing
   settles nothing; undoing a decision leaves the suggestion decided (composed, lead).
9. **The section** sits after Details, in place of Share and Links. Suggested: "Is this a
   transfer?", "Same amount out and in, the same day", the other side as a `field` strip
   ("From Current account", amount without a sign), "Confirming leaves both out of income
   and spending. Balances don't change.", outline "Confirm transfer" and ghost "Not a
   transfer", with the reason in place of the caption when confirming is blocked.
   Confirmed: one row ("To Savings", date, amount, ✕ to unpair) and "Not counted as
   income or spending."; a pot's pair says to undo the import instead of a ✕. The undo
   bar reads "Confirmed transfer: Current account → Savings", "Marked as not a transfer"
   or "Unpaired transfer" (composed).
10. **Open.** The table doesn't mark a suggested pair's rows yet, and there is no "Pair
    with…" for rows detection didn't pair; both need a design. (The bulk-action bar's
    "Mark as transfer" now pairs two rows by hand: "Mark as transfer and Create rule
    from the bulk-action bar" below.)
    *Amended 2026-11-19: the table now marks a suggested pair's rows ("two kinds of
    transfer, and marking a suggested pair" below). "Pair with…" is still open.*

### Decision record: removing transactions, and which rows need a category (2026-11-16)

From an architecture review of `crates/core`. Nothing the user sees changes except point
3's count.

1. **One module removes transactions** (`core::removal`, crate-private).
   `removal::remove(conn, Scope::Row(id) | Scope::Import(batch))` dissolves the rows'
   transfer groups (their partners stay as ordinary transactions), deletes every row
   that refers to them, then deletes the transactions, all inside the caller's DB
   transaction. `manual::delete` and `import::undo` call it and keep only their own
   checks and steps (manual-only and lock date; balances, flags and the batch's
   status). `removal::dissolve_transfer_group` is also `manual::update`'s.
2. **The list of what refers to a transaction is checked against the schema.**
   `removal::REFERENCES` names every `(table, column)` with a foreign key to
   `"transaction"` (split lines, tag links, change items, both sides of a link, both
   sides of a duplicate candidate, schedule matches), and a test compares it with the
   live schema's `pragma_foreign_key_list`. A new table that points at a transaction
   fails that test until it is listed, instead of failing a delete at run time. The
   per-module `clear`/`clear_batch` functions are gone, except `split::clear`
   (crate-private), which setting, removing and undoing a split use. Supersedes the
   per-table clears of "splits, tags and notes, manual rows, merchant names", points 7
   and 13.
3. **Whether a transaction still needs a category is decided by
   `category::assign`**, from the same facts as whether it takes one
   (`CategoryFacts::needs_category`): it takes a category of its own and has none, or
   it is split and a line has none. A confirmed transfer leg or a confirmed refund's
   money in never needs one. The Transactions table's Uncategorised chip and its count
   (`RegisterRow::is_uncategorised`) and the import summary's count
   (`needs_category_in_batch`, from the same facts SQL as `facts_in`) use it, and the
   parity test runs both readers. Before, a confirmed refund's money in with no
   category of its own was counted as uncategorised, though its purchase's category
   counts for it and the table offers no category for it. Supersedes the rule spelled
   out in "the import flow", point 5, for `CommitSummary::uncategorized`. A partly
   refunded money in is the same: its uncategorised remainder still shows in reports
   and `uncategorized_transactions` (point 4), but not under the Uncategorised chip,
   where it couldn't be categorised before either.
4. `Ledger::uncategorized` (MCP's `uncategorized_transactions`) is a filter over
   ledger lines, not transactions, and is unchanged: a fully refunded money in's own
   line nets to zero and isn't listed; a partly refunded one's uncategorised remainder
   is.

### Decision record: Mark as transfer and Create rule from the bulk-action bar (2026-11-18)

The bulk-action bar's "Mark as transfer" and "Create rule" were drawn (D3, `112-0`) but did
nothing. `DESIGN.md` → "Bulk actions" draws only the buttons, so both are composed from
what the inspector already does (lead, 2026-11-18). Marked **(composed)**. **No
migration.**

1. **Mark as transfer is enabled for exactly two selected rows that make one transfer**
   (`transfer::markable`, on plain values so the view answers it from the register):
   different accounts, one currency, opposite amounts that aren't zero, neither in a
   suggested or confirmed pair, excluded, split or linked (what Confirm refuses, point 3
   of "transfer-pair confirmation"). Dates, closed accounts and the transfer window
   aren't checked: the user knows the pair. Otherwise the button is disabled; it doesn't
   say why yet, as there's no tooltip pattern for a toolbar button.
2. **A row in a rejected group may be marked.** The register doesn't show rejected
   groups, and "Not a transfer" or Unpair shouldn't be final. When the two rows are one
   rejected pair, that group is confirmed again; otherwise a new confirmed group is made
   and each row leaves its rejected group, its old partner staying there so detection
   still doesn't suggest it. That partner is then alone in a rejected group, and stays
   there if the marked row is later removed (undo-import dissolves only the removed rows'
   groups): detection won't pair it again, though it can still be marked by hand, and
   an export lists it with `transfer_status` `rejected` and no other side. Accepted for
   now; cleaning up lone rejected groups would need undo to recreate them.
3. **The write** (`inbox::mark_transfer` over `transfer::mark_in`) re-checks in `BEGIN
   IMMEDIATE` and, in the same DB transaction, settles an agent's pending suggestion for
   exactly that pair, as Confirm does (point 8 of "transfer-pair confirmation").
4. **Undo** (`transfer::unmark`) reverses it exactly: the rejected pair's group back to
   rejected, or the new group removed and each row back in the group it left. It refuses
   if the pair has changed since. As with transfer decisions, there is no change batch,
   and settled suggestions stay decided. The undo bar reads "Marked as transfer: Current
   account → Savings" (money out first).
5. **Create rule** runs `rule::rule_from` on every selected row and opens Rules on the
   unsaved preview (`TransactionsEvent::CreateRule`, as "Create rule from this" does) only
   when every row gives the same rule: same field, pattern and category. Otherwise it says
   why under the selection's Category select. It is enabled whenever several rows are
   selected, since knowing whether they agree needs a database read.

### Decision record: two kinds of transfer, and marking a suggested pair (2026-11-19)

From the architecture review (R.10): "transfer" had two sources of truth, and a
suggested pair counted as income and spending with nothing in the table to say so.

1. **The two mechanisms.** A *confirmed pair* is money between two of the user's
   accounts where Orre holds both sides. Both legs are out of income and spending
   whatever their category, and take no category of their own ("what a transaction can
   take, decided in core", point 1, in `categories.md`). A category of kind `transfer` is
   for a single row whose other side Orre doesn't hold, such as an account that isn't
   in Orre. It counts as neither income nor spending (`reporting::ledger::Side::of`).
   A *suggested pair* is only a guess: it counts by its own category until it is
   confirmed, which is why the table marks it.
2. **The marker (composed, user 2026-11-19).** A row that is a leg of a suggested pair
   shows the ⇄ glyph in `warning` beside the merchant, in the slot a confirmed
   transfer's glyph uses. DESIGN.md maps "unconfirmed transfer pair" to `warning`. The
   row isn't dimmed and its category cell keeps the row's own category, because the row
   still counts. Hover text: "Suggested transfer: counted as income or spending until
   you confirm it". An excluded leg isn't marked, since it doesn't count. A confirmed
   pair keeps its look, and a rejected pair shows nothing.
   The status comes from `RegisterRow::pair`. DESIGN.md's Transactions row table has it
   as "Suggested transfer" (composed, no Paper artboard).
3. **Rules skip the category of rows that take no category of their own** (a confirmed
   transfer's legs, a confirmed refund's money in), as they skip split rows. Other rule
   actions still apply. Decided by the lead, following the user's 2026-11-16 choice;
   built in `crates/core/src/rule`. It answers the question left open in "what a
   transaction can take, decided in core", point 4.
4. **Rejected: counting a suggested pair as a transfer before it is confirmed.** A wrong
   guess would hide real spending from the reports with no visible sign.
