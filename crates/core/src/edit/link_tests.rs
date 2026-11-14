//! Tests for the link and share edits (M3.11): each is one undoable change
//! batch, recorded on the money in's `links`.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount, date_key};
use crate::change::{LinkValue, Value};
use crate::id::AccountId;
use crate::link::{LinkKind, LinkStatus, LinkTarget};
use crate::money::{Currency, Money};

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn day(month: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, d).unwrap()
}

fn person(text: &str) -> ShareLabel {
    ShareLabel::new(text).unwrap()
}

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Joint", AccountKind::Current, Currency::GBP),
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

    fn links(&self, inflow: TransactionId) -> Vec<LinkValue> {
        match change::read(self.db.conn(), inflow, Field::Links).unwrap() {
            Value::Links(links) => links,
            _ => unreachable!(),
        }
    }

    fn link_ids(&self, inflow: TransactionId) -> Vec<LinkId> {
        link::links_of(&self.db, inflow)
            .unwrap()
            .into_iter()
            .filter(|link| link.inflow == inflow)
            .map(|link| link.id)
            .collect()
    }

    fn refund(&mut self, inflow: TransactionId, purchase: TransactionId, minor: i64) -> LinkId {
        let (id, batch) = link(
            &mut self.db,
            &NewLink {
                kind: LinkKind::Refund,
                inflow,
                target: LinkTarget::Purchase(purchase),
                amount: gbp(minor),
            },
        )
        .unwrap();
        assert!(batch.is_some());
        id
    }

    fn last_batch(&self) -> ChangeBatchId {
        change::list(&self.db).unwrap()[0].id
    }
}

#[test]
fn linking_is_one_batch_and_undo_unlinks() {
    let mut f = Fixture::new();
    let purchase = f.row(day(10, 28), -6800, "ASOS.com");
    let refund = f.row(day(11, 8), 3400, "ASOS.com");
    f.refund(refund, purchase, 3400);
    assert_eq!(
        f.links(refund),
        [LinkValue {
            target_id: Some(purchase.get()),
            share_label: None,
            kind: LinkKind::Refund,
            amount_minor: 3400,
            status: LinkStatus::Confirmed,
        }]
    );
    let batch = f.last_batch();
    let items = change::items(&f.db, batch).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].field(), Field::Links);
    assert_eq!(link::purchase_left(&f.db, purchase).unwrap(), gbp(3400));

    let undone = change::undo(&mut f.db, batch).unwrap();
    assert_eq!(undone.restored, 1);
    assert!(f.links(refund).is_empty());
    assert_eq!(link::purchase_left(&f.db, purchase).unwrap(), gbp(6800));
}

#[test]
fn unlinking_rejects_and_undo_confirms_the_same_row() {
    let mut f = Fixture::new();
    let purchase = f.row(day(10, 28), -6800, "ASOS.com");
    let refund = f.row(day(11, 8), 3400, "ASOS.com");
    let id = f.refund(refund, purchase, 3400);
    let batch = reject_links(&mut f.db, &[id]).unwrap().unwrap();
    assert_eq!(f.links(refund)[0].status, LinkStatus::Rejected);
    // Unlinking again changes nothing.
    assert_eq!(reject_links(&mut f.db, &[id]).unwrap(), None);

    change::undo(&mut f.db, batch).unwrap();
    assert_eq!(f.links(refund)[0].status, LinkStatus::Confirmed);
    // Undo kept the row, so the link keeps its ID.
    assert_eq!(f.link_ids(refund), [id]);
}

#[test]
fn not_a_refund_is_undone_back_to_the_suggestion() {
    let mut f = Fixture::new();
    let purchase = f.row(day(10, 28), -6800, "ASOS.com");
    let refund = f.row(day(11, 8), 3400, "ASOS.com");
    assert_eq!(link::suggest_all(f.db.conn()).unwrap(), 1);
    let suggestion = f.link_ids(refund)[0];
    let batch = reject_links(&mut f.db, &[suggestion]).unwrap().unwrap();
    assert_eq!(f.links(refund)[0].status, LinkStatus::Rejected);
    // A rejected pair is never suggested again.
    assert_eq!(link::suggest_all(f.db.conn()).unwrap(), 0);

    change::undo(&mut f.db, batch).unwrap();
    let links = f.links(refund);
    assert_eq!(links[0].status, LinkStatus::Suggested);
    assert_eq!(links[0].target_id, Some(purchase.get()));
}

#[test]
fn confirming_a_set_of_claims_is_one_batch() {
    let mut f = Fixture::new();
    let trainline = f.row(day(11, 2), -3860, "Trainline");
    let hotel = f.row(day(11, 3), -11200, "Premier Inn");
    for claim in [trainline, hotel] {
        set_sharing(&mut f.db, claim, &Sharing::ClaimBack)
            .unwrap()
            .unwrap();
    }
    let payment = f.row(day(11, 9), 15060, "ACME LTD EXPENSES");
    assert_eq!(link::suggest_all(f.db.conn()).unwrap(), 2);
    let ids = f.link_ids(payment);
    let batch = confirm_links(&mut f.db, &ids).unwrap().unwrap();
    assert_eq!(change::items(&f.db, batch).unwrap().len(), 1);
    assert!(link::outstanding_claims(&f.db).unwrap().is_empty());

    change::undo(&mut f.db, batch).unwrap();
    assert_eq!(link::outstanding_claims(&f.db).unwrap().len(), 2);
    assert!(
        f.links(payment)
            .iter()
            .all(|link| link.status == LinkStatus::Suggested)
    );
}

#[test]
fn a_dropped_suggestion_on_another_payment_comes_back_with_undo() {
    let mut f = Fixture::new();
    let purchase = f.row(day(10, 28), -3400, "ASOS.com");
    let first = f.row(day(11, 8), 3400, "ASOS.com");
    assert_eq!(link::suggest_all(f.db.conn()).unwrap(), 1);
    // Another payment takes the whole purchase: the suggestion no longer fits.
    let other = f.row(day(11, 9), 3400, "Refund");
    f.refund(other, purchase, 3400);
    assert!(f.links(first).is_empty());
    let batch = f.last_batch();
    assert_eq!(change::items(&f.db, batch).unwrap().len(), 2);

    change::undo(&mut f.db, batch).unwrap();
    assert_eq!(f.links(first)[0].status, LinkStatus::Suggested);
    assert!(f.links(other).is_empty());
}

#[test]
fn undo_leaves_links_that_no_longer_fit() {
    let mut f = Fixture::new();
    let purchase = f.row(day(10, 28), -3400, "ASOS.com");
    let refund = f.row(day(11, 8), 3400, "ASOS.com");
    let id = f.refund(refund, purchase, 3400);
    let unlink = reject_links(&mut f.db, &[id]).unwrap().unwrap();
    // The purchase is refunded by another payment since.
    let other = f.row(day(11, 9), 3400, "Refund");
    f.refund(other, purchase, 3400);

    let undone = change::undo(&mut f.db, unlink).unwrap();
    assert_eq!((undone.restored, undone.changed_since), (0, 1));
    assert_eq!(f.links(refund)[0].status, LinkStatus::Rejected);
}

#[test]
fn sharing_records_share_and_claim_and_undoes() {
    let mut f = Fixture::new();
    let odeon = f.row(day(11, 7), -3180, "Odeon");
    let batch = set_sharing(
        &mut f.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: person("Sam"),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        link::owed_to(&f.db, &person("Sam"), Currency::GBP).unwrap(),
        gbp(1590)
    );
    let fields: Vec<Field> = change::items(&f.db, batch)
        .unwrap()
        .iter()
        .map(|item| item.field())
        .collect();
    assert_eq!(fields, [Field::Share]);

    let claim = set_sharing(&mut f.db, odeon, &Sharing::ClaimBack)
        .unwrap()
        .unwrap();
    let fields: Vec<Field> = change::items(&f.db, claim)
        .unwrap()
        .iter()
        .map(|item| item.field())
        .collect();
    assert_eq!(fields, [Field::Share, Field::Reimbursable]);
    assert_eq!(
        set_sharing(&mut f.db, odeon, &Sharing::ClaimBack).unwrap(),
        None
    );

    change::undo(&mut f.db, claim).unwrap();
    change::undo(&mut f.db, batch).unwrap();
    assert!(link::owed_by_label(&f.db).unwrap().is_empty());
    // Money in can't be shared: core's checks refuse it.
    let income = f.row(day(11, 7), 1000, "Pay");
    assert!(matches!(
        set_sharing(&mut f.db, income, &Sharing::ClaimBack),
        Err(Error::Link(link::Error::NotMoneyOut(_)))
    ));
}

#[test]
fn renaming_a_person_covers_shares_and_settlements_and_undoes() {
    let mut f = Fixture::new();
    let odeon = f.row(day(11, 7), -3180, "Odeon");
    set_sharing(
        &mut f.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: person("Sam"),
        },
    )
    .unwrap();
    let payment = f.row(day(11, 9), 1000, "SAM K");
    link(
        &mut f.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: payment,
            target: LinkTarget::Person(person("Sam")),
            amount: gbp(1000),
        },
    )
    .unwrap();
    let batch = rename_person(&mut f.db, &person("Sam"), &person("Sam K"))
        .unwrap()
        .unwrap();
    let fields: Vec<Field> = change::items(&f.db, batch)
        .unwrap()
        .iter()
        .map(|item| item.field())
        .collect();
    assert_eq!(fields, [Field::Share, Field::Links]);
    let people = link::owed_by_label(&f.db).unwrap();
    assert_eq!(people.len(), 1);
    assert_eq!(
        (people[0].label.as_str(), people[0].owed),
        ("Sam K", gbp(590))
    );

    change::undo(&mut f.db, batch).unwrap();
    let people = link::owed_by_label(&f.db).unwrap();
    assert_eq!(
        (people[0].label.as_str(), people[0].owed),
        ("Sam", gbp(590))
    );
    assert_eq!(f.links(payment)[0].share_label.as_deref(), Some("Sam"));
}
