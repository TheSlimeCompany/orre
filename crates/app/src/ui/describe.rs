//! What every view's test-only `describe()` shares (ROADMAP U.1–U.3): the
//! actions a view offers, the rows of a left panel, an inspector and a
//! dialog, as plain data.
//!
//! Compiled for tests only (`#[cfg(test)]` on the module), like each
//! view's `describe()`: nothing here is in a release build or reachable
//! from `coffer mcp` (`docs/architecture/app.md` → "Decision record: views
//! that describe themselves stay out of the release binary").

use crate::ui::keys::{Key, ViewKeys};
use crate::ui::list::Keys;

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

    /// A list's movement keys ([`Keys::moves`]), each doing something when
    /// `enabled` (the list has rows).
    pub fn moves(keys: &Keys, enabled: bool) -> Vec<Self> {
        keys.moves()
            .into_iter()
            .map(|(stroke, to)| Self::keyed(to.label(), stroke, enabled))
            .collect()
    }

    /// A view's keys in declared order, every keystroke of each, then its
    /// list's moves: `enabled` says whether a key does anything now, and
    /// `rows` whether the list has rows to move through.
    pub fn view_keys(keys: &ViewKeys, enabled: impl Fn(&Key) -> bool, rows: bool) -> Vec<Self> {
        keys.keys()
            .iter()
            .flat_map(|key| {
                let on = enabled(key);
                key.keys()
                    .iter()
                    .map(move |stroke| Self::keyed(key.label(), stroke, on))
            })
            .chain(Self::moves(keys.list(), rows))
            .collect()
    }
}

/// A row of a left panel: what it's called, the count after it, and
/// whether it's the one open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRow {
    pub label: &'static str,
    pub count: Option<String>,
    /// Marked as needing attention (a `danger` dot).
    pub flagged: bool,
    pub selected: bool,
}

/// An inspector's header: its title, the figure on the right, and the
/// caption under them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub figure: Option<String>,
    pub caption: Option<String>,
}

/// A section of an inspector: its title, its label and value rows, and its
/// buttons. Fixed explanatory captions are left out; a sentence with the
/// data in it (a figure, a date, a refusal) is a row labelled "".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub title: Option<String>,
    /// Each row's label ("" for a row continuing the one above) and value,
    /// as drawn.
    pub rows: Vec<(String, String)>,
    pub actions: Vec<Action>,
}

impl Section {
    /// A section titled `title`.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// Adds a row.
    pub fn row(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }

    /// Adds a button.
    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }
}

/// An inspector showing something: its header and sections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspector {
    pub header: Header,
    pub sections: Vec<Section>,
}

/// A dialog open over the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    /// Its fields, by label, with the value shown.
    pub fields: Vec<(String, String)>,
    /// Why the last confirmation was refused.
    pub error: Option<String>,
    /// Its buttons, Cancel first.
    pub actions: Vec<Action>,
}
