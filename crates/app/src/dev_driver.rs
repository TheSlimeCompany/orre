//! The dev driver (ROADMAP U.5): lets an agent check a UI change in the
//! running app instead of reporting it as build-verified. A dev build opens
//! a local socket; `orre dev <command>` sends one line and prints the reply.
//!
//! Commands ([`Command`]): `describe` (the window as plain data:
//! [`crate::shell::describe`]), `show <view>`, `press <keys>`,
//! `type <text>`, `click <element id>` and `run <label>` (click the one
//! visible, enabled control with that accessible label, inside the open
//! dialog when there is one).
//!
//! Only in a debug build with the `dev-driver` feature, which is off by
//! default and enabled by no other feature; a release build with it doesn't
//! compile. The GUI starts it only against a scratch `ORRE_DATA_DIR`
//! ([`guard`]), never the platform data dir, and `orre mcp` can't reach it
//! (`docs/architecture/app.md` → "Decision record: the dev driver").
//!
//! It drives the window with the calls the UI tests use (gpui-kit's
//! `test-support`), on the real window: a click is a pointer move, press
//! and release at the control's centre, so it goes through hit-testing as a
//! person's would.

#[cfg(not(debug_assertions))]
compile_error!(
    "the `dev-driver` feature is for debug builds only: it must never be in a release build \
     (docs/architecture/app.md → \"Decision record: the dev driver\")"
);

mod act;
pub mod client;
mod command;
pub mod guard;
mod server;
#[cfg(test)]
mod tests;

pub use act::act;
pub use command::Command;
pub use server::start;
