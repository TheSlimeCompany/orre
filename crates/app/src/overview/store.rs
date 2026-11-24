//! Reading what the Overview shows, the way [`crate::transactions::store`]
//! reads the register: no connection is held between calls. Each call
//! opens the database at its path through [`gate::open`], which checks the
//! restore lock, so a restore can swap the file.
//!
//! The read model is core's ([`overview`]); this only opens the database.
//! It reads the chart's months, every older line once and the recurring
//! series, so the view runs it on a background thread ([`OverviewView::reload`](super::OverviewView::reload)); the store
//! is `Clone` and holds only a path, so it can go there.

use std::path::PathBuf;

use orre_core::backup::gate;
use orre_core::db::Database;
use orre_core::reporting::overview::{self, Overview, OverviewQuery};

use crate::accounts::store::StoreError;

/// The overview of the database at `database`.
#[derive(Clone, Debug)]
pub struct OverviewStore {
    database: PathBuf,
}

impl OverviewStore {
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// What the screen shows for `query`.
    pub fn load(&self, query: &OverviewQuery) -> Result<Overview, StoreError> {
        let db = gate::open(&self.database, Database::open)?;
        Ok(overview::overview(&db.reader()?, query)?)
    }

    /// What the screen shows for `query` with its month replaced by the
    /// month the screen opens on ([`overview::landing_month`]); the result's
    /// `selected.period` says which it was.
    pub fn land(&self, query: &OverviewQuery) -> Result<Overview, StoreError> {
        let db = gate::open(&self.database, Database::open)?;
        let reader = db.reader()?;
        let month = overview::landing_month(&reader, query.account, query.today)?;
        Ok(overview::overview(
            &reader,
            &OverviewQuery { month, ..*query },
        )?)
    }
}
