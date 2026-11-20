//! Tests for Owed to you's read model.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount, date_key};
use crate::edit;
use crate::link::{LinkKind, LinkTarget, NewLink, Sharing};

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn day(month: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, d).unwrap()
}

fn sam() -> ShareLabel {
    ShareLabel::new("Sam").unwrap()
}

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("orre.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn row(&self, date: NaiveDate, minor: i64, merchant: &str) -> TransactionId {
        self.db
            .conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 merchant, merchant_display, description_raw, description_normalized, origin) \
                 VALUES (?1, ?2, ?3, 'GBP', ?4, ?4, ?4, upper(?4), 'manual')",
                params![self.account, date_key(date), minor, merchant],
            )
            .unwrap();
        TransactionId::new(self.db.conn().last_insert_rowid())
    }

    fn claim(&mut self, date: NaiveDate, minor: i64, merchant: &str) -> TransactionId {
        let id = self.row(date, minor, merchant);
        edit::set_sharing(&mut self.db, id, &Sharing::ClaimBack).unwrap();
        id
    }

    fn shared(&mut self, date: NaiveDate, minor: i64, mine: i64, merchant: &str) -> TransactionId {
        let id = self.row(date, minor, merchant);
        edit::set_sharing(
            &mut self.db,
            id,
            &Sharing::Shared {
                my_share: gbp(mine),
                label: sam(),
            },
        )
        .unwrap();
        id
    }
}

#[test]
fn claims_are_oldest_first_with_their_suggested_payment() {
    let mut f = Fixture::new();
    let train = f.claim(day(11, 2), -3860, "Trainline");
    let hotel = f.claim(day(11, 3), -11200, "Premier Inn Leeds City Centre");
    let uber = f.claim(day(11, 8), -1430, "Uber");
    let acme = f.row(day(11, 9), 15060, "ACME LTD EXPENSES");
    crate::link::suggest_all(f.db.conn()).unwrap();

    let queue = queue(&f.db).unwrap();
    let ids: Vec<TransactionId> = queue.claims.iter().map(|c| c.transaction).collect();
    assert_eq!(ids, [train, hotel, uber]);
    assert_eq!(queue.claims[0].account_name, "Current account");
    assert_eq!(queue.claims[1].merchant, "Premier Inn Leeds City Centre");
    let matched: Vec<bool> = queue.claims.iter().map(|c| c.matched).collect();
    assert_eq!(matched, [true, true, false]);
    assert_eq!(queue.matches.len(), 1);
    let found = &queue.matches[0];
    assert_eq!(
        (found.inflow, found.amount, found.claims.clone()),
        (acme, gbp(15060), vec![train, hotel])
    );
    assert_eq!(queue.totals, [gbp(3860 + 11200 + 1430)]);
    assert_eq!(queue.open(), 3);

    // Linking it clears both.
    edit::confirm_links(&mut f.db, &found.links).unwrap();
    let queue = super::queue(&f.db).unwrap();
    assert_eq!(queue.claims.len(), 1);
    assert!(queue.matches.is_empty());
}

#[test]
fn a_persons_costs_are_what_is_still_owed() {
    let mut f = Fixture::new();
    let early = f.shared(day(10, 1), -6000, -3000, "Pizza");
    let dishoom = f.shared(day(10, 30), -4800, -2400, "Dishoom Shoreditch");
    let odeon = f.shared(day(11, 7), -3180, -1590, "Odeon Luxe Leicester Square");
    let payment = f.row(day(10, 12), 3000, "SAM K");
    edit::link(
        &mut f.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: payment,
            target: LinkTarget::Person(sam()),
            amount: gbp(3000),
        },
    )
    .unwrap();

    let queue = queue(&f.db).unwrap();
    assert_eq!(queue.people.len(), 1);
    let person = &queue.people[0];
    assert_eq!(person.owed, gbp(3990));
    let costs: Vec<(TransactionId, Money)> = person
        .costs
        .iter()
        .map(|c| (c.transaction, c.part))
        .collect();
    assert_eq!(costs, [(dishoom, gbp(2400)), (odeon, gbp(1590))]);
    assert!(!costs.iter().any(|(id, _)| *id == early));
    assert_eq!(person.last_paid(), Some(day(10, 12)));
    assert_eq!(person.payments[0].merchant, "SAM K");

    // A part payment leaves the rest of the oldest cost.
    let more = f.row(day(11, 10), 1000, "SAM K");
    edit::link(
        &mut f.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: more,
            target: LinkTarget::Person(sam()),
            amount: gbp(1000),
        },
    )
    .unwrap();
    let queue = super::queue(&f.db).unwrap();
    let person = &queue.people[0];
    let costs: Vec<(TransactionId, Money)> = person
        .costs
        .iter()
        .map(|c| (c.transaction, c.part))
        .collect();
    assert_eq!(costs, [(dishoom, gbp(1400)), (odeon, gbp(1590))]);
    assert_eq!(person.payments.len(), 2);
    assert_eq!(person.last_paid(), Some(day(11, 10)));
}

#[test]
fn someone_paid_up_isnt_listed() {
    let mut f = Fixture::new();
    f.shared(day(11, 7), -3180, -1590, "Odeon");
    let payment = f.row(day(11, 9), 1590, "SAM K");
    edit::link(
        &mut f.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: payment,
            target: LinkTarget::Person(sam()),
            amount: gbp(1590),
        },
    )
    .unwrap();
    let queue = queue(&f.db).unwrap();
    assert!(queue.people.is_empty());
    assert_eq!(queue, OwedQueue::default());
}
