# Orre — Architecture: Money and the data model

Money as integer minor units, and the SQLite schema.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Money

`orre_core::money` (M1.5). Decided 2026-11-10:

- **Currencies come from a fixed table.** `Currency` can only be built from a code in a
  hand-written ISO 4217 table, which gives its minor units (`GBP` 2, `JPY` 0, `KWD` 3,
  `CLF` 4); the tests freeze it, because changing a value changes what every stored
  amount in that currency means. The table holds the active list, apart from codes with
  no minor unit (precious metals, `XDR`, `XSU`, `XUA`, test codes), plus codes withdrawn
  since 2014 (`HRK`, `LTL`, `SLL`, `ZWL`, …), since older statements and card payments
  abroad still name them. ISO's minor units win where everyday use differs (`MGA`,
  `MRU`). An unknown code, including one read back from the database, is an error.
- **Arithmetic is checked.** Adding, subtracting or comparing two currencies is an
  error, as is overflowing `i64`; nothing panics or wraps. There are no operator
  overloads for the same reason.
- **Import reads amounts exactly or not at all.** `Money::parse_decimal` takes the
  preset's decimal mark (`.` or `,`); thousands may be grouped by the other mark, a
  space (including no-break spaces) or an apostrophe, in groups of three and never
  after a leading zero (`0,123` with a `.` mark is refused, not read as 123: it's a
  decimal in the other style); the sign is a leading `-`, `−` or `+`, or parentheses.
  Extra decimals are accepted only when they
  are zeros (`12.340` GBP): anything else would mean rounding, so it's refused and the
  row is flagged. Currency symbols, codes and trailing minus signs are the importer's
  to deal with (a preset feature if a bank needs one). Parse errors don't contain the
  text, since errors get logged.
- **Display formatting is the app's.** Core's only text form is the plain, locale-free
  `-1234.56` (`to_decimal_string`, for exports and the CLI), which parses back
  unchanged. Symbols, locale separators, `−` and `+` follow `DESIGN.md` → "Money" in
  the app.

## Data model

The schema is SQL in `crates/core/migrations/`. Conventions (M1.4; the tests in
`crates/core/src/db/schema_tests.rs` check them):

- Every table is `STRICT`, so a money column (`*_minor`, INTEGER) refuses a fractional
  value. Currency codes are three capital letters; the ISO 4217 list is checked in Rust.
- Dates are `YYYY-MM-DD`, times `HH:MM:SS`, months `YYYY-MM`, timestamps UTC
  `YYYY-MM-DDTHH:MM:SS.SSSZ`, each checked to round-trip through SQLite's date functions.
  Enums are TEXT keys listed in a `CHECK`; booleans are 0/1.
- IDs are `INTEGER PRIMARY KEY AUTOINCREMENT`, so an ID is never reused after a delete
  (an undone import) and an old suggestion or log entry can't come to name another row.
- Foreign keys have no `ON DELETE` action and are all indexed: deleting is explicit in
  core, never a side effect. Deleting a rule clears `transaction.rule_id` in core.
- A trigger refuses edits to an imported or mirrored row's source fields (account, batch,
  date, time, amount, currency, merchant, descriptions, external ID, fingerprint, source
  category and type, original amount, note); a transaction's `id`, `origin` and
  `created_at` never change.
- Partial unique indexes enforce dedupe, source ID first: `(account_id, external_id)`,
  and `(account_id, fingerprint)` among rows without an external ID (a row with a source
  ID is never matched by fingerprint, since the occurrence index depends on the file's
  order). Manual rows carry neither key. An insert that collides with a stored row is
  refused by a trigger before SQLite resolves the conflict, so `INSERT OR REPLACE` can't
  swap a row past the read-only trigger, and core never uses `REPLACE` or upserts on
  `"transaction"` (a test scans for them); `INSERT OR IGNORE` on a duplicate is an error
  too, because core dedupes first.
- Triggers also keep categories to two levels with keys that match their place (a
  top-level key is one part, a subcategory's is `<parent key>.<part>`), make a
  category's `key`, `parent_id` and `kind` read-only (keys are stable; only the label
  changes) and give a subcategory its group's kind,
  keep a transaction in its account's currency, keep `asset`/`liability` accounts to
  `valuation` snapshots only (and valuations to those accounts), put a pot under a
  `current` or `savings` account in the same currency and keep it there once it holds
  transactions or balances, fix an account's currency once it has transactions, balances,
  pots or a rule with an amount bound (and keep such a rule from moving to an account in
  another currency), keep an imported row and a file's balance on the
  batch's account (a mirrored row or balance on a pot of it), and make an import batch
  read-only except for `status`. Undo deletes a batch's transactions and balances, then
  sets `undone` in the same DB transaction: a batch can't become `undone` while rows
  reference it, never returns to `committed`, and an undone batch takes no rows.
- An account has at most one entered balance (`batch_id` empty: `statement`, `manual`,
  `valuation`) per date, since each asserts the same end-of-day balance; a correction
  edits it. Balances from files may repeat a date across overlapping exports, and a
  mismatch is a failed check, not a refused insert.
- `transaction` is an SQL keyword, so the table name is always quoted (`"transaction"`).
- Tables arrive with the roadmap item that needs them. M1.4 created the ones below up to
  and including `report`, except `transaction_split` and `ledger_line` (V5, M1.20),
  `tag`/`transaction_tag` (V6, M1.21), `merchant_alias` (V7, M1.23),
  `change_batch`/`change_item` (V8, M1.24/M1.26), `transaction_link` (V9, M1.25; it also
  redefines `ledger_line`), `duplicate_candidate` (V10, M1.26), `mcp_client`,
  `mcp_access_log` and `setting` (V11, M2.1), the schedule tables and
  `recurring_dismissal` (V12, M2.6/M2.10), the inbox's decision columns (V13, M2.3) and
  `mcp_access_log` rebuilt with two outcomes, `pending_id` and `summarised_calls` (V14),
  `account.preset_id` (V15, M3.2), and `rule.origin`, `change_batch.rule_id` and
  `rule_prompt_dismissed` (V16, M3.13), `change_item.field` rebuilt with `links` (V17,
  M3.11), `category.hidden` (V18, M3.7), `report.access_level` (V19, R.2),
  `duplicate_candidate` rebuilt with the `unmatched_source_id` reason (V20, R.3), and
  `"transaction"` rebuilt with `superseded` and `superseded_by` (V21, R.4: the first
  migration run with foreign keys off, marked by its first line),
  `category_key_retired` (V22, R.11) and `account_source_identity` (V23, R.13).
- refinery embeds migrations in file-name order (`V10__` before `V1__`);
  `db::migrations::embedded` sorts them by version.

- `account(id, parent_id?, name, institution, kind [current|savings|credit_card|pot|cash|
  asset|liability], currency, opening_balance_minor?, opening_date?,
  reconciled_through?, closed_on?, sub_account_key?, preset_id?)` — pots are child
  accounts of their parent; a pot's parent is a `current` or `savings` account in the same
  currency; `sub_account_key` (V4, pots only, unique per parent, read-only once set) is
  the bank's ID for a pot the import created; `preset_id` (V15, current, savings and
  credit card only) is the preset the next import uses ("Decision record: accounts
  management");
  `asset` and `liability` accounts hold `valuation` snapshots only, no transactions
- `import_batch(id, account_id, source_file, format [csv|ofx|qif|camt053|mt940],
  preset_id?, imported_at, status [committed|undone])` — `source_file` is the file name
  without its directory; QFX is `ofx`
- `account_source_identity(id, batch_id UNIQUE → import_batch, account_id → account ON
  DELETE CASCADE, identity_hash)` (V23, R.13) — HMAC-SHA256 of a file's normalised
  account number under `setting` `import.account_key`, one row per committed batch whose
  file had a number; undoing the batch deletes its row. The number itself is never
  stored and MCP never reads the table ("Decision record: a file's account number is
  checked, never stored")
- `import_flag(id, batch_id, location_kind [line|entry], location, kind [unreadable|
  source_id_conflict|locked|running_balance], detail)` — a row the import left out or
  doubts (M1.11–M1.13, V3): its place in the file and core's reason, never the row's
  text. Read-only; undo deletes it with its batch
- `change_batch(id, kind [rule_apply|duplicate_merge|bulk_edit], created_at,
  status [applied|undone], undone_at?, rule_id? (V16: the rule a `rule_apply` applied;
  not a foreign key, it outlives its rule))` and `change_item(id, batch_id, transaction_id,
  field [category|merchant|user_note|tags|exclusion|split|share|reimbursable|links (V17)],
  old_value, new_value)` — undo for changes to existing transactions (V8). Values are
  JSON, one shape per field (`orre_core::change::Value`); one item per batch,
  transaction and field; items and a batch's kind and date are read-only, and an undone
  batch stays undone. Deleting a transaction deletes its items. A `links` item holds the
  `transaction_link` rows whose inflow is the item's transaction, in any status, so each
  link is recorded once ("Decision record: refunds, reimbursements and settlements
  (built)")
- `transaction(id, account_id, batch_id?, date, time?, amount_minor, currency, merchant?,
  merchant_display?, merchant_display_source? [rule|user], description_raw, description_normalized, external_id?, fingerprint,
  source_category?, source_type?, original_amount_minor?, original_currency?, category_id?,
  category_source [rule|preset_map|user|ai_approved], rule_id?, transfer_group_id?, note?,
  user_note?, my_share_minor?, share_label?, reimbursable,
  excluded_reason? [duplicate|bank_error|disputed|other|superseded], excluded_note?,
  excluded_at?, superseded_by? (V21: the row that replaced it, set exactly when the reason
  is `superseded`),
  origin [imported|mirrored|manual], created_at)` — `mirrored` rows are the generated
  pot-side half of a sub-account transfer, and undoing their import batch removes them too;
  `manual` rows have no batch and no `external_id` or `fingerprint`; `note` is the imported note (read-only), `user_note` the
  user's; `category_id` is empty when the transaction is split; `merchant_display_source`
  (V7) is empty while the name is derived and set when a rule or the user named the
  merchant, which recomputes leave alone. The three `excluded_*`
  fields are set and cleared together (`excluded_note` is required for `other`); a
  superseded row keeps its source ID, and the unique source ID index counts only rows that
  aren't superseded;
  `created_at` is when the row entered the database (the batch commit for imported rows)
- `transaction_split(id, transaction_id, line_no, amount_minor, category_id?,
  my_share_minor?, share_label?, user_note?)` — lines sum exactly to the parent amount
  (checked in core), each non-zero with the parent's sign, in the parent's currency;
  `line_no` from 1, unique per transaction (V5)
- `ledger_line` (view) — the split lines, or the transaction itself when unsplit, with
  `split_id`/`line_no` NULL on a transaction's own row, `transfer_status` and
  `my_amount_minor`. It doesn't filter: reports skip excluded rows and confirmed
  transfers; a linked refund takes its purchase's category
- `tag(id, name, private)`, `transaction_tag(transaction_id, tag_id)` — names unique
  ignoring case, at most 64 characters; the link table is `WITHOUT ROWID` and its rows
  are only inserted or deleted (V6)
- `merchant_alias(id, match_kind [exact|prefix|regex], pattern, display_name,
  created_at)` (V7)
- `transaction_link(id, kind [refund|reimbursement|settlement], inflow_id, target_id?,
  share_label?, amount_minor, status [suggested|confirmed|rejected], created_at)` —
  refunds and reimbursements target a purchase; settlements target a share label (V9).
  One link per pair in any status; the inflow is money in, the target money out, in one
  currency; confirmed links never take more than either side has; a linked inflow isn't
  split and has no share; a linked row's amount and currency are fixed. `ledger_line`
  gains `link_id`, `link_kind`, `target_id`, `target_split_id`, `in_reports` and
  `personal_minor` (see "Decision record: M1 wave 4", section B)
- `duplicate_candidate(id, transaction_a, transaction_b, reason [overlapping_import|
  manual_entry|unmatched_source_id (V20)], status [pending|merged|dismissed], kept? [a|b], change_batch_id?,
  created_at)` — a pair once, `transaction_a < transaction_b`, on one account; `kept` and
  the `duplicate_merge` batch are set exactly when merged (V10)
- `balance_snapshot(id, account_id, batch_id?, date, balance_minor,
  source [ofx|csv|statement|manual|valuation], note?, created_at)` — each one is an
  assertion; an account's first is its starting point and every later one is a check;
  `note` is user text, not sent over MCP by default. `ofx` and `csv` snapshots belong to
  their import batch and are removed when it is undone; the others have no batch
- `category(id, parent_id?, key, label, kind [spending|income|transfer], hidden)` — two
  levels, stable keys (`food.groceries` under `food`; key and parent read-only),
  translatable labels; a subcategory has its group's kind, and kinds are read-only.
  `hidden` (V18, 0/1) takes a category out of the pickers and menus but keeps its
  transactions and its place in reports; a hidden group's subcategories drop out with it
  while each keeps its own flag ("Decision record: the categories editor (built)"). The
  defaults are seeded by `V2__default_categories.sql` (see "Categories")
- `category_key_retired(key, retired_at)` (V22, R.11) — the key of every deleted
  category, filled by an `AFTER DELETE ON category` trigger and never removed. A key is
  taken while a category has it or it is retired; categories deleted before V22 left no
  record ("Decision record: category keys are never reused")
- `rule(id, priority, match_field [merchant|description|source_category|source_type],
  match_kind [is|starts_with|contains|regex], pattern, amount_min?, amount_max?,
  account_id?, created_at, origin? [new|edit|inbox] (V16))` and `rule_action(id, rule_id, kind [set_category|
  set_merchant|add_tag|set_note|exclude|split_template|my_share], value_json)` — one
  action per kind per rule; an `exclude` action's value is a reason key; `priority` isn't
  unique (ties go by ID). Patterns and `value_json` shapes are checked in core
  (`rule::RuleDefinition`; see "Decision record: rules", point 9)
- `rule_prompt_dismissed(match_field, pattern, dismissed_at)` — "Not now" on the
  Transactions rule prompt, per field and folded pattern (V16, M3.13)
- `transfer_group(id, status [suggested|confirmed|rejected], created_at)`
- `schedule(id, account_id, merchant_display, category_id?, cadence [weekly|fortnightly|
  four_weekly|monthly|quarterly|yearly], start_date, amount_minor, currency,
  tolerance_minor, tolerance_days, status [active|paused|ended], series_key?,
  created_at)`, `schedule_skip(schedule_id, occurrence_date, skipped_at)` and
  `schedule_match(schedule_id, occurrence_date, transaction_id, matched_by [auto|user],
  matched_at)` (V12) — `start_date` is the first tracked occurrence; occurrence *k* is
  `Cadence::nth(start_date, k)`, always counted from the start, and the next date is
  derived, never stored. Account, cadence, start, currency and series key are read-only;
  an ended schedule stays ended. A transaction fulfils at most one occurrence (UNIQUE),
  on the schedule's own account; an occurrence is never both matched and skipped; matches
  and skips are only inserted or deleted. Missed and late are derived, not stored
  ("Decision record: M2: MCP server", section D)
- `recurring_dismissal(series_key, dismissed_at)` (V12) — a detected series the user
  marked "Not recurring"; it isn't suggested again
- `budget(id, category_id, month?, amount_minor, currency, rollover,
  group_kind [fixed|non_monthly|flex], opening_set_aside_minor?, created_at)` — rollover
  is derived from `ledger_line`, never stored; `opening_set_aside_minor` is what a
  non-monthly budget had already saved when it was created. `month` empty is the standing
  budget and `YYYY-MM` overrides one month. Fixed and flex amounts are monthly; a
  non-monthly amount is yearly
- `fx_rate(date, base, quote, rate_e9, source [ecb|manual])` — 1 `base` = `rate_e9` /
  10⁹ `quote`: fixed point, so no float enters a conversion
- `suggestion(id, kind [categorization|rule|transfer_pair|merchant_alias|refund_link],
  payload_json, rationale, source_client, created_at, status [pending|approved|rejected],
  decided_at?, decision_reason?, change_batch_id?, applied_json?)` — `source_client =
  orre` for suggestions the app files itself. `payload_json` is the validated canonical
  form; one pending suggestion per `(kind, payload_json)` (partial unique index
  `suggestion_pending_once`). V13 adds the decision columns and triggers: the proposal
  fields are read-only, a suggestion is filed pending, a decided one stays decided and has
  `decided_at`, only a rejection has a reason (≤ 500 characters), and only an approval has
  a change batch or `applied_json` (a record of what it created, with no foreign keys)
- `report(id, period, title, body_markdown, source_client, created_at, access_level?
  [aggregate|pseudonymous|standard])` — the level it was saved at (V19); NULL, saved
  before V19, reads as `standard`
- `mcp_client(name, enabled, access_level [aggregate|pseudonymous|standard],
  status [requested|known|ignored], created_at, last_seen_at?)` (V11) — keyed by the name
  `orre mcp --client` was given (1–64 of `A–Z a–z 0–9 . _ -`, never `orre`); an
  enabled client is `known`. An unknown name creates a `requested` row that isn't
  enabled. Revoking sets `enabled = 0` and the level back to `aggregate`
- `mcp_access_log(id, at, client, tool, access_level?, outcome [ok|refused_off|
  refused_not_enabled|refused_level|refused_restoring|refused_unavailable|error],
  purpose?, args_redacted_json, row_count, result_hash?, withheld_count, pending_id?,
  summarised_calls?)` (V11, rebuilt by V14) — read-only; never stores returned data.
  `client` is the name (no foreign key, so a log row outlives its client); a refused or
  failed call has no rows, hash or withheld count; `withheld_count` is how many
  descriptions the call withheld as instructions. `refused_restoring` and
  `refused_unavailable` are calls refused while the database couldn't be opened, moved in
  from the pending file with their own time and `pending_id` (unique); `summarised_calls`
  is set only on a row standing for calls past the pending file's cap (decision record
  "M2: MCP server", F.8). Rows older than 12 months are deleted when a server starts
- `setting(key, value)` (V11) — `mcp.enabled` (the kill switch; on unless set off) and
  `mcp.pseudonym_key` (32 random bytes, created by `orre ai-access enable`, never
  exposed); `import.account_key` (32 random bytes, created by the first import commit
  whose file states an account number, never exposed; R.13); `app.window` (the window's
  state, app-only); `reporting.month_start` (the month start as JSON, M4.11; missing
  or unreadable means the 1st; `reporting.md`, "Decision record: the month start")

### Decision record: the rename to Orre, and the schema history reset (2026-11-20)

**Context.** The app was called Coffer until it went to market. "Coffer" is a common word,
and a quick screen of the trademark registers and app stores found finance products using
it and most of the alternatives. "Orre" had no software trademark anywhere and no finance
app using it (the user's choice, 2026-11-20). A solicitor's clearance search is still to
be done before launch.

**Decision.**

1. The name is **Orre** in the app's text and the docs, and `orre` everywhere else: the
   binary, the crates (`orre-core`, `orre-import`, `orre-mcp`, `orre-app`), the
   environment variables (`ORRE_DATA_DIR`, `ORRE_LOG`, `ORRE_BLESS_GOLDEN`), the data
   directory (`ProjectDirs` application `Orre`: `~/.local/share/orre` on Linux) and the
   database file (`orre.db`).
2. Nothing moves the old data directory. No one's data was kept: the only database was
   the developer's own, and it was not needed (user, 2026-11-20).
3. The shipped migrations were edited in place rather than followed by new ones. V1's
   `source_client` for the app's own suggestions and V11's reserved client name are now
   `orre`, and the comments and the `-- orre: foreign_keys=off` directive follow the
   rename. Nothing had shipped to anyone, so "never edit a migration that has shipped"
   (`AGENTS.md` → Invariants) still holds from here on. This is not a precedent for
   editing migrations later.

**Consequences.** Refinery checks each applied migration's checksum, so a Coffer database
or backup no longer opens: the app reports a migration history it doesn't recognise, and
restore refuses the file. Start from an empty data directory. Decision records written
before this one say Orre where they said Coffer; git history has the original wording.
The Paper file was renamed Orre the same day, with every wordmark, eyebrow and caption in
it; its tokens didn't change (content hash `6a126241`), so `design/tokens.json` changed only
its `fileName`. The "Logo · concepts" artboard is a record of the exploration and keeps its
captions' meaning, such as coin stack B spelling a C. The mark and app icon (a strongbox)
are unchanged.
