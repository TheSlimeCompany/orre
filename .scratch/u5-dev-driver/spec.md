# U.5 — dev driver

Status: done (2026-11-21)

Source: `docs/ROADMAP.md` → "UI review", U.5; the rules are in `docs/architecture/app.md` →
"Decision record: views that describe themselves stay out of the release binary"
(2026-11-18). The user picked the scope on 2026-11-21: spike first, then every view and dialog,
with an `orre dev` subcommand as the client.

## Problem

An agent that changes the UI can only report it as verified by tests, snapshots or the build.
It can't check the running app. U.1–U.3 gave every built view a `describe()`, but only tests
can call it.

## What it is

- A Cargo feature `dev-driver` on `orre-app`. It's off by default, no other feature enables
  it, and it fails to compile without `debug_assertions` (`compile_error!`), so a release build
  can't have it. It turns on `gpui-kit/test-support`, which drives the window with the same
  calls the UI tests use (`click(id)`, `press(key)`, `input(text)`). The spike on 2026-11-21
  showed these work on a real window.
- The `describe` modules and what they read are compiled under
  `any(test, feature = "dev-driver")`. Test helpers that need a `VisualTestContext` stay under
  `cfg(test)`.
- When the GUI opens in a dev-driver build, the driver listens on a Unix socket
  `$ORRE_DATA_DIR/dev-driver.sock` (mode 0600). It starts only when `ORRE_DATA_DIR` is set and
  isn't the platform data dir. Otherwise the window opens without a driver and logs why. A path
  too long for a socket gets an error that says so.
- `orre dev <command>`, compiled only with the feature, sends one command and prints the
  reply. Commands:
  - `describe`: the view that's open and its description, the dialog that's open and its
    description, and focus.
  - `show <view>`: clicks the view's tab.
  - `press <keys>`: space-separated keystrokes, as the tests write them.
  - `type <text>`: text into the focused field.
  - `click <element id>`.
  - `run <label>`: clicks the one visible, enabled control with that accessible label,
    inside the open dialog when there is one. Built differently from the first draft,
    which gave each `ui::describe::Action` a `target` element id: the labels were already
    there and are checked instead (decision record "the dev driver", point 2).
- Replies are the description's pretty `Debug` text, or `error: …`. No serde on the
  description types.

## Rules kept (decision record 2026-11-18, points 1–5)

- It's never in a release build. `orre mcp` reaches none of it: the driver starts only from
  the GUI's `shell::open`, and `orre dev` only talks to the socket.
- It runs only against a scratch data dir, on a local socket.
- `describe()` still shows only what the window shows, through `envelope::shown`.

## Acceptance

- `cargo build` (default features) and `cargo build --release` contain no driver.
  `cargo build --release --features dev-driver` fails.
- A test for every view checks that the driver can click each enabled button its
  `describe()` lists.
- A test drives the shell through the driver's command handler (not the socket):
  `describe`, `show`, `press`, `run`, plus errors for an unknown action, an unknown element
  and a bad keystroke.
- The driver refuses to start without `ORRE_DATA_DIR`, and on the platform data dir (unit
  test).
- The invariant check in `crates/core/tests/invariants.rs` allows `describe` modules gated on
  `dev-driver`, and nothing else.
- The docs are updated: a new decision record in `docs/architecture/app.md` (listed in
  `ARCHITECTURE.md`), U.5 ticked in the ROADMAP, the AGENTS.md status and Build lines, and
  CODEMAP.

## Out of scope

- Hover. The description says what hover would show, as before.
- Drags. Every drag already has a keyboard path (U.8, U.9).
- Views that aren't built yet (Overview, Trends, Budgets, Net worth, Reviews). `describe`
  says the view isn't built.
