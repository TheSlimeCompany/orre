# Coffer — Architecture: Rules

Rules and their actions, the rules engine at import, transfer detection, the CLI, and the Rules view.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Rules

`coffer_core::rule` (M1.15, M1.24). A rule is a `rule` row and its `rule_action` rows,
made in the Rules view, from the Transactions prompt or by approving an Inbox suggestion
(`rule.origin`: `new`, `edit`, `inbox`). Rules run on import. Past transactions change
only through Apply, never because a rule was edited ("rules", point 4).

### A rule

- **Conditions.** A pattern on one `MatchField` (`merchant`, `description`,
  `source_category`, `source_type`), tested by a `MatchKind` (`is`, `starts_with`,
  `contains`, `regex`), with optional inclusive `AmountBounds` on the signed
  `amount_minor` and an optional account. Every condition must hold.
- **Matching ignores case.** The plain kinds compare text folded per character
  (`rule::fold`: `Straße` equals `STRASSE`); `regex` uses the regex crate's
  case-insensitive mode. The merchant is matched cleaned (`merchant::clean`), before any
  merchant name applies, so names never change which rules match. The description is
  matched with whitespace collapsed. A field the row doesn't have never matches ("rules
  engine, transfer detection and the CLI", point 5).
- **Actions**: `set_category`, `split_template`, `set_merchant`, `add_tag`, `set_note`,
  `exclude`, `my_share`. At least one, one per kind, never both `set_category` and
  `split_template` (`RuleDefinition::new`). `exclude` never takes `other` (a rule has no
  note) or `superseded`. Shares and split lines are integer basis points.
- **Validated in core before storing** ("rules", point 9). A regex compiles within
  `REGEX_SIZE_LIMIT` (1 MiB) and `REGEX_NEST_LIMIT` (32); a pattern is at most
  `MAX_PATTERN_LEN` (1,000 bytes); each `value_json` has one strict shape
  (`RuleAction::decode`). A rule naming a missing category is refused when saved.
  Errors never repeat the pattern or the value.
- **Order.** Priority ascending, then ID. A new rule goes first; `create_at` and
  `move_to` place one, and every write renumbers priorities 0, 1, 2, …. `rule::update`
  keeps the place, origin and date.
- **Rules win field by field** ("rules", point 1; "M1 wave 4 — change batches, rule
  actions, links, duplicates, balance checks and export", point 4). In priority order,
  the first matching rule that sets a field sets it. Category and split are one field;
  merchant, tags, note, exclusion and share are one rule each (`Outcome`).
- **Deleting a rule** (`rule::delete`) keeps the values it set and clears their
  `rule_id` ("rules", point 7).

### What a rule may write

- **Never a value the user set, unless the user includes the row on Apply**
  (`PlannedChange::user_set`): a category with `category_source` `user` or
  `ai_approved`, a name with `merchant_display_source = user`, any note, exclusion or
  share. Notes and shares have no "set by", so one a rule set counts as the user's later
  ("M1 wave 4", point 9). Tags are only added.
- **No category on a row that takes none of its own** (`category::assign`): split, a leg
  of a confirmed transfer pair, the money in of a confirmed refund. The rule's other
  actions still apply; a suggested pair or refund doesn't count ("rules skip rows that
  take no category of their own", point 1).
- **Splits.** A split row is skipped whole. A `split_template` isn't made on a row in a
  suggested or confirmed transfer, on money in with a suggested or confirmed link, or
  when a line would get 0 (`SplitSkip`); the row keeps its category. Shares round half
  away from zero and 10000 bp stores `NULL` (`share_minor`); split lines go by largest
  remainder (`split_amounts`) ("M1 wave 4", points 5–6).
- **Locks.** Only an exclusion into or out of `duplicate`, on a row dated on or before
  its account's `reconciled_through`, meets a lock (`exclusion::moves_balance`, through
  `change::locked_for` into `PlannedChange::locked`); `bank_error` and `disputed` go
  through ("a lock guards balances only", point 4, which settles "rules", point 8). See
  `links-and-checks.md` → "Balance checks and locks".

### At import

`rule::apply_to_batch` runs inside the import's DB transaction, after the preset's
category map, the pot mirror and merchant names, and before transfer detection,
duplicates and link suggestions (`import.md` → "Dedupe, reconciliation and the commit").
It covers every row of the batch, mirrored rows included, and every action. A matching
`set_category` overrides the preset's category (`category_source = rule`, `rule_id`).
The rule set is loaded once; a rule whose category no longer exists fails the import
when it matches a row. The import writes through a `change::Recorder` it never
finishes, so it records no change batch. New rows are never in a locked period. A pot's
mirrored pair is confirmed before the rules run, so neither leg gets a rule's category
("rules skip rows that take no category of their own", point 3). A bank's correction
then takes the user's fields from the row it replaces (`supersede::carry_over`).

### Applying to history

`rule::preview_apply` plans one rule against every transaction it matches, any account
or origin, but never a row a bank's correction superseded. It evaluates the whole rule
set, so the rule changes only the fields it wins. `ApplyPreview` has the `rows` it
changes, `split_rows`, `skipped_splits` and `skipped_categories`; `user_set_rows`
change only when included ("Change these too"). `rule::commit_apply` plans again and
refuses a stale preview (`ApplyError::Stale`), an included row with nothing user-set
(`NotIncludable`) and a change that meets a lock (`Locked`). It records one `rule_apply`
batch naming the rule (`Recorder::for_rule`, `change_batch.rule_id`); tags it created
stay after an undo. A rule not stored yet goes through `rule::preview_draft` and
`save_draft`. Approving an agent's rule in the Inbox applies it to history with nothing
included (`inbox.md`, "Inbox", point 2); `coffer inbox approve` only with
`--apply-to-history`.

### Transfer detection

`transfer::detect_for_batch`, at import after the rules. Two rows pair when they are on
different accounts (pots included), in one currency, with exactly opposite non-zero
amounts, at most `TransferWindow::DEFAULT` apart (3 days, both ends; at most 31), and
neither is excluded, split, in a transfer group of any status, or dated after its
account's `closed_on`. Closest pairs are taken first (days, then time, then IDs), so the
result is deterministic ("rules engine, transfer detection and the CLI", points 2–3).
Each pair is a `suggested` `transfer_group`; detection never changes a category. A
suggested pair counts by its own categories until confirmed; a confirmed pair is out of
income and spending; a category of kind `transfer` is for one row whose other side
Coffer doesn't hold (`transactions.md`, "two kinds of transfer, and marking a suggested
pair"). Confirming, rejecting, unpairing and marking by hand: `transactions.md`,
"transfer-pair confirmation (built)" and "Mark as transfer and Create rule from the
bulk-action bar".

### The CLI

`coffer_app::cli`, in the lib so tests can run it, with `anyhow`; no command edits
rules. `coffer import` uses only a confident, unique detection (OFX by its format, a
preset by its own header row, `Detection::Source`), else lists presets for `--preset`.
`--statement N` picks an OFX statement; `--account` takes an ID or exact name, else the
only open account in the file's currency that holds transactions, pots left out.
Flagged rows, a balance mismatch or a bank's correction need `--yes`; `--dry-run`
writes nothing; `undo-import` without `--yes` only previews. A command that acts as the
user (`import`, `undo-import --yes`, `accounts add`) needs an interactive terminal
(`cli/terminal.rs`; `mcp.md`, "approval needs the user's terminal"). The CLI sets up no
logging and prints no row text it wasn't asked for.

### The Rules view

`crates/app/src/rules.rs` (M3.13): Rules, Merchant names, Change history and Categories
(`categories.md` → "Categories"). Matches is what a rule decides today
(`rule::overview`: the rule set over every stored transaction, as an import would); a
rule that matches rows but decides none is never reached, and `RuleSummary::caught_by`
names the rule above that catches most of them ("the Rules view (built)", point 3).
Edits save once valid; a new rule is stored only from its apply preview. The
Transactions prompt is `rule::propose` (`contains` the `rule::stem`). Writes go through
one pipeline (`rules/write.rs`, `Rules::write`, `query`, `Rules::reload`) that tells the
shell only when a transaction changed; each section's state is one value
(`rules/state.rs`, `RuleEdit`) ("the Rules view's edit pipeline and section state").
Undo is the bar's or Change history's (`change::undo_preview`, `change::undo`). At
100,606 transactions the view's load takes about 521 ms on the UI thread
(`rule::overview` 412 ms; `app.md`, "performance at 100,000 transactions"; ROADMAP R.19).

## Decision records

Oldest first.

### Decision record: rules (2026-11-10)

The open questions from the D6.7 designs (`DESIGN.md` → "Rules"), decided by the user on
the designer's recommendation:

1. **Rules win field by field.** In priority order, the first matching rule sets a field,
   and a later rule can still set a different field (a tag rule below a category rule
   both apply). A rule is "never reached" when every transaction it matches already has
   all its fields set by rules above.
2. **A new rule goes first.** A rule from an edit or from the Inbox is usually narrower
   than the rules it corrects. The Position select moves it.
3. **A rule never overwrites a category the user set unless asked.** Never on import.
   On Apply, only rows with `category_source = user` that the user includes with "Change
   these too". The same applies to agent rules (Inbox impact preview, point 1).
4. **Rule edits don't re-run on history.** Rules run on import. History changes only
   through Apply, which records one `change_batch` (`rule_apply`) per apply.
5. **Merchant names resolve Is, then Starts with, then Pattern.** An `exact` match wins,
   then the longest `prefix`, then `regex` in list order. Names apply at once with no
   batch, because the display name is derived.
6. **Undo skips rows changed since.** Undoing a batch restores a field only where it
   still holds the batch's `new_value`. Rows changed since keep the later value, and the
   undo dialog counts them.
7. **Deleting a rule leaves its values.** Its transactions keep what it set, and their
   `rule_id` is cleared. The inspector's "Set by" reads "A deleted rule", and a later
   rule or Apply may change them.
8. **Locked periods guard balances only.** `reconciled_through` doesn't cover categories,
   tags, notes or names, which don't change a balance, so Apply and Undo inside a locked
   period aren't flagged. An Exclude action with reason `duplicate` does change a
   balance, and is flagged like any other change there. *Amended 2026-11-12 by the
   user:* an exclusion a rule applies counts as a change under a lock whatever its
   reason (it changes the locked window's reported totals), so an Apply or its Undo that
   would change an exclusion there needs an unlock; so does any exclusion change in a
   change batch (merges included). See "Decision record: M1 wave 4", point 3.
   *Amended 2026-11-19 by the user:* back to balances only, which reverses the
   2026-11-12 amendment. An Apply or Undo meets a lock only with an exclusion into or out
   of a reason that leaves the balance (`duplicate`); `bank_error` and `disputed` go
   through. See "Decision record: a lock guards balances only".
9. **Rule definitions are validated in core before they're stored** (`coffer_core::rule`).
   A `regex` pattern must compile case-insensitively within a 1 MiB compiled-size limit
   and a nesting depth of 32. Every pattern is at most 1,000 bytes. Errors never repeat
   the pattern. Each action's `value_json` has one strict shape, which refuses unknown or
   repeated fields, wrong types and floats: `set_category` is a category key string;
   `set_merchant` and `set_note` are non-blank strings; `add_tag` is a non-empty array of
   distinct tag names; `exclude` is a reason key, never `other` (a rule carries no note);
   `split_template` is an array of two or more `{"category", "basis_points"}` lines in
   distinct categories, each above 0 and summing to exactly 10000; `my_share` is
   `{"basis_points": 0..=10000}`. Shares are integer basis points, and the engine turns
   them into minor units. Amount bounds are inclusive and compare against the signed
   `amount_minor`. A test keeps the Rust enum keys equal to the schema's CHECK lists.

### Decision record: rules engine, transfer detection and the CLI (2026-11-11)

Decided while building M1.15, M1.16 and M1.18 (`coffer_core::rule`,
`coffer_core::transfer`, `coffer_app::cli`) and wiring them into `import::commit`. The
transfer window and the order of preset map and rules were decided by the user.

1. **Rules override the preset's category map.** On import, a new row first gets the
   category its preset maps its source category to (`category_source = preset_map`;
   the CSV importer puts it on `StatementLine::category`). The rules then run on the
   batch, and a matching `set_category` rule replaces it (`category_source = rule`,
   `rule_id`). Rows no rule matches keep the preset's category. Rules never change a
   `user` or `ai_approved` category (rules decision record, point 3). A preset key with
   no category (the user deleted it) leaves the row uncategorised.
2. **Transfer window: 3 days, both ends included** (`TransferWindow::DEFAULT`; at most
   31). A pair is two rows on different accounts (pots and their parent included; never
   `asset` or `liability`), in one currency, with exactly opposite non-zero amounts, at
   most the window apart, neither excluded, neither already in a transfer group of any
   status (a `rejected` group keeps its rows, so a rejected pair isn't suggested again),
   and neither dated after its account's `closed_on`. Each pair becomes a `suggested`
   `transfer_group`. Detection never changes categories.
3. **Closest pairs first.** Candidate pairs are taken in order of the gap in days, then
   the gap in time when both rows have one (a pair with both times first), then the
   lower and higher transaction IDs; taking one removes its rows from the rest. The
   result depends only on row values and IDs, so it is deterministic, and two identical
   transfers each way on one day pair one-to-one. This doesn't maximise the number of
   pairs: out on the 10th and the 12th with one deposit on the 12th pairs the 12th.
4. **Rules, preset categories and transfer detection run inside the import's DB
   transaction,** in that order, on the batch's rows only (mirrored rows included). An
   error in any of them rolls the import back: a rule whose category no longer exists
   fails the import when it matches a row (not when it matches nothing), and the user
   fixes the rule. The rule set is loaded once per commit.
5. **Matching.** `is`, `starts_with` and `contains` compare text folded per character
   (lowercase, uppercase, lowercase again: `Straße`, `STRASSE` and `strasse` are equal,
   final `ς` is `σ`) without Unicode tables. `regex` uses the regex crate's
   case-insensitive mode, so a regex `straße` doesn't match `STRASSE`. The description
   is matched with whitespace collapsed but its case kept, so a regex sees the letters
   the bank wrote; patterns are compared as written. A missing field (no merchant)
   never matches.
6. **Rule store.** A new rule goes first; `create_at` and `move_to` place one at a
   position, and every write renumbers priorities 0, 1, 2, … in order. A rule naming a
   category or split category that doesn't exist is refused when saved. Errors about a
   stored rule that no longer validates name its ID and the action kind, never its
   text. Deleting a rule clears `rule_id` and keeps the category (rules decision
   record, point 7).
7. **`coffer import` acts only on a confident, unique detection.** OFX by its format; a
   preset only by its own header row (`Detection::Source`). A CSV that merely parses,
   or a tie, is an error that lists the presets for `--preset`. A multi-statement OFX
   file needs `--statement N` (from 1); without it the command lists the statements by
   kind, the account number's last four digits, currency and row count. `--account`
   takes an ID or an exact name; without it, the import goes to the only open account in
   the file's currency that holds transactions, else the command lists them. (A pot
   counted until M1.17; it no longer does: "Decision record: sub-accounts", point 7.)
8. **The CLI confirms with `--yes`, and `--dry-run` writes nothing.** An import with
   flagged rows or a balance mismatch is refused (exit 1) without `--yes`; `undo-import`
   without `--yes` previews and exits 0. `accounts` (list) and `undo-import` need an
   existing database; `import` and `accounts add` create it. Opening applies pending
   migrations, as the app does. `accounts add` checks the pot rules (a pot needs a
   current or savings parent in its currency; only a pot has a parent) before the
   schema does, for readable messages.
9. **`anyhow` in `coffer_app::cli`.** The CLI is the binary's layer but lives in the
   `coffer-app` lib so tests can run commands against a scratch database and read their
   output; it uses `anyhow` like the binary. Library crates (`core`, `import`, `mcp`)
   still use `thiserror` only. The CLI sets up no logging and never prints row text it
   wasn't asked for (a test checks fixture descriptions don't appear).

### Decision record: the Rules view (built) (2026-11-14)

M3.13 and the rules editor half of M3.7. Marked **(user)**: decided by the user on
2026-11-14 when asked before the build (each was the recommended option). Everything else
follows `DESIGN.md` → "Rules", the "Decision record: rules" above and "M1 wave 4" (A).
Migration `V16__rules_editor.sql`.

1. **V16 records what D6.7 shows (user).** `rule.origin` (`new`, `edit`, `inbox`; NULL
   for rules from before) gives the footer "Created from an edit on 12 Mar";
   `change_batch.rule_id` names the rule a `rule_apply` batch applied, so Change history
   can say "Applied rule VINTED → Shopping". It isn't a foreign key: a batch outlives its
   rule, an undone batch can't be updated (V8), and rule IDs are never reused, so a
   missing rule reads as "Applied a rule since deleted". Batches from before V16 fall
   back to the rule their category items name. `rule_prompt_dismissed` remembers "Not
   now" per field and folded pattern. `change::Recorder::for_rule` records the rule;
   `commit_apply` (and so the Inbox's approval) uses it.
2. **Core reads, core decides.** New in core, each with tests: `rule::update` (edits keep
   the place, origin and date and never re-run on history, point 4 of "rules");
   `rule::create_from` (with an origin); `rule::preview_draft` / `save_draft` (a rule not
   stored yet, planned by `preview_apply`'s own code in a rolled-back transaction, then
   stored and applied in one DB transaction, refusing a stale preview); `rule::overview`
   (each rule's matches, what it decides today field by field, its newest match, never
   reached and the rule above that catches it); `rule::preview_rows` (the preview's rows
   with now and after); `rule::propose` / `rule_from` / `dismiss` (the Transactions
   prompt); `merchant::overview` (each name's derived rows, the cleaned texts it catches,
   last seen); `change::history` / `history_items` (each batch summarised, each item as
   before → after); `inbox::preview::touching_rule` (pending agent rules over a rule's
   rows).
3. **Matches is what a rule decides today.** The whole rule set evaluated over every
   stored transaction now, as an import would; a rule decides a row when it wins at least
   one field there. Never reached: it matches rows but decides none; the card names the
   rule above that matches most of them. Not what the rows hold, since edits don't re-run
   on history.
4. **Edits save as they're made, once valid.** Selects save at once; text saves on blur
   and Enter (as M3.9's fields). A rule that doesn't validate (no text, a pattern that
   can't be used, an empty action) isn't saved and says why under the field; the stored
   rule stays as it was. A new rule (New rule, "Create rule…") is made on the apply
   preview and stored only by "Create rule and change N" or "Create rule only"; its
   preview follows every keystroke once valid. Apply on a stored rule previews it as it
   is stored.
5. **Composed, not drawn (user; `DESIGN.md` → "Rules" → "Built in M3.13").** Amount:
   Any amount / Over / Under / Between, then Out or In and the amount(s) unsigned (D6.3's
   Out/In), mapped to the signed inclusive bounds (Over £1,000 out is `≤ −100000`, Under
   £5 out is `−500..=0`); bounds no choice can say read as "Custom" and stay. Split: lines
   of a category and a percent (two decimals) adding up to 100%. Exclude: D6.3's reasons
   without Other. Tag: D6.3's pills and tag menu. New rule and New name from scratch.
   The Transactions Rule section's "Open rule" when a rule set the category. The delete
   name dialog. Empty Merchant names and Change history.
6. **The prompt proposes `contains <stem>`.** The cleaned bank merchant (else the
   cleaned description) cut at its first `*` and before its first word with a digit
   (`VINTED *KX8F2Q` → `VINTED`), three characters at least, setting the category the
   user chose, any account and amount. Shown only after one row's category is set by
   hand, when no rule decides that row's category, the rule isn't stored or turned down,
   and it matches at least one other transaction; the counts are its own preview's.
   "Create rule from this" (Rule section) builds the same rule for any row with a
   category.
7. **Left out (user).** "Show 31", "Show in Transactions" (names, history) and the
   Inbox's "Edit in Rules…": Transactions has no filter by a set of rows yet. The
   sparkle line is built: the first pending agent rule over the rule's rows, "Review in
   the Inbox" opening it there.
8. **Reordering** is dragging a row onto another (it takes that place), the new rule's
   Position select, and "Move above rule N". `rule::move_to` renumbers.
9. **Undo.** After an apply the D6.3 bar offers Undo (`change::undo`); any batch is undone
   from Change history through the dialog, which counts what goes back and what stays
   because it changed since (`change::undo_preview`). Undone rows stay listed, struck
   through, "Undone 28 Oct".
10. **Categories editor not built.** M3.7's categories editor isn't drawn in Paper; it
    waits for a design decision.

### Decision record: the Rules view's edit pipeline and section state (2026-11-16)

From an architecture review of the Rules view. Nothing the user sees changes except
points 3, 4, 7 and 8.

1. **The Rules view writes through one edit pipeline** (`rules/write.rs`,
   `Rules::write`/`query`), as Transactions does (decision record "one gate for opening
   the database, and the Transactions edit pipeline", point 3). A write is a policy and
   one call to core on the database the pipeline opens. The policy says where an error
   is said (`Said::Toolbar`, `Dialog` or `UndoBar`) and whether the shell is told
   `RulesEvent::Changed`; the pipeline owns the rest: the refusal without a database
   ("There is no database to change.", said in one place), clearing the bar, reading
   everything again through `Rules::reload` (the one way the view reads again) and the
   event. Choosing what is selected afterwards stays with the caller. Reads that open
   the database (a preview, the suggestions over a rule, a change's items, what undoing
   would do) go through `query`.
2. **`RulesStore` keeps only `open` and `load`** (the read model, `RulesData`). Its 16
   methods that opened the database and forwarded to one core function are gone; each
   call site names core's function itself. Supersedes point 4 of the decision record
   named in point 1, for Rules.
3. **Deleting a rule tells the shell.** `rule::delete` clears `transaction.rule_id`,
   which Transactions' Rule section reads ("Open rule"), and nothing told Transactions
   to read again. Editing and moving a rule change no transaction and tell nothing; an
   apply tells only when it changed a transaction.
4. **Errors are said once.** The undo dialog gets its error back and says it; it no
   longer also shows under the toolbar behind the dialog. When the bar's Undo fails, the
   bar offers Undo again with "Couldn't undo: <reason>", as Transactions' does.
5. **Each section's state is one value** (`rules/state.rs`): its filter and its
   selection with the form that selection edits, `RuleEdit::Stored { id, form,
   touching, apply }` or `RuleEdit::New { form, position, origin, apply }`, and the
   same Stored/New shape for names and categories. A form exists exactly when a row is
   selected or being made, a preview belongs to the rule it previews (`Apply::target`
   and `Target` are gone), and the suggestions over a rule belong to the rule selected.
   Each section keeps its selection while another is open, as before.
6. **Leaving Rules drops a new rule and closes a preview**, whatever the way out
   (`show`, a filter, selecting a row elsewhere). Before, leaving by `show` left the new
   rule's fields behind with nothing selected.
7. **The filter toggles switch section as `show` does**: an actual switch clears the
   bar, closes the preview when leaving Rules and, when the section has nothing
   selected, selects the first row the filter keeps. Before, they switched the section
   and nothing else.
8. **One rule for what is selected after a delete**, for rules, names and categories:
   the row after it in the filtered list, else the row before, else nothing. Deleting a
   name used to select the first name, and deleting a category the filter hides used
   to select the first listed category; both now follow the rule. The panel counts names and history with the same
   predicates the tables filter with.

### Decision record: rules skip rows that take no category of their own (2026-11-19)

Review finding R.10, in part. "What a transaction can take, decided in core", point 4, left
open whether rules should skip a confirmed transfer leg and a confirmed refund's money in,
as they skip split rows. Decided by the lead, following the user's 2026-11-16 choice that
core refuses categorising those rows (same record, point 2).

1. **A rule's category is skipped where `category::assign` says the row takes none of its
   own**: split, a leg of a confirmed transfer pair, the money in of a confirmed refund.
   This is the category-and-split field (decision record "M1 wave 4", point 4): a
   `set_category` isn't written, and a `split_template` there was already skipped as a
   row in a transfer or linked money in (`SplitSkip`). The rule's other actions
   (merchant, tags, note, exclusion, share) still apply. A suggested pair or a suggested
   refund doesn't count, so its rows are still categorised.
2. **At import and on Apply alike.** Both plan through the same code. Apply lists each such
   row in `ApplyPreview::skipped_categories` (`SkippedCategory`: the row, the rule and
   `NoOwnCategory`); the row's other changes stay in `rows`, and a row with none isn't
   listed there, so it reads as no change (the Inbox preview's "Unchanged" or "Other").
   Split rows are still skipped whole and listed in `split_rows`.
3. **A pot's mirrored pair gets no rule category.** The import mirrors a pot transfer into
   a confirmed pair before the rules run ("rules engine, transfer detection and the
   CLI", point 4, and "sub-accounts"), so the existing order already skips both legs. A
   preset map's category on the imported leg is not a rule's and stays.
4. **Other writers still don't ask** (duplicate merges, removing a split, undo):
   `change::unwritable` still refuses only split rows.
