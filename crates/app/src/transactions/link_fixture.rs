//! Sample data for the links snapshots and tests: D6.5's fixture (Paper
//! `9WS-0`, `BAT-0`, `CCF-0`, `EB0-0`), written into a database through
//! core's own API (`edit`, `link::suggest_all`), so what the screens show
//! comes from the real read models. Every merchant, amount and person is
//! invented.
//!
//! - **Joint account**: ASOS.com −£19.99 on 19 Sep, refunded on 25 Sep
//!   (linked); −£42.00 on 14 Oct; −£68.00 on 28 Oct; and a +£34.00 refund
//!   on 8 Nov, which Coffer suggests for the 28 Oct purchase.
//! - **Current account**: shared costs with Sam (Pizza Pilgrims −£60.00 on
//!   5 Oct, half his, settled by SAM K +£30.00 on 12 Oct; Dishoom
//!   Shoreditch −£48.00 on 30 Oct and Odeon Luxe Leicester Square −£31.80
//!   on 7 Nov, half each) and with Priya (Honest Burgers −£41.86 on 8 Nov,
//!   £20.93 hers); claims (Trainline −£38.60 on 2 Nov, Premier Inn Leeds
//!   City Centre −£112.00 on 3 Nov, Uber −£14.30 on 8 Nov) and ACME LTD
//!   EXPENSES +£150.60 on 9 Nov, which Coffer matches to the first two.
//!
//! Owed to you reads £225.73: 3 expense claims and 2 people.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::edit;
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::link::{self, LinkKind, LinkTarget, NewLink, ShareLabel, Sharing};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};

fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).expect("a fixture date")
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn person(name: &str) -> ShareLabel {
    ShareLabel::new(name).expect("a fixture person")
}

fn row(
    db: &mut Database,
    account: AccountId,
    (month, day): (u32, u32),
    minor: i64,
    merchant: &str,
    category: Option<&str>,
) -> TransactionId {
    let mut entry = ManualTransaction::new(account, date(month, day), gbp(minor), merchant);
    entry.merchant = Some(merchant.to_owned());
    entry.category = category.map(|key| CategoryKey::new(key).expect("a fixture category"));
    manual::create(db, &entry).expect("a fixture row")
}

fn share(db: &mut Database, id: TransactionId, mine: i64, name: &str) {
    edit::set_sharing(
        db,
        id,
        &Sharing::Shared {
            my_share: gbp(mine),
            label: person(name),
        },
    )
    .expect("a fixture share");
}

fn claim(db: &mut Database, id: TransactionId) {
    edit::set_sharing(db, id, &Sharing::ClaimBack).expect("a fixture claim");
}

fn link_to(
    db: &mut Database,
    kind: LinkKind,
    inflow: TransactionId,
    target: LinkTarget,
    minor: i64,
) {
    edit::link(
        db,
        &NewLink {
            kind,
            inflow,
            target,
            amount: gbp(minor),
        },
    )
    .expect("a fixture link");
}

/// Which variant of the fixture to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// As the artboards: the refund and the claims suggested.
    Suggested,
    /// The states board's "claim back, partly repaid": ACME paid £20.00 of
    /// Trainline on 9 Nov instead (Paper `FJ1-0`).
    PartlyRepaid,
}

/// The rows the snapshots select.
pub struct Seeded {
    pub joint: AccountId,
    pub current: AccountId,
    pub asos_purchase: TransactionId,
    pub asos_refund: TransactionId,
    pub odeon: TransactionId,
    pub trainline: TransactionId,
    pub acme: TransactionId,
}

/// Writes D6.5's sample accounts and transactions into `db`, which should
/// be empty.
pub fn seed(db: &mut Database, variant: Variant) -> Seeded {
    let create = |db: &mut Database, name: &str| {
        account::create(
            db,
            &NewAccount::new(name, AccountKind::Current, Currency::GBP),
        )
        .expect("a fixture account")
    };
    let joint = create(db, "Joint account");
    let current = create(db, "Current account");

    // ASOS.com on the joint account.
    let refunded = row(db, joint, (9, 19), -1_999, "ASOS.com", Some("shopping"));
    let back = row(db, joint, (9, 25), 1_999, "ASOS.com", None);
    link_to(
        db,
        LinkKind::Refund,
        back,
        LinkTarget::Purchase(refunded),
        1_999,
    );
    row(db, joint, (10, 14), -4_200, "ASOS.com", Some("shopping"));
    let asos_purchase = row(db, joint, (10, 28), -6_800, "ASOS.com", Some("shopping"));
    let asos_refund = row(db, joint, (11, 8), 3_400, "ASOS.com", None);

    // Shared costs on the current account.
    let pizza = row(
        db,
        current,
        (10, 5),
        -6_000,
        "Pizza Pilgrims",
        Some("food.eating_out"),
    );
    share(db, pizza, -3_000, "Sam");
    let sam = row(db, current, (10, 12), 3_000, "SAM K", None);
    link_to(
        db,
        LinkKind::Settlement,
        sam,
        LinkTarget::Person(person("Sam")),
        3_000,
    );
    let dishoom = row(
        db,
        current,
        (10, 30),
        -4_800,
        "Dishoom Shoreditch",
        Some("food.eating_out"),
    );
    share(db, dishoom, -2_400, "Sam");
    let odeon = row(
        db,
        current,
        (11, 7),
        -3_180,
        "Odeon Luxe Leicester Square",
        Some("entertainment"),
    );
    share(db, odeon, -1_590, "Sam");
    let burgers = row(
        db,
        current,
        (11, 8),
        -4_186,
        "Honest Burgers",
        Some("food.eating_out"),
    );
    share(db, burgers, -2_093, "Priya");

    // Claims.
    let trainline = row(db, current, (11, 2), -3_860, "Trainline", Some("transport"));
    let hotel = row(
        db,
        current,
        (11, 3),
        -11_200,
        "Premier Inn Leeds City Centre",
        Some("travel"),
    );
    let uber = row(db, current, (11, 8), -1_430, "Uber", Some("transport"));
    for id in [trainline, hotel, uber] {
        claim(db, id);
    }
    let acme = match variant {
        Variant::Suggested => row(db, current, (11, 9), 15_060, "ACME LTD EXPENSES", None),
        Variant::PartlyRepaid => {
            let acme = row(db, current, (11, 9), 2_000, "ACME LTD EXPENSES", None);
            link_to(
                db,
                LinkKind::Reimbursement,
                acme,
                LinkTarget::Purchase(trainline),
                2_000,
            );
            acme
        }
    };

    link::suggest(db).expect("the fixture's suggestions");
    Seeded {
        joint,
        current,
        asos_purchase,
        asos_refund,
        odeon,
        trainline,
        acme,
    }
}

#[cfg(test)]
mod tests {
    use coffer_core::link::owed;
    use coffer_core::reporting::linking::{self, Counts};
    use coffer_core::reporting::register::Register;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn the_fixture_reads_as_the_artboards() {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let seeded = seed(&mut db, Variant::Suggested);
        let queue = owed::queue(&db).unwrap();
        assert_eq!(queue.totals, [gbp(22_573)]);
        assert_eq!((queue.claims.len(), queue.people.len()), (3, 2));
        assert_eq!(queue.matches.len(), 1);
        assert_eq!(queue.matches[0].claims.len(), 2);
        let sam = queue
            .people
            .iter()
            .find(|p| p.label.as_str() == "Sam")
            .unwrap();
        assert_eq!(sam.owed, gbp(3_990));
        assert_eq!(sam.costs.len(), 2);
        assert_eq!(sam.last_paid(), Some(date(10, 12)));

        let register = Register::load(&db.reader().unwrap(), date(11, 14)).unwrap();
        let refund = register.row(seeded.asos_refund).unwrap();
        assert_eq!(refund.links.len(), 1, "the suggestion");
        assert_eq!(
            linking::counts_as(&register, register.row(seeded.odeon).unwrap()),
            Counts::Shared
        );

        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let seeded = seed(&mut db, Variant::PartlyRepaid);
        let register = Register::load(&db.reader().unwrap(), date(11, 14)).unwrap();
        let trainline = register.row(seeded.trainline).unwrap();
        assert_eq!(trainline.share.left, gbp(1_860));
        assert_eq!(linking::counts_as(&register, trainline), Counts::ToClaim);
    }
}
