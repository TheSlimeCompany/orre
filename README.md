# Orre

A local-first personal finance dashboard. Orre imports bank exports (CSV, OFX/QFX)
into a local SQLite database, shows them as tables and charts in a native desktop app,
and lets AI clients (Claude Code, Claude Desktop, Codex) query your data through an
MCP server.

> **Status:** pre-release. v0.1 is not out yet, and there are no packages or release
> builds yet: build it from source (see "Install"). The release workflow that will
> build them, and how to install them, is in [`docs/install.md`](docs/install.md).
> Linux and macOS are the official platforms; Windows is best-effort. The headless
> core, the importers, the MCP server and the desktop app's day-to-day screens are
> built; analytics (M4) has started with the monthly Overview.
> See [`docs/ROADMAP.md`](docs/ROADMAP.md) for what's done and what comes next, and
> [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for how it works.

## What it does

- **Import.** Bank exports go into a local SQLite database: OFX/QFX from any bank that
  exports it, CSV with the bundled Monzo preset, and any other CSV once you map its
  columns and save the mapping as a preset. To add your bank as a preset, see
  [`presets/README.md`](presets/README.md). More bank presets are planned: see the
  [roadmap](docs/ROADMAP.md). Every import is previewed,
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
  saving reports. Notes are never returned, and bank text is checked for common account
  identifiers (sort codes, IBANs, long digit runs) before it's returned, but Orre doesn't
  remove every personal detail from free text. Descriptions are stored as the bank wrote
  them; only a statement's own account number is kept as a keyed hash instead. At
  `standard`, the full-access level, clients see account and merchant names and can ask
  for bank descriptions, which may contain details such as addresses.
- **Exact money.** Amounts are integer minor units plus an ISO 4217 currency code.

## Install

There are no release downloads yet. When v0.1 is tagged, its builds will be published on
the [releases page](https://github.com/TheSlimeCompany/orre/releases): a Linux tarball
(no Nix needed), a Nix flake, and an unsigned macOS `.dmg`. Until then, build from
source. [`docs/install.md`](docs/install.md) covers the packaged builds: the
[Linux tarball](docs/install.md#linux-tarball), [Nix](docs/install.md#nix-linux), and
the [macOS `.dmg`](docs/install.md#macos-unsigned-dmg), including
[how to open it past Gatekeeper](docs/install.md#why-macos-warns-you-and-how-to-open-it).

### Build from source

This works on Linux and macOS without Nix.

1. Install Rust with [rustup](https://rustup.rs). In the repository it follows
   `rust-toolchain.toml` (stable, with rustfmt and clippy). The minimum supported Rust
   is 1.95.
2. Install the build dependencies. On Debian or Ubuntu, these are the packages CI
   installs (`.github/workflows/ci.yml`):

   ```sh
   sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
     libvulkan-dev libfontconfig-dev libfreetype-dev libx11-xcb-dev libxcb1-dev \
     libasound2-dev libzstd-dev pkg-config clang libssl-dev
   ```

   On macOS, install Xcode or the Command Line Tools. GPUI compiles its shaders with
   `xcrun metal`; if `xcrun -f metal` fails, run
   `xcodebuild -downloadComponent MetalToolchain`.
3. Build and run:

   ```sh
   git clone https://github.com/TheSlimeCompany/orre.git
   cd orre
   cargo build --release --locked -p orre-app
   ./target/release/orre
   ```

Orre keeps its data in `~/.local/share/orre` on Linux and
`~/Library/Application Support/Orre` on macOS, or in `ORRE_DATA_DIR` if you set it.

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

Contributions are welcome: see [`CONTRIBUTING.md`](CONTRIBUTING.md) and the
[Code of Conduct](CODE_OF_CONDUCT.md).

The supported dev environment is [devenv](https://devenv.sh) (Nix), loaded via direnv.
It is optional: it provides the Rust toolchain, the runtime libraries GPUI needs and the
git hooks, but rustup and your system's libraries work too (see "Install").

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
GitHub Actions CI (`.github/workflows/ci.yml`) runs the same checks on every pull request
on Linux and macOS, plus a minimum-Rust-version check. The release workflow
(`.github/workflows/release.yml`, run by hand or by a `v*` tag) builds the downloads;
see [`docs/install.md`](docs/install.md).

## Documentation

- [`VISION.md`](VISION.md): the long-term direction, principles and non-goals
- [`docs/install.md`](docs/install.md): installing Orre's release builds: the
  [Linux tarball](docs/install.md#linux-tarball), [Nix](docs/install.md#nix-linux) and
  the [macOS `.dmg`](docs/install.md#macos-unsigned-dmg) (and
  [opening it past Gatekeeper](docs/install.md#why-macos-warns-you-and-how-to-open-it))
- [`docs/mcp-setup.md`](docs/mcp-setup.md): connecting an AI client
- [`presets/README.md`](presets/README.md): add your bank as a CSV preset, no Rust required
- [`docs/data-and-privacy.md`](docs/data-and-privacy.md): where your data lives, backups and privacy
- [`SECURITY.md`](SECURITY.md): what Orre protects, what it doesn't, and how to report a vulnerability
- [`CONTRIBUTING.md`](CONTRIBUTING.md): building, testing and sending changes
- [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md): the Contributor Covenant
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md): design overview and decisions; the
  data model, import pipeline, MCP surface and each topic's decision records are in
  [`docs/architecture/`](docs/architecture/)
- [`presets/README.md`](presets/README.md): add your bank as a CSV preset, no Rust required
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
