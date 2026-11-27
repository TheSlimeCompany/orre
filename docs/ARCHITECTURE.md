# Orre — Architecture

A local-first personal finance dashboard. Import bank statements, see where money goes,
and talk to your data through any MCP-capable AI client (Claude Code, Claude Desktop, Codex).

Built for one user first; released as open source (UK, US, euro area at launch).

## Principles

- **Local-first.** All data lives in a local SQLite file. The app never needs an account or an API key.
- **Deterministic import.** Parsing statements does not involve a model. Every import is
  verifiable (balance reconciliation where balances are available).
- **AI is external and supervised.** AI clients connect over MCP. They can read through
  narrow query tools and write only to a suggestion inbox the user approves.
- **Contributable without Rust.** Bank presets are data files.

## Contents

This file is the overview: principles, the decisions table, the crates, the
dependencies and the milestones. The detail is in `docs/architecture/`, one file per
topic. Each has the reference sections for that topic, then its decision records,
oldest first. A record covering several topics is in the file for its main one.

### [Import](architecture/import.md)

How a file becomes rows: the importers, presets, dedupe, reconciliation, the commit and its undo, and the import dialog.

- Section: Import pipeline (The `Importer` trait; The CSV importer; Presets; The OFX
  importer; Dedupe, reconciliation and the commit)
- Decision record: import commit, dedupe and undo (2026-11-11)
- Decision record: importer paths and locked balances (2026-11-11)
- Decision record: the import flow (2026-11-14)
- Decision record: a bank's correction supersedes the stored row (2026-11-18)
- Decision record: a preset's dedupe identity is frozen (2026-11-18)
- Decision record: a file's account number is checked, never stored (2026-11-19)
- Decision record: New account from Import (2026-11-20)

### [Money and the data model](architecture/data-model.md)

Money as integer minor units, and the SQLite schema.

- Section: Money
- Section: Data model
- Decision record: the rename to Orre, and the schema history reset (2026-11-20)

### [Accounts](architecture/accounts.md)

Accounts and their balances: pots and other sub-accounts, accounts the user keeps by hand, assets and liabilities, closing and deleting, cash counts.

- Section: Accounts (Kinds; Creating and editing; Balances; Locks; Closing, reopening and
  deleting; Net worth and the panel; What leaves the account)
- Decision record: accounts you keep yourself (2026-11-10)
- Decision record: sub-accounts (2026-11-11)
- Decision record: accounts management (2026-11-14)
- Decision record: asset and liability accounts, closing and deleting (built) (2026-11-14)
- Decision record: cash counts (built) (2026-11-15)
- Decision record: editing and removing entered balances (built) (2026-11-20)

### [Transactions](architecture/transactions.md)

The Transactions view and what the user can do to a row: splits, tags, notes, merchant names, manual rows, exclusions, transfer pairs, removal.

- Section: Transactions (The row; Edits, undo and the lock; Which rows take a category;
  Splits, tags, notes and names; Exclusion; Manual rows; Transfers; Removing
  transactions; The register)
- Decision record: Transactions additions (2026-11-10)
- Decision record: splits, tags and notes, manual rows, merchant names (2026-11-12)
- Decision record: the Transactions view (2026-11-14)
- Decision record: splits, tags and notes in the inspector (2026-11-14)
- Decision record: excluding, the Source section and manual transactions (built) (2026-11-15)
- Decision record: transfer-pair confirmation (built) (2026-11-15)
- Decision record: removing transactions, and which rows need a category (2026-11-16)
- Decision record: Mark as transfer and Create rule from the bulk-action bar (2026-11-18)
- Decision record: two kinds of transfer, and marking a suggested pair (2026-11-19)
- Decision record: a column sort is one flat list (2026-11-20)
- Decision record: transfer or refund in the suggestion card (2026-11-20)

### [Categories](architecture/categories.md)

The category tree, the categories editor, and which rows take a category.

- Section: Categories
- Decision record: categories (2026-11-11)
- Decision record: the categories editor (built) (2026-11-15)
- Decision record: what a transaction can take, decided in core (2026-11-16)
- Decision record: category keys are never reused (2026-11-19)

### [Rules](architecture/rules.md)

Rules and their actions, the rules engine at import, transfer detection, the CLI, and the Rules view.

- Section: Rules (A rule; What a rule may write; At import; Applying to history; Transfer
  detection; The CLI; The Rules view)
- Decision record: rules (2026-11-10)
- Decision record: rules engine, transfer detection and the CLI (2026-11-11)
- Decision record: the Rules view (built) (2026-11-14)
- Decision record: the Rules view's edit pipeline and section state (2026-11-16)
- Decision record: rules skip rows that take no category of their own (2026-11-19)

### [Links and review queues](architecture/links-and-checks.md)

Change batches, refunds, reimbursements and shared costs, possible duplicates, balance checks and lock dates, export.

- Section: Links and checks (Change batches; Refunds, reimbursements and shared costs;
  Possible duplicates; Balance checks and locks; Export)
- Decision record: review queues (2026-11-10)
- Decision record: refunds, reimbursements and shared costs (2026-11-10)
- Decision record: M1 wave 4 — change batches, rule actions, links, duplicates, balance checks and export (2026-11-13)
- Decision record: review queues (built) (2026-11-14)
- Decision record: refunds, reimbursements and settlements (built) (2026-11-14)
- Decision record: a source ID is checked against the row's content (2026-11-18)
- Decision record: a lock guards balances only (2026-11-19)

### [Inbox](architecture/inbox.md)

Suggestions waiting for the user: the Inbox and its impact preview.

- Section: Inbox (Suggestions; Approving and rejecting; The impact preview; The Inbox
  screen; Orre's own suggestions)
- Decision record: Inbox impact preview (2026-11-10)
- Decision record: Inbox (2026-11-14)
- Decision record: the preview says why a rule skips a row's category (2026-11-19)

### [MCP](architecture/mcp.md)

The MCP server: tools by access level, the gate, the access log, untrusted text, and safe report rendering.

- Section: MCP surface
- Decision record: safe report rendering and withheld text (2026-11-10)
- Decision record: M2: MCP server (2026-11-13)
- Decision record: approval needs the user's terminal (2026-11-18)
- Decision record: a client reads only its own reports (2026-11-18)
- Decision record: merchant names are untrusted, and what `pseudonymous` protects (2026-11-19)

### [Settings, AI access and backups](architecture/settings-and-backups.md)

The Settings view, AI clients and their access, local backups and restore.

- Section: Settings, AI access and backups (The Settings view; AI clients and access
  levels; Local backups; Retention; Restore; Presets and backups)
- Decision record: Settings, AI access and backups (2026-11-10)
- Decision record: local backups (2026-11-11)
- Decision record: Settings, AI access and backups (built) (2026-11-14)
- Decision record: backup retention and presets (2026-11-19)

### [Reporting](architecture/reporting.md)

Budgets, the forecast, and how spending is counted over the ledger.

- Section: Reporting (The ledger and the spending rule; Reports; Previews of a change;
  Net worth; Budgets; Schedules and the forecast)
- Decision record: budgets and the forecast (2026-11-10)
- Decision record: one spending rule over the ledger (2026-11-16)
- Decision record: one net worth rule (2026-11-19)
- Decision record: full-text search over bank text (2026-11-23)
- Decision record: similar transactions (2026-11-24)
- Decision record: the month start (2026-11-24)
- Decision record: the Overview (2026-11-24)

### [The app](architecture/app.md)

The app shell, window state, the status bar, and how the app opens the database and applies edits.

- Section: The app (The shell and navigation; Window state; The status bar; Opening the
  database; Applying edits; What runs off the UI thread; `describe()` stays out of
  release builds; The dev driver; Live updates)
- Decision record: the app shell and window state (2026-11-14)
- Decision record: one gate for opening the database, and the Transactions edit pipeline (2026-11-15)
- Decision record: the status bar reads the database (2026-11-17)
- Decision record: views that describe themselves stay out of the release binary (2026-11-18)
- Decision record: performance at 100,000 transactions (2026-11-19)
- Decision record: Accounts and Rules read in the background (2026-11-19)
- Decision record: right-click menus on the accounts panel (2026-11-20)
- Decision record: the dev driver (2026-11-21)
- Decision record: live updates (2026-11-21)
- Decision record: live updates in the shell (2026-11-22)
- Decision record: what arrived from another process (2026-11-23)

### In this file

- Section: Principles
- Section: Decisions
- Section: Architecture
- Section: Dependencies
- Section: Dashboard (v1)
- Section: Milestones
- Section: Deferred / open
- Decision record: feature research (2026-11-09)

## Decisions

| Area | Decision |
|---|---|
| Stack | Rust 2024, GPUI via `gpui-kit` / `gpui-component` (charts, `DataTable`, sidebar/dock built in) |
| Platforms | Linux + macOS official; Windows community-supported, to be built in CI once there is one (M5.5; there is no CI yet) |
| License | `MIT OR Apache-2.0` |
| Storage | SQLite, WAL mode, `synchronous = FULL`, file mode 0600 (data dir 0700), `orre.db` in the platform data dir (`~/.local/share/orre`; `~/Library/Application Support/Orre` on macOS), overridable with `ORRE_DATA_DIR`. Not encrypted in v1 (SQLCipher possible later) |
| Money | Integer minor units + ISO 4217 currency per account/transaction. Never floats |
| Currency | Multi-currency; one home currency for reporting. ECB reference rates fetched opt-in and cached (the app's only network call); manual rates as fallback |
| Formats (v1) | CSV (generic mapping + presets), OFX/QFX. QIF, CAMT.053, MT940 later via `Importer` trait |
| PDF | Deferred. Plan: local text extraction + per-layout templates; any model-based extraction opt-in only |
| Categorization | Local rules engine first; AI-proposed categories/rules land in the Inbox for approval. Approving a categorisation sets those transactions' categories (an undoable `bulk_edit` batch, source `ai_approved`); only approving a `rule` suggestion creates a rule. The app also files suggestions itself (source `orre`): a `rule` after repeated identical manual recategorisations, and a `categorization` for an imported row whose nearest look-alikes agree (FT.4, `bm25` over the full-text index): deterministic, no ML. Learned classifiers are not used |
| Transfers | Auto-detect opposite-amount pairs across own accounts within 3 days (`TransferWindow::DEFAULT`); user confirms; a confirmed pair is excluded from income/spend and its legs take no category. A suggested pair counts by its own category until confirmed, and the table marks it. A category of kind `transfer` is for a single row whose other side Orre doesn't hold. See "Decision record: two kinds of transfer, and marking a suggested pair" |
| Pots / sub-accounts | Monzo Pots, Starling Spaces, Revolut Vaults etc. are sub-accounts of their parent. A preset's `[sub_account_transfer]` identifies pot transfers; the import commit creates the pot when its key (the bank's ID for it, or its name where the export has no ID, as Monzo's doesn't) is new, mirrors the row onto it (`origin = mirrored`) and pairs the two as a confirmed transfer. Pots count toward net worth (opening balance entered once per pot). See "Decision record: sub-accounts" |
| AI integration | `orre mcp` stdio MCP server (`rmcp`). Read-only query tools + inbox-only writes. No account numbers at any level; names and bank text only at `standard` (see "MCP surface") |
| Splits | A transaction can be split into lines that sum exactly to its amount, in its currency (enforced in core, in the same DB transaction as the edit). Reports, budgets and MCP read the `ledger_line` view: one row per split line, or the transaction itself when it isn't split |
| Tags and notes | Many tags per transaction, alongside categories. Tag names are user text: MCP sees them only at `standard` access, and never for tags marked private. The imported note stays read-only; the user's note is a separate field. Neither note is exposed over MCP, at any level |
| Manual entries and corrections | Manual rows have `origin = manual` and no import batch. Imported source fields (date, amount, raw description, external ID) are never edited: a bad import is undone, or a row is excluded with a recorded reason and stays visible under an "excluded" filter. User-facing fields (category, display merchant, tags, notes, splits) are editable. Manual rows are the user's own data: every field is editable and they can be deleted. Exclusion reasons and their effect on balances: see the decision record "Transactions additions" |
| Merchant names | `merchant_display` is derived: deterministic clean-up of the raw text, then the merchant alias table, then rule actions. It can be recomputed at any time; the raw text is untouched. A name set by a rule or the user (`merchant_display_source`) is never recomputed. See "Decision record: splits, tags and notes, manual rows, merchant names" |
| Refunds, reimbursements, shared costs | `transaction_link` ties a refund to its purchase (it nets against the purchase's category instead of counting as income), a reimbursement to the expense it repays, and a settlement to a share label. `my_share_minor` (default: the full amount) gives personal and gross views; the difference builds an owed-to-me balance per local share label. Share labels never leave the app. Still single-user: no one else's accounts are modelled |
| Duplicates | Exact duplicates are prevented at import (source ID, then fingerprint). Near-duplicates (same account and amount within ±N days, different text) become `duplicate_candidate` rows for review. Merging excludes one row, never deletes it, and can be undone |
| Reconciliation | Every `balance_snapshot` is an assertion checked against the computed balance; a failure reports the window since the last passing assertion. `account.reconciled_through` freezes the account's balance through that date: creating or deleting a row, changing its amount, date or account, entering a balance, or an exclusion into or out of a reason that leaves the balance (`duplicate`, `superseded`) on a locked date is refused, whether from an import, a manual edit, a merge, a rule apply or an undo, until an explicit unlock. Categories, names, notes, tags, splits, shares, links and other exclusions aren't locked (user, 2026-11-19). See "Decision record: a lock guards balances only" |
| Bulk changes | Changes to many existing transactions (a rule applied to history, duplicate merges, bulk edits) are previewed, then committed as an undoable `change_batch` that records old and new values, like import batches |
| Schedules and forecast | A detected recurring series becomes a schedule only when the user confirms it. The forecast is the last known balance plus schedule occurrences (plus the remaining flex budget), per account per day, in integer arithmetic, using cached FX rates only |
| Budgets | Per-category monthly amounts with optional rollover. Each budget is in a group: fixed, non-monthly (accrues 1/12 of a yearly amount, seeded from yearly schedules) or flex |
| Asset and liability accounts | Account kinds `asset` and `liability` hold manually valued items (house, pension, mortgage, loan). Valuations are `balance_snapshot` rows with source `valuation`; net worth uses the last value on or before each date. Investment holdings and prices are deferred (ROADMAP L.14) |
| Backups and export | Rotating `VACUUM INTO` snapshots (0600) in `<data dir>/backups` (0700) before every import commit, migration and restore; the 10 most recent automatic backups plus the newest of each of the 12 months before the current one are kept, and manual ones stay until the user deletes them ("Decision record: backup retention and presets"). Restore runs `PRAGMA integrity_check` and a schema-version check on a copy before swapping files (see "Decision record: local backups"). Export is CSV/JSON with a documented, versioned schema (split lines carry their parent ID); user notes and private tags only with `--include-notes` / `--include-private-tags` (user, 2026-11-12), share labels never. Beancount/hledger journal export follows in M5.8. Local files only, no network |
| MCP access | MCP is off until the user enables a client. Each client has an access level (`aggregate`, `pseudonymous` or `standard`); tools above it are never registered. The client is named by `orre mcp --client <name>` in its configuration. This is a guardrail against over-sharing, not authentication: any local process can read the database file (at-rest encryption is L.6). Every tool call is recorded in `mcp_access_log`. Approval happens only in Orre's own window or CLI; MCP elicitation, sampling and MCP Apps are not used for it |

### Decision record: feature research (2026-11-09)

The rows above from Splits to MCP access come from
[`research/feature-ideas.md`](research/feature-ideas.md). Four points were checked
against the invariants and decided as follows:

1. **The MCP access log is not an MCP write.** The invariant "MCP tools may write to
   `suggestion` and `report` and nothing else" still holds for every tool. The server
   appends to `mcp_access_log` as its own audit record, in the same way it already writes
   logs to stderr. The AI can't choose what gets written; its only input is the optional
   `purpose` string, which is stored as untrusted text and never interpreted. A log of
   what the AI read makes the inbox model easier to verify, so the benefit outweighs a
   strict reading of the invariant.
2. **Imported source fields are immutable.** Date, amount, raw description and external ID
   are never edited. Corrections go through undoing a batch, excluding a row with a
   recorded reason (still visible under a filter), or user-facing fields such as category,
   display merchant, splits and notes. This is stricter than most apps, but it keeps
   "never silently alter data" and keeps dedupe and reconciliation stable across
   re-imports.
3. **MCP access levels are a guardrail, not a security boundary.** They stop a client
   from being shown more than the user chose to share. They don't stop a local process
   that reads the SQLite file directly; at-rest protection is L.6 (SQLCipher).
4. **Defaults, not invariants.** Keeping 10 backups plus one per month, and non-monthly
   budgets accruing 1/12 of the yearly amount, are starting values. Change them freely
   without a new decision.

## Architecture

```
crates/
  core/    domain types, schema + migrations, rules engine, transfer matching,
           reporting queries, dedupe. No UI, no AI.
  import/  `Importer` trait; csv (generic + presets), ofx
  mcp/     rmcp server over core
  app/     GPUI dashboard
presets/   <region>/<bank>.toml  — CSV column mappings (data, not code)
fixtures/  redacted sample files; every importer and preset has one
```

Packages are named `orre-core`, `orre-import`, `orre-mcp` and `orre-app`. The
binary is built from `orre-app`, which depends on the other three; shared files stay at the
repository root (`assets/` for fonts and brand marks, `design/` for the exported tokens).

Single binary: `orre` (GUI: the app shell around the views, see "Decision record: the
app shell and window state"), `orre mcp --client <name>` (stdio MCP server), and the
headless commands (`orre_app::cli::Command`): `orre import <file>`, `orre
accounts`, `orre undo-import`, `orre backup [--list | --delete <file>]` / `orre
restore <file> [--yes]` (M1.28), `orre export`, `orre ai-access`, `orre budgets`,
`orre schedules`, `orre inbox` and `orre categorize`.

The GUI and MCP processes share the SQLite file; WAL allows concurrent readers.

## Dependencies

Versions live in `[workspace.dependencies]` in the root `Cargo.toml`; crates opt in with
`<crate>.workspace = true`. Chosen 2026-11 (crates.io research); revisit when pins lift.

| Area | Crates | Notes |
|---|---|---|
| Storage | `rusqlite` (`bundled`), `refinery` | `rusqlite` pinned to 0.40 until `refinery` supports 0.41. `refinery` checksums applied migrations, enforcing "never edit a shipped migration". No connection pool: one connection per process, WAL + `busy_timeout` + `BEGIN IMMEDIATE`; the app and the CLI open with `Database::open`, which creates the database if needed and runs migrations (all pending ones in one transaction, applied by Orre itself with refinery's checksums and history table, so Rust such as the merchant naming pass can run before the commit); `orre mcp` opens with `Database::open_existing`, which never creates or migrates and refuses a missing or older database, telling the user to open Orre or run a CLI command. Both check an existing file first (a SQLite database, not a newer schema, applied migrations matching the embedded ones) and leave a refused file untouched: no permission change, WAL switch or write. SQLCipher later = `bundled-sqlcipher-vendored-openssl` |
| Import | `csv`, `serde`, `encoding_rs`, `chrono`, `toml` | Encoding comes from the preset, the OFX header or a BOM; without any, only UTF-8 is accepted, and a file that doesn't decode is refused, never guessed. The BOM is stripped before `csv`. Dates must round-trip through their format string. OFX is read by Orre's own tolerant SGML/XML reader, not `ofx-kit` (dropped in M1.10, see "The OFX importer") |
| Money / FX | none | Own `Money { minor: i64, currency }` (`orre_core::money`, M1.5) with a hand-written, test-frozen exponent table. FX in `i128` fixed point with one explicit rounding. Decimal types from format crates are converted to minor units at the import boundary, never stored. See "Money" below |
| Rules | `regex`, `aho-corasick`, `serde_json` | `regex` is linear-time, so user rules can't hang the app. `serde_json` (already in the tree, MIT OR Apache-2.0) decodes `rule_action.value_json` strictly in core |
| Plumbing | `thiserror` (libs), `anyhow` (binary), `clap`, `tracing`, `tracing-subscriber`, `directories`, `uuid` (v7) | Logs go to stderr |
| MCP | `rmcp` (`transport-io`), `tokio` (current-thread), `schemars` | Only the `mcp` crate / `orre mcp` path uses tokio; `core` stays synchronous |

Later: `ureq` for the ECB fetch, reading the ECB Data Portal's CSV
(`data-api.ecb.europa.eu`, `format=csvdata`) with the `csv` crate already in import, so no
XML parser; `mt940`; an own CAMT.053 mapping on `roxmltree` (the `camt053` crate is barely
used); own QIF parser. Avoid GPL crates (`ofxy`, `tree_magic_mini`'s GPL data feature).
Research and candidates per roadmap item: [`research/crates.md`](research/crates.md)
(2026-11-13).

## Dashboard (v1)

- **Transactions**: filterable/sortable table, inline category edit, import flow; split
  editor, tags and notes in the inspector; manual transactions; exclude with a reason.
- **Inbox**: approve/reject AI suggestions in bulk, with an impact preview per suggestion.
- **Review queues**: possible duplicates; reconciliation (assertion results, lock/unlock);
  refunds, outstanding claims and owed-to-me balances.
- **Monthly overview**: income vs spending vs net per month; category breakdown with MoM delta.
- **Trends**: category/merchant over time, top merchants, recurring payments.
- **Upcoming**: confirmed schedules, missed payments, daily balance forecast per account.
- **Budgets & net worth**: per-category monthly targets with rollover, groups and a pace line;
  balances/net worth over time (from OFX `LEDGERBAL`, CSV balance column, manual opening
  balance, or asset/liability valuations).
- **Reviews**: saved MCP reports, safely rendered.
- **Rules**: rules in priority order with their actions, apply-to-history preview, merchant
  names, and the history of changes to existing transactions (undo).
- **Settings**: AI access (clients, access levels, access log, kill switch), backups and
  export.

## Milestones

1. **Core + import (headless)** — workspace split, schema/migrations, CSV (generic + preset)
   and OFX importers, dedupe, reconciliation, rules, transfer detection;
   `orre import` CLI; fixtures + tests. Also splits, tags and notes, manual
   transactions, merchant clean-up, rule actions, refund/reimbursement links,
   near-duplicate queue, balance assertions and lock, backups, CSV/JSON export.
2. **MCP server** — read tools, inbox writes, `monthly_review` prompt; usable from
   Claude Code/Codex against real data. Also schedules, access levels, the access log and
   the untrusted-text envelope.
3. **GUI foundation** — app shell, transactions table, import + mapping screen, inbox. Also
   the split editor, manual entries, links, duplicates and reconciliation, rule previews,
   inbox impact preview, AI access settings, backups, asset/liability accounts.
4. **Analytics** — monthly overview, trends, recurring, budgets, net worth, FX. Also
   upcoming payments and forecast, budget rollover/groups/pace, safe report rendering.
5. **Open-source release** — README, CONTRIBUTING (preset how-to), presets for common
   UK/US/EU banks, CI (Linux/macOS/Windows build), release artifacts (tarball, Nix, .dmg),
   Beancount/hledger export.

## Deferred / open

- PDF import; QIF, CAMT.053, MT940 importers.
- Encrypted DB.
- Streamable-HTTP MCP inside the GUI (live "jump to transaction").
- i18n of UI strings beyond category labels.
- Investment holdings and prices, goals, attachments (ROADMAP L.11–L.14). Any online price
  fetch would be a second network call and needs its own decision.
- Feature research and the list of rejected ideas: [`research/feature-ideas.md`](research/feature-ideas.md).
- Crate research for M3–M5 and Later: [`research/crates.md`](research/crates.md).
