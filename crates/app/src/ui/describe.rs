//! What every view's test-only `describe()` shares (ROADMAP U.1–U.3): the
//! actions a view offers, as plain data.
//!
//! Compiled for tests only (`#[cfg(test)]` on the module), like each
//! view's `describe()`: nothing here is in a release build or reachable
//! from `coffer mcp` (`docs/architecture/app.md` → "Decision record: views
//! that describe themselves stay out of the release binary").

/// Something the user can do, as the screen offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub label: String,
    /// The key that does it (as bound, e.g. "a", "escape"); `None` for a
    /// button.
    pub key: Option<&'static str>,
    /// Whether doing it would do anything now (a disabled button, a key
    /// with nothing to act on).
    pub enabled: bool,
}

impl Action {
    /// An action done with `key`.
    pub fn keyed(label: &str, key: &'static str, enabled: bool) -> Self {
        Self {
            label: label.to_owned(),
            key: Some(key),
            enabled,
        }
    }

    /// A button with no key of its own.
    pub fn button(label: impl Into<String>, enabled: bool) -> Self {
        Self {
            label: label.into(),
            key: None,
            enabled,
        }
    }
}
