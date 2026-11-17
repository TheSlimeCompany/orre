//! Hand-built ledgers for the reporting tests: a fresh database in a
//! temporary directory, with helpers that write through core's own API
//! (manual rows, splits, links, shares, exclusions, transfers, balances and
//! tags), so each test states exactly the rows it counts.
//!
//! Dates are written `YYYY-MM-DD`; amounts are minor units in the account's
//! currency, negative for money out; categories are keys.

// Each test module uses its own helpers.
#![allow(dead_code)]

use chrono::NaiveDate;
use rusqlite::OptionalExtension;
use tempfile::TempDir;

use crate::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
use crate::category::CategoryKey;
use crate::db::{Database, Reader};
use crate::exclusion::{self, ExclusionReason};
use crate::id::{AccountId, BatchId, LinkId, SplitId, TransactionId};
use crate::link::{self, LinkKind, LinkTarget, NewLink, ShareLabel, Sharing};
use crate::manual::{self, ManualTransaction};
use crate::money::{Currency, Money};
use crate::split::{self, SplitLine};
use crate::tag::{self, TagName};
use crate::transfer;

/// A `YYYY-MM-DD` date.
pub(crate) fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

/// The share label every shared row gets. It never leaves the app.
pub(crate) const SHARE_LABEL: &str = "Test Housemate";

/// A database for one test.
pub(crate) struct TestLedger {
    _dir: TempDir,
    pub db: Database,
    /// While set, `row_at` writes imported rows.
    importing: bool,
}

impl TestLedger {
    pub fn new() -> TestLedger {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        TestLedger {
            _dir: dir,
            db,
            importing: false,
        }
    }

    /// A read-only connection, as the MCP server's.
    pub fn reader(&self) -> Reader {
        self.db.reader().unwrap()
    }

    /// A current account.
    pub fn account(&mut self, name: &str, currency: Currency) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Current, currency),
        )
        .unwrap()
    }

    /// A pot under `parent`, in its currency.
    pub fn pot(&mut self, parent: AccountId, name: &str) -> AccountId {
        let currency = account::get(&self.db, parent).unwrap().currency;
        let mut pot = NewAccount::new(name, AccountKind::Pot, currency);
        pot.parent = Some(parent);
        account::create(&mut self.db, &pot).unwrap()
    }

    /// An `asset` or `liability` account.
    pub fn valuation_account(
        &mut self,
        name: &str,
        kind: AccountKind,
        currency: Currency,
    ) -> AccountId {
        account::create(&mut self.db, &NewAccount::new(name, kind, currency)).unwrap()
    }

    fn currency(&self, account: AccountId) -> Currency {
        account::get(&self.db, account).unwrap().currency
    }

    /// A manual row, optionally categorised (category source `user`); an
    /// imported one while [`TestLedger::start_importing`] is in effect.
    pub fn row(
        &mut self,
        account: AccountId,
        day: &str,
        minor: i64,
        category: Option<&str>,
    ) -> TransactionId {
        self.row_at(account, day, minor, category, None, "TEST ROW")
    }

    /// Makes the rows written from now on imported rows, each in its
    /// account's batch ([`TestLedger::batch_of`]), until
    /// [`TestLedger::stop_importing`]. They are as an import leaves them: a
    /// fingerprint, and the category's source `rule`.
    pub fn start_importing(&mut self) {
        self.importing = true;
    }

    /// Goes back to writing manual rows.
    pub fn stop_importing(&mut self) {
        self.importing = false;
    }

    /// The import batch `account`'s imported rows go in, made on first use.
    pub fn batch_of(&mut self, account: AccountId) -> BatchId {
        let conn = self.db.conn();
        let existing = conn
            .query_row(
                "SELECT id FROM import_batch WHERE account_id = ?1",
                [account],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        if let Some(batch) = existing {
            return batch;
        }
        conn.execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (?1, 'statement.csv', 'csv')",
            [account],
        )
        .unwrap();
        BatchId::new(conn.last_insert_rowid())
    }

    fn imported_row(
        &mut self,
        account: AccountId,
        day: &str,
        minor: i64,
        category: Option<&str>,
        description: &str,
    ) -> TransactionId {
        let currency = self.currency(account);
        let batch = self.batch_of(account);
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, fingerprint, origin, \
             category_id, category_source) \
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, upper(?6), 'fp-' || ?3 || '-' || ?4 || '-' || ?6 \
                    || '-' || (SELECT count(*) FROM \"transaction\"), \
                    'imported', c.id, CASE WHEN c.id IS NULL THEN NULL ELSE 'rule' END \
               FROM (SELECT ?7 AS key) k LEFT JOIN category c ON c.key = k.key",
            rusqlite::params![account, batch, day, minor, currency, description, category],
        )
        .unwrap();
        TransactionId::new(conn.last_insert_rowid())
    }

    /// A manual row with a merchant (its display name is derived from it)
    /// and bank text. While [`TestLedger::start_importing`] is in effect, an
    /// imported row with no merchant: `merchant` is ignored.
    pub fn row_at(
        &mut self,
        account: AccountId,
        day: &str,
        minor: i64,
        category: Option<&str>,
        merchant: Option<&str>,
        description: &str,
    ) -> TransactionId {
        if self.importing {
            return self.imported_row(account, day, minor, category, description);
        }
        let currency = self.currency(account);
        let mut entry =
            ManualTransaction::new(account, date(day), Money::new(minor, currency), description);
        entry.merchant = merchant.map(str::to_owned);
        entry.category = category.map(|key| CategoryKey::new(key).unwrap());
        manual::create(&mut self.db, &entry).unwrap()
    }

    /// Splits `transaction` into lines of `(minor, category)`.
    pub fn split(
        &mut self,
        transaction: TransactionId,
        lines: &[(i64, Option<&str>)],
    ) -> Vec<SplitId> {
        let currency = self.transaction_currency(transaction);
        let lines: Vec<SplitLine> = lines
            .iter()
            .map(|&(minor, category)| {
                SplitLine::new(
                    Money::new(minor, currency),
                    category.map(|key| CategoryKey::new(key).unwrap()),
                )
            })
            .collect();
        split::set(&mut self.db, transaction, &lines).unwrap()
    }

    /// A confirmed refund of `minor` from `inflow` to `purchase`.
    pub fn refund(&mut self, inflow: TransactionId, purchase: TransactionId, minor: i64) -> LinkId {
        self.link(LinkKind::Refund, inflow, purchase, minor)
    }

    /// A confirmed reimbursement of `minor` from `inflow` to `purchase`
    /// (which must be claimed back first, [`TestLedger::claim_back`]).
    pub fn reimburse(
        &mut self,
        inflow: TransactionId,
        purchase: TransactionId,
        minor: i64,
    ) -> LinkId {
        self.link(LinkKind::Reimbursement, inflow, purchase, minor)
    }

    fn link(
        &mut self,
        kind: LinkKind,
        inflow: TransactionId,
        purchase: TransactionId,
        minor: i64,
    ) -> LinkId {
        let currency = self.transaction_currency(inflow);
        link::create(
            &mut self.db,
            &NewLink {
                kind,
                inflow,
                target: LinkTarget::Purchase(purchase),
                amount: Money::new(minor, currency),
            },
        )
        .unwrap()
    }

    /// Shares `transaction` with [`SHARE_LABEL`]: the user's part is
    /// `my_share_minor` (with the amount's sign).
    pub fn share(&mut self, transaction: TransactionId, my_share_minor: i64) {
        let currency = self.transaction_currency(transaction);
        link::set_sharing(
            &mut self.db,
            transaction,
            &Sharing::Shared {
                my_share: Money::new(my_share_minor, currency),
                label: ShareLabel::new(SHARE_LABEL).unwrap(),
            },
        )
        .unwrap();
    }

    /// Marks `transaction` as claimed back as a whole.
    pub fn claim_back(&mut self, transaction: TransactionId) {
        link::set_sharing(&mut self.db, transaction, &Sharing::ClaimBack).unwrap();
    }

    /// Excludes `transaction` (`Other` gets a note).
    pub fn exclude(&mut self, transaction: TransactionId, reason: ExclusionReason) {
        let note = reason.needs_note().then_some("test");
        exclusion::exclude(&mut self.db, transaction, reason, note).unwrap();
    }

    /// Pairs `a` and `b` as a confirmed transfer.
    pub fn transfer(&mut self, a: TransactionId, b: TransactionId) {
        transfer::record_confirmed_pair(self.db.conn(), a, b).unwrap();
    }

    /// Pairs `a` and `b` as a transfer Coffer only suggested, as detection
    /// writes it.
    pub fn suggested_transfer(&mut self, a: TransactionId, b: TransactionId) {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO transfer_group (status) VALUES ('suggested')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id IN (?2, ?3)",
            rusqlite::params![conn.last_insert_rowid(), a, b],
        )
        .unwrap();
    }

    /// A refund of `minor` from `inflow` to `purchase` that Coffer only
    /// suggested, as its link suggestions write it.
    pub fn suggested_refund(&mut self, inflow: TransactionId, purchase: TransactionId, minor: i64) {
        self.db
            .conn()
            .execute(
                "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
                 VALUES ('refund', ?1, ?2, ?3, 'suggested')",
                rusqlite::params![inflow, purchase, minor],
            )
            .unwrap();
    }

    /// An entered (manual) balance at the end of `day`.
    pub fn balance(&mut self, account: AccountId, day: &str, minor: i64) {
        let currency = self.currency(account);
        account::add_balance(
            &mut self.db,
            account,
            &EnteredBalance {
                date: date(day),
                balance: Money::new(minor, currency),
                source: EnteredSource::Manual,
                note: None,
            },
        )
        .unwrap();
    }

    /// A valuation of an `asset` or `liability` account. Core has no API
    /// for valuations yet, so it is written directly.
    pub fn valuation(&mut self, account: AccountId, day: &str, minor: i64) {
        self.db
            .conn()
            .execute(
                "INSERT INTO balance_snapshot (account_id, date, balance_minor, source) \
                 VALUES (?1, ?2, ?3, 'valuation')",
                rusqlite::params![account, day, minor],
            )
            .unwrap();
    }

    /// Tags `transaction` with `name` (created on first use).
    pub fn tag(&mut self, transaction: TransactionId, name: &str, private: bool) {
        let name = TagName::new(name).unwrap();
        let id = match tag::find(&self.db, &name).unwrap() {
            Some(tag) => tag.id,
            None => tag::create(&mut self.db, &name, private).unwrap(),
        };
        tag::add_to_transaction(&mut self.db, transaction, id).unwrap();
    }

    fn transaction_currency(&self, transaction: TransactionId) -> Currency {
        self.db
            .conn()
            .query_row(
                "SELECT currency FROM \"transaction\" WHERE id = ?1",
                [transaction],
                |row| row.get(0),
            )
            .unwrap()
    }
}
