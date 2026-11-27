//! Orre's GPUI dashboard: theme, UI components and screens, and the
//! headless CLI commands ([`cli`]). The `orre` binary (`src/main.rs`) is
//! built from this crate.
//!
//! # Sample data
//!
//! The `fixture` modules (sample accounts, transactions, settings…), the
//! component showcase and `orre --showcase` are compiled only under
//! `cfg(test)` and the `dev-samples` feature, which no release path
//! enables: a finance app must not show invented money as if it were real
//! (W1-3). Without it, naming any of them doesn't compile; these examples
//! check that, one module each (and, with `dev-samples`, that they name
//! real items).
//!
//! Accounts:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::accounts::fixture::today;
//! ```
//!
//! Overview:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::overview::fixture::october;
//! ```
//!
//! Import:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::import::fixture::setup;
//! ```
//!
//! Checks:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::checks::fixture::seed;
//! ```
//!
//! Transactions:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::transactions::fixture::id;
//! ```
//!
//! Transactions (links):
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::transactions::link_fixture::seed;
//! ```
//!
//! Inbox:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::inbox::fixture::seed;
//! ```
//!
//! Settings:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::settings::fixture::clock;
//! ```
//!
//! Rules:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::rules::fixture::seed;
//! ```
//!
//! The showcase:
#![cfg_attr(not(feature = "dev-samples"), doc = "```compile_fail")]
#![cfg_attr(feature = "dev-samples", doc = "```no_run")]
//! let _ = orre_app::showcase::init;
//! ```

pub mod accounts;
pub mod checks;
pub mod cli;
#[cfg(feature = "dev-driver")]
pub mod dev_driver;
pub mod format;
pub mod import;
pub mod inbox;
pub mod overview;
pub mod rules;
pub mod settings;
pub mod shell;
#[cfg(any(test, feature = "dev-samples"))]
pub mod showcase;
pub mod theme;
pub mod transactions;
pub mod ui;

/// Today on the computer's clock, in its time zone. When the local date
/// can't be read, the UTC date, with a warning logged: never a sample day,
/// which a release build doesn't have (W1-3).
pub fn today() -> chrono::NaiveDate {
    orre_core::schedule::local_today().unwrap_or_else(|error| {
        tracing::warn!(%error, "couldn't read the local date; using the UTC date");
        chrono::Utc::now().date_naive()
    })
}
