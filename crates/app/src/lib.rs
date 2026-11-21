//! Orre's GPUI dashboard: theme, UI components and screens, and the
//! headless CLI commands ([`cli`]). The `orre` binary (`src/main.rs`) is
//! built from this crate.

pub mod accounts;
pub mod checks;
pub mod cli;
#[cfg(feature = "dev-driver")]
pub mod dev_driver;
pub mod format;
pub mod import;
pub mod inbox;
pub mod rules;
pub mod settings;
pub mod shell;
pub mod showcase;
pub mod theme;
pub mod transactions;
pub mod ui;
