//! What a batch of previews reads once ([`super::preview::preview_all`]).
//!
//! Previewing a rule suggestion reads every transaction (its impact, its
//! rows, its approval), and the Inbox previews every pending suggestion in
//! a row. [`Shared`] holds the ledger as the rules read it
//! ([`rule::History`]) and the rows the last rule matched, so the batch
//! reads the ledger once and each rule finds its rows once.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use rusqlite::Connection;

use crate::rule::{History, RuleDefinition};

/// The ledger and the rows a rule matches in it, read when first needed.
/// Valid while the database isn't changed under it: the previews roll back
/// what they write.
#[derive(Default)]
pub(crate) struct Shared {
    history: OnceCell<History>,
    /// The definition last matched, and the positions of the rows it
    /// matches: a rule's impact, its rows and its approval ask in turn.
    matched: RefCell<Option<(RuleDefinition, Rc<Vec<usize>>)>>,
}

impl Shared {
    /// The ledger, read on the first call.
    pub(crate) fn history(&self, conn: &Connection) -> rusqlite::Result<&History> {
        // `OnceCell::get_or_try_init` isn't stable.
        if self.history.get().is_none() {
            let _ = self.history.set(History::load(conn)?);
        }
        Ok(self.history.get().expect("the ledger was just read"))
    }

    /// The positions of the rows `definition` matches ([`History::matching`]).
    pub(crate) fn matched(
        &self,
        conn: &Connection,
        definition: &RuleDefinition,
    ) -> rusqlite::Result<Rc<Vec<usize>>> {
        if let Some((last, matched)) = &*self.matched.borrow()
            && last == definition
        {
            return Ok(matched.clone());
        }
        let matched = Rc::new(self.history(conn)?.matching(definition));
        *self.matched.borrow_mut() = Some((definition.clone(), matched.clone()));
        Ok(matched)
    }
}
