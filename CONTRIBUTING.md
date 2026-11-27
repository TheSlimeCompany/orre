# Contributing to Orre

Thanks for helping. Orre is a one-person project, so reviews are best effort. Everyone
taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md). Report security problems
privately, as [`SECURITY.md`](SECURITY.md) describes, not in an issue.

Before larger changes, read [`VISION.md`](VISION.md) (its non-goals rule some work out)
and open an issue to talk it through. [`AGENTS.md`](AGENTS.md) holds the project's
rules and invariants; they apply to people as much as to AI agents.

## Adding your bank

Most banks need no Rust: a CSV preset is a TOML file plus a redacted sample. See
[`presets/README.md`](presets/README.md).

## Building

Either:

- **rustup and your system's libraries.** rustup follows `rust-toolchain.toml` (stable
  with rustfmt and clippy); the minimum supported Rust is 1.95. The Linux and macOS
  build dependencies are in the README, under "Build from source".
- **devenv (optional).** The supported dev environment: [devenv](https://devenv.sh)
  (Nix), loaded with `direnv allow`, or run a command with `devenv shell -- <command>`.
  It provides the toolchain, GPUI's runtime libraries and lefthook.

```sh
cargo build
cargo run                     # opens the app
```

Set `ORRE_DATA_DIR` to a scratch folder when you run Orre against test data, so it
never touches your real database.

## Testing

Run the narrowest test first, then the whole workspace:

```sh
cargo test -p orre-core               # one crate: orre-core, orre-import, orre-mcp, orre-app
cargo test -p orre-core <name>        # one test
cargo test                            # the workspace
```

Before you send a change, these must pass:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The [lefthook](https://lefthook.dev) `pre-push` hook runs these and more (the lockfile,
docs, the crate dependency direction, a guard against bank data and against edits to
shipped migrations). devenv installs it; run it by hand with:

```sh
lefthook run pre-push --force < /dev/null
```

The `< /dev/null` is needed: without it the data guard waits on stdin. CI
(`.github/workflows/ci.yml`) runs the same checks on Linux and macOS for every pull
request.

## Rules that matter most

- **Never commit real bank exports or databases.** Fixtures in `fixtures/` must be
  synthetic or fully redacted: fake merchants, IDs, amounts and notes.
- **Never edit a SQL migration that has shipped.** Schema changes are new migrations.
- Add or update tests with every behaviour change.

The full list is under "Invariants" in [`AGENTS.md`](AGENTS.md).

## Pull requests

Open pull requests against `main`. CI must pass before a change is merged. Keep each
pull request to one change, and say in its description what it fixes or adds and how
you checked it.

By contributing, you agree that your work is dual licensed under MIT OR Apache-2.0, as
the README's "License" section says.
