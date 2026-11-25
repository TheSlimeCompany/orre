# Orre

A local-first personal finance dashboard. Orre imports bank exports (CSV, OFX/QFX)
into a local SQLite database, shows them as tables and charts in a native desktop app,
and lets AI clients (Claude Code, Claude Desktop, Codex) query your data through an
MCP server.

> **Status:** pre-release. v0.1 is not out yet, and there are no packages or release
> builds: build it from source (see "Development"). Linux and macOS are the official
> platforms. The headless core, the importers, the MCP server and the desktop app's
> day-to-day screens are built; analytics (M4) has started with the monthly Overview.
> See [`docs/ROADMAP.md`](docs/ROADMAP.md) for what's done and what comes next, and
> [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for how it works.

## What it does

- **Import.** CSV (with bank presets, or a column mapping you can save as a preset)
  and OFX/QFX go into a local SQLite database. Every import is previewed,
  deduplicated (the bank's ID first, a fingerprint otherwise), reconciled against the
  file's balances where it has them, committed in one transaction, and undoable as a
  batch.
- **Desktop app.** A native GPUI app: accounts (including pots,
  cash, and manual assets and liabilities), a transactions table with categories,
  splits, tags, transfer and refund linking, rules and categories editors,
  possible-duplicates and balance checks, backups and export, AI access settings, and a
  monthly Overview. It picks up changes made by another process (an import in a
  terminal, an AI client's suggestion) while it's open.
- **CLI.** `orre import`, `accounts`, `undo-import`, `backup`, `restore`, `export`,
  `ai-access`, `budgets`, `schedules`, `inbox` and `categorize` work without the GUI.
- **MCP server.** `orre mcp` gives AI clients read tools over your data, limited by the
  access level you give each client (`aggregate`, `pseudonymous` or `standard`), and
  off until you enable a client. A client can only file suggestions and save reports.
  Suggestions wait in an approval inbox until you approve or reject them in Orre;
  reports are saved straight away (`orre inbox report <ID>` shows one). Calls to
  registered tools are recorded in an access log.
  Setup: [`docs/mcp-setup.md`](docs/mcp-setup.md).

## Principles

- **Local-first.** Your data stays in a SQLite file on your machine. The only network
  call Orre plans to make is an opt-in exchange-rate fetch (ECB, not built yet). No
  telemetry. An AI client you connect may send Orre's tool results to its model
  provider.
- **Deterministic import.** Imports are previewed, deduplicated, reconciled against
  balances where possible, committed atomically, and undoable as a batch.
- **Nothing silently dropped.** Unparseable rows and failed reconciliations are
  flagged, not skipped.
- **Safe AI access.** MCP tools are read-only apart from filing suggestions and
  saving reports. Notes are never returned, Orre stores no account numbers from your
  files, and bank text is checked for common account identifiers (sort codes, IBANs,
  long digit runs), but Orre doesn't remove every personal detail from free text. At `standard`, the full-access level, clients see account and
  merchant names and can ask for bank descriptions, which may contain details such as
  addresses.
- **Exact money.** Amounts are integer minor units plus an ISO 4217 currency code.

## Usage

```sh
orre                             # open the dashboard
orre import <file>               # import a bank export
orre accounts                    # list accounts
orre undo-import <batch>         # preview undoing an import; add --yes to undo it
orre inbox                       # review MCP suggestions
orre mcp --client <name>         # run the MCP server over stdio for one AI client
orre --help                      # every command
```

## Development

The dev environment is managed with [devenv](https://devenv.sh) (Nix) and loaded via
direnv. It provides the Rust toolchain and the runtime libraries GPUI needs.

```sh
direnv allow           # or prefix commands with: devenv shell --
cargo build
cargo run
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

A [lefthook](https://lefthook.dev) `pre-push` hook (installed automatically when the
devenv shell loads) runs fmt, clippy, tests, and docs, and blocks pushing bank exports
or databases outside `fixtures/`. Run it by hand with `lefthook run pre-push --force < /dev/null`.

## Documentation

- [`SECURITY.md`](SECURITY.md): what Orre protects, what it doesn't, and how to report a vulnerability
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md): design overview and decisions; the
  data model, import pipeline, MCP surface and each topic's decision records are in
  [`docs/architecture/`](docs/architecture/)
- [`docs/ROADMAP.md`](docs/ROADMAP.md): milestones and status
- [`docs/DESIGN.md`](docs/DESIGN.md): visual design system and tokens
- [`docs/CODEMAP.md`](docs/CODEMAP.md): where each module is, by crate and area
- [`AGENTS.md`](AGENTS.md): guidelines for AI coding agents

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

The bundled fonts in [`assets/fonts/`](assets/fonts/) (Inter and Geist Mono) are
licensed separately under the SIL Open Font License 1.1; see the `OFL.txt` beside each.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
