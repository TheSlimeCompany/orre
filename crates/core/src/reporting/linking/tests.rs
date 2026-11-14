//! Tests for the linking read model, on registers read from a database
//! written through core.

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount, date_key};
use crate::db::Database;
use crate::edit;
use crate::id::AccountId;
use crate::link::{LinkTarget, NewLink, ShareLabel, Sharing};
use crate::money::Currency;

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn day(month: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, d).unwrap()
}

struct Fixture {
    db: Database,
    joint: AccountId,
    current: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let mut make = |name| {
            account::create(
                &mut db,
                &NewAccount::new(name, AccountKind::Current, Currency::GBP),
            )
            .unwrap()
        };
        let joint = make("Joint");
        let current = make("Current");
        Fixture {
            db,
            joint,
            current,
            _dir: dir,
        }
    }

    fn row(
        &self,
        account: AccountId,
        date: NaiveDate,
        minor: i64,
        merchant: &str,
        category: Option<&str>,
    ) -> TransactionId {
        self.db
            .conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 merchant, merchant_display, description_raw, description_normalized, origin, \
                 category_id, category_source) \
                 VALUES (?1, ?2, ?3, 'GBP', ?4, ?4, ?4, upper(?4), 'manual', \
                 (SELECT id FROM category WHERE key = ?5), \
                 CASE WHEN ?5 IS NULL THEN NULL ELSE 'user' END)",
                params![account, date_key(date), minor, merchant, category],
            )
            .unwrap();
        TransactionId::new(self.db.conn().last_insert_rowid())
    }

    fn register(&self) -> Register {
        Register::load(&self.db.reader().unwrap(), day(11, 14)).unwrap()
    }

    fn refund(&mut self, inflow: TransactionId, purchase: TransactionId, minor: i64) {
        edit::link(
            &mut self.db,
            &NewLink {
                kind: LinkKind::Refund,
                inflow,
                target: LinkTarget::Purchase(purchase),
                amount: gbp(minor),
            },
        )
        .unwrap();
    }
}

fn shopping(register: &Register) -> i64 {
    register
        .categories()
        .iter()
        .find(|category| category.key.as_str() == "shopping")
        .map(|category| category.id)
        .unwrap()
}

#[test]
fn rows_say_how_they_count() {
    let mut f = Fixture::new();
    let purchase = f.row(f.joint, day(10, 28), -6800, "ASOS.com", Some("shopping"));
    let refund = f.row(f.joint, day(11, 8), 3400, "ASOS.com", None);
    f.refund(refund, purchase, 3400);
    let odeon = f.row(f.current, day(11, 7), -3180, "Odeon", Some("entertainment"));
    edit::set_sharing(
        &mut f.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: ShareLabel::new("Sam").unwrap(),
        },
    )
    .unwrap();
    let uber = f.row(f.current, day(11, 8), -1430, "Uber", Some("transport"));
    edit::set_sharing(&mut f.db, uber, &Sharing::ClaimBack).unwrap();
    let train = f.row(f.current, day(11, 2), -3860, "Trainline", Some("transport"));
    edit::set_sharing(&mut f.db, train, &Sharing::ClaimBack).unwrap();
    let acme = f.row(f.current, day(11, 9), 3860, "ACME LTD EXPENSES", None);
    edit::link(
        &mut f.db,
        &NewLink {
            kind: LinkKind::Reimbursement,
            inflow: acme,
            target: LinkTarget::Purchase(train),
            amount: gbp(3860),
        },
    )
    .unwrap();
    let sam = f.row(f.current, day(11, 10), 1000, "SAM K", None);
    edit::link(
        &mut f.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: sam,
            target: LinkTarget::Person(ShareLabel::new("Sam").unwrap()),
            amount: gbp(1000),
        },
    )
    .unwrap();
    let plain = f.row(f.current, day(11, 9), -420, "Pret", Some("food"));

    let register = f.register();
    let counts = |id| counts_as(&register, register.row(id).unwrap());
    assert_eq!(
        counts(refund),
        Counts::Refund {
            category: Some(shopping(&register))
        }
    );
    assert_eq!(counts(purchase), Counts::Refunded);
    assert_eq!(counts(odeon), Counts::Shared);
    assert_eq!(counts(uber), Counts::ToClaim);
    assert_eq!(counts(train), Counts::Claimed);
    assert_eq!(counts(acme), Counts::RepaysClaims(1));
    assert_eq!(counts(sam), Counts::RepaysPerson("Sam".into()));
    assert_eq!(counts(plain), Counts::Plain);

    let row = register.row(purchase).unwrap();
    assert_eq!(row.share.left, gbp(3400));
    assert_eq!(row.links.len(), 1);
    assert_eq!(register.row(refund).unwrap().share.left, gbp(0));
    assert_eq!(register.people().len(), 1);
    assert_eq!(register.people()[0].owed, gbp(590));
}

#[test]
fn candidates_put_the_same_merchant_first_then_the_newest() {
    let mut f = Fixture::new();
    let older = f.row(f.joint, day(10, 14), -4200, "ASOS.com", Some("shopping"));
    let newer = f.row(f.joint, day(10, 28), -6800, "ASOS.com", Some("shopping"));
    let other = f.row(f.joint, day(11, 1), -999, "Boots", None);
    let refunded = f.row(f.joint, day(9, 19), -1999, "ASOS.com", Some("shopping"));
    let back = f.row(f.joint, day(9, 25), 1999, "ASOS.com", None);
    f.refund(back, refunded, 1999);
    let _later = f.row(f.joint, day(11, 12), -500, "ASOS.com", None);
    let _income = f.row(f.joint, day(11, 1), 500, "ASOS.com", None);
    let refund = f.row(f.joint, day(11, 8), 3400, "ASOS.com", None);

    let register = f.register();
    let from = register.row(refund).unwrap();
    let list = candidates(&register, from, LinkKind::Refund, "");
    let ids: Vec<TransactionId> = list.iter().map(|c| c.transaction).collect();
    assert_eq!(ids, [newer, older, refunded, other]);
    assert_eq!(list[2].blocked, Some(Blocked::AlreadyRefunded));
    assert!(list[0].same_merchant && !list[3].same_merchant);
    // The search matches like the table's.
    let ids: Vec<TransactionId> = candidates(&register, from, LinkKind::Refund, "boo")
        .iter()
        .map(|c| c.transaction)
        .collect();
    assert_eq!(ids, [other]);
    // A reimbursement lists claims only.
    assert!(candidates(&register, from, LinkKind::Reimbursement, "").is_empty());
    assert_eq!(default_amount(from.share.left, list[0].left), gbp(3400));
    assert_eq!(default_amount(gbp(3400), gbp(1200)), gbp(1200));

    // From the purchase: money in on or after it.
    let purchase = register.row(newer).unwrap();
    let ids: Vec<TransactionId> = candidates(&register, purchase, LinkKind::Refund, "")
        .iter()
        .map(|c| c.transaction)
        .collect();
    assert_eq!(ids, [refund, _income]);
    assert_eq!(
        kinds_from(purchase),
        [LinkKind::Refund],
        "a purchase not claimed back takes refunds only"
    );
    assert_eq!(kinds_from(from).len(), 3);
}

#[test]
fn a_suggested_refund_says_why() {
    let f = Fixture::new();
    let purchase = f.row(f.joint, day(10, 28), -6800, "ASOS.com", Some("shopping"));
    let refund = f.row(f.joint, day(11, 8), 3400, "asos.com", None);
    crate::link::suggest_all(f.db.conn()).unwrap();
    let register = f.register();
    let row = register.row(refund).unwrap();
    let why = refund_why(&register, row, &row.links[0]).unwrap();
    assert_eq!(
        why,
        RefundWhy {
            same_merchant: true,
            same_account: true,
            days_earlier: 11,
            exact: false,
        }
    );
    // A suggestion doesn't count yet.
    assert_eq!(register.row(purchase).unwrap().share.left, gbp(6800));
    assert_eq!(counts_as(&register, row), Counts::Plain);
}

#[test]
fn settlements_list_people_who_owe_in_the_currency() {
    let mut f = Fixture::new();
    let odeon = f.row(f.current, day(11, 7), -3180, "Odeon", None);
    let dishoom = f.row(f.current, day(10, 30), -4800, "Dishoom", None);
    for (id, share, name) in [(odeon, -1590, "Sam"), (dishoom, -2400, "Priya")] {
        edit::set_sharing(
            &mut f.db,
            id,
            &Sharing::Shared {
                my_share: gbp(share),
                label: ShareLabel::new(name).unwrap(),
            },
        )
        .unwrap();
    }
    let payment = f.row(f.current, day(11, 9), 3000, "SAM K", None);
    let register = f.register();
    let from = register.row(payment).unwrap();
    let names: Vec<&str> = people(&register, from, "")
        .iter()
        .map(|p| p.label.as_str())
        .collect();
    assert_eq!(names, ["Priya", "Sam"]);
    let names: Vec<&str> = people(&register, from, "SA")
        .iter()
        .map(|p| p.label.as_str())
        .collect();
    assert_eq!(names, ["Sam"]);
    assert!(candidates(&register, from, LinkKind::Settlement, "").is_empty());

    // From the person: money in, newest first.
    let ids: Vec<TransactionId> = payments(&register, Currency::GBP, "")
        .iter()
        .map(|c| c.transaction)
        .collect();
    assert_eq!(ids, [payment]);
    assert!(payments(&register, Currency::EUR, "").is_empty());
}
