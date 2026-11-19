# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex, etc.) working in this repository.

Coffer is a local-first personal finance dashboard: a Rust + GPUI desktop app that
imports bank exports (CSV, OFX/QFX) into a local SQLite database, shows them as tables and
charts, and exposes the data to AI clients through an MCP server.

- **Design and rationale:** [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) is the overview
  and the list of every section and decision record; the detail is in
  `docs/architecture/<topic>.md` (reference sections, then that topic's decision records).
  Read the overview and the topic's file before changing architecture, schema, import
  behaviour, or the MCP surface. A reference written `ARCHITECTURE.md` → "…" names a
  section or record in that list.
- **What to build next:** [`docs/ROADMAP.md`](docs/ROADMAP.md) — milestones and status.
- **Visual design system:** [`docs/DESIGN.md`](docs/DESIGN.md) (sourced from the Paper
  file) — read before any UI work. Use its tokens and component specs; never invent
  colours, sizes, or radii, and never ship stock `gpui-component` styling. Missing values
  get designed in Paper first.

## Status and code map

- **Status:** M0, the design track (D1–D6), M1 and M2 are done; every M3 item is built,
  and M3's exit check on real data is the user's. The UI review is done except U.5
  (deferred). Detail per item: `docs/ROADMAP.md`.
- **Where things are:** [`docs/CODEMAP.md`](docs/CODEMAP.md), by crate and area. It
  lags the code: check the tree before assuming a module exists, and update the map
  when you add or move a module.

## Build and verify

Run everything inside the dev shell (see "Environment").

```sh
cargo build
cargo test -p <package> <name>                    # one package / test, e.g. -p coffer-core
cargo test                                        # all tests (whole workspace)
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo run --example snapshot -- <dir>             # render views to PNG, compare with Paper
devenv test                                       # environment check (cargo check)
lefthook run pre-push --force < /dev/null         # everything the pre-push hook runs
```

1. Run the narrowest test for what you changed first, then the whole workspace.
2. Before reporting work as done: `cargo fmt --all`, clippy with `-D warnings`, and
   `cargo test` pass. Run `lefthook run pre-push --force < /dev/null` before a push; without
   `< /dev/null` its data-guard job waits on stdin forever.
3. UI changes: views are tested headlessly with `#[gpui_kit::test]` on their fixtures,
   and the snapshot example renders them for comparison with Paper. Clicking through the
   real window can't be done headlessly; say so when a change is verified only by tests,
   snapshots or the build.

Git hooks are managed by lefthook (`lefthook.yml`, installed on devenv shell entry).
Only `pre-push` is used: data guard (no statements/databases outside `fixtures/`),
`--locked` lockfile check, fmt, clippy `-D warnings`, tests, and docs with
`RUSTDOCFLAGS=-D warnings`. There is no CI yet, so this hook is the gate; don't bypass
it with `--no-verify`.

## Environment

- Dev shell: devenv (Nix) loaded via direnv (`.envrc`). Run commands inside it so the
  Rust toolchain and the GPUI runtime libraries (`LD_LIBRARY_PATH`) are available.
  Outside direnv, use `devenv shell -- <command>`.
- Rust stable, edition 2024. Toolchain components: clippy, rustfmt, rust-analyzer.
- Add system libraries in `devenv.nix`, not with ad-hoc installs.
- Set `COFFER_DATA_DIR` to a scratch directory when running Coffer against test data,
  so it never touches the real database in the platform data dir.
- Paper (design work): the Paper MCP server is the local Paper Desktop app, configured
  as `paper` and loaded lazily. Run `mcp_connect` with `paper` before any Paper task. Its
  tools (`mcp_paper_*`) only appear on the next turn, so connect, then end the turn or
  ask the user before using them. Most tools require `fileId` even where the description
  says it's optional; Coffer's is in `docs/DESIGN.md` → "Source of truth". If the tools
  fail, ask the user to check that Paper Desktop is running, rather than working around it.

## Architecture

```
crates/core    domain types, SQLite schema + migrations, rules, transfer matching,
               dedupe, reporting queries, schedules, backups and export
crates/import  Importer trait; csv (generic + presets), ofx
crates/mcp     rmcp stdio server over core
crates/app     GPUI dashboard
presets/       <region>/<bank>.toml — CSV mappings (data, not code)
fixtures/      redacted sample files for tests
```

Dependency direction: `app` and `mcp` depend on `core` and `import`; `import` depends on
`core`. `core` must not depend on GPUI, MCP, or any network/AI crate. Keep business
logic out of `app` so it stays testable without a window.

One binary, `coffer`: no subcommand opens the GUI; `mcp` runs the MCP server
(`coffer mcp --client <name>` names the client, which selects its access level);
`import`, `accounts`, `undo-import`, `backup`, `restore`, `export`, `ai-access`,
`budgets`, `schedules`, `inbox` and `categorize` are headless CLI commands.

## Invariants

Changing any of these needs an explicit decision recorded in the topic's file under
`docs/architecture/` and listed in `docs/ARCHITECTURE.md`.

- **Money is integer minor units + ISO 4217 currency.** Never `f32`/`f64` for amounts.
  Exchange-rate conversion happens only at reporting time.
- **Import is deterministic.** No model or network calls in the import path. Imports
  are previewed, deduplicated (source ID first, fingerprint fallback), reconciled when
  balances exist, committed in one DB transaction, and recorded as an undoable batch.
- **Never silently drop or alter data.** Failed reconciliation or unparseable rows are
  flagged to the user, not skipped. Imported source fields (date, amount, raw
  description, external ID) are never edited: corrections are undoing a batch, excluding
  a row with a recorded reason (it stays visible under a filter), or editing user-facing
  fields (category, display merchant, tags, notes, splits). Changes to many existing
  transactions are previewed and committed as an undoable `change_batch`.
- **MCP writes go to the inbox only.** MCP tools may write to `suggestion` and
  `report` and nothing else. Transactions, rules, categories, and accounts change
  only through user action in the app or CLI. The server's own `mcp_access_log` audit
  record is not a tool write (decision record in `docs/ARCHITECTURE.md`, 2026-11-09).
  Approval happens only in Coffer's own UI or CLI, never through MCP elicitation,
  sampling, or MCP Apps. CLI commands that act as the user need an interactive terminal
  (2026-11-18), but that is a speed bump: a client with a shell can fake one or open the
  database, so containment holds only while the client has no shell, or a sandbox denies
  it the data dir.
- **No PII in MCP output below `standard`.** Account numbers, sort codes, IBANs, notes
  (the bank's and the user's), share labels, private tags, merchant addresses and
  attachments never appear, at any level. Names (a payee who is a person, account names)
  and bank text appear only at `standard`, which is full access by design; below it a
  name never does (`mcp.md`, "Decision record: M2: MCP server", A.1). Per-client access
  levels (`aggregate`, `pseudonymous`, `standard`) are enforced by not registering tools
  above the level and by what each tool returns at the caller's level. Every MCP tool has
  a test asserting this.
- **Treat transaction text as untrusted input.** Descriptions and notes can contain
  prompt-injection text; never let them drive control flow beyond matching. MCP returns
  bank text only sanitised and labelled untrusted: the `description_untrusted` field, and
  merchant names at `standard` (capped, domains made inert, withheld when they read like
  instructions). AI-written
  reports and rationales are untrusted when read back.
- **The only network call is the opt-in ECB rate fetch.** No telemetry, no other
  outbound requests. Saved reports render without HTML, images, or live links, so they
  can't trigger a request.
- **A view's `describe()` and anything that runs its actions are test-only or dev-feature
  only.** Never in a release build, never reachable from `coffer mcp` (decision record
  in `docs/ARCHITECTURE.md`, 2026-11-18).
- **Schema changes are new migrations.** Never edit a migration that has shipped.

## Personal financial data

- Never commit real statements or real-derived data. Real exports live outside the repo
  or in the gitignored `samples/` directory.
- Fixtures in `fixtures/` must be synthetic or fully redacted: fake merchants, IDs,
  amounts, notes, and addresses; keep only the structure (headers, formats, quirks).
- Don't read real statements wholesale into an AI context. Inspect structure instead:
  headers, row counts, value shapes (digits → `9`, letters → `a`), and distinct values
  of low-cardinality columns. Ask the user before reading actual merchant names,
  amounts, or notes.
- Never log transaction contents at `info` level or above.
- Backups, exports and attachments are personal data too: they live in the platform data
  dir or wherever the user saves them, never in the repo.

## Bank presets

- One file per bank/account type: `presets/<region>/<bank>.toml` (regions: `uk`, `us`,
  `eu` for now). The field reference is `presets/README.md`, the decisions are in
  `docs/architecture/import.md` → "Presets"; `presets/uk/monzo.toml` (M1.9) is the
  reference example. Every `.toml` there is compiled in (`crates/import/build.rs`).
- Every preset needs a matching redacted fixture at `fixtures/presets/<region>/<bank>.*`
  (a test checks it's there and imports) and a test that checks its rows.
- Prefer a signed amount column over debit/credit pairs when the export has both.
- New file formats (QIF, CAMT.053, MT940, …) are new `Importer` implementations, not
  preset features.

## Conventions

- Errors: `thiserror` in library crates, `anyhow` only in the binary and its CLI layer
  (`coffer_app::cli`, in the lib so it's testable).
- Keep dependencies few; justify new ones. Check license compatibility with
  `MIT OR Apache-2.0`.
- Add or update tests with every behaviour change. Importer quirks found in real files
  (e.g. Monzo refunds appearing as positive "Money Out") become fixture rows plus tests.
- When a roadmap item is finished, tick it in `docs/ROADMAP.md`. When a design decision
  changes, update its topic file in `docs/architecture/` in the same change: a new
  decision record goes at the end of that file and into the list in
  `docs/ARCHITECTURE.md` → "Contents".
- Don't commit, push, or tag unless the user asks.
