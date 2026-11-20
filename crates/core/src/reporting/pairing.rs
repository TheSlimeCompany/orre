//! `pairing`: the transfer pair a row is in, as the Transactions inspector's
//! Transfer section shows it (ROADMAP M3.8; `ARCHITECTURE.md` → "Decision
//! record: transfer-pair confirmation (built)").
//!
//! A pure function over the [`Register`], like [`super::linking`]: the
//! inspector asks nothing of the database while it draws. Whether each
//! action is allowed comes from [`transfer::availability`], the same checks
//! the action runs when it writes, so the two can't disagree except when
//! the database changed since the register was read, and then the write
//! refuses.

use crate::category::CategoryKind;
use crate::id::TransactionId;
use crate::link::LinkStatus;
use crate::link::MAX_LINKED_PURCHASE_MINOR;
use crate::manual::Origin;
use crate::transfer::{self, Action, Availability, Blocked, PairRow, TransferStatus};

use super::register::{Register, RegisterRow, RowCategory, RowSource};
use crate::id::TransferGroupId;

/// A row's pair: the other side and what the user can do with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair<'a> {
    pub group: TransferGroupId,
    /// `Suggested` or `Confirmed`.
    pub status: TransferStatus,
    /// The group's other row, when the pair has exactly two.
    pub partner: Option<&'a RegisterRow>,
    pub confirm: Availability,
    pub reject: Availability,
    pub unpair: Availability,
    /// Whether the pair can be linked as a refund instead
    /// ([`inbox::link_as_refund`](crate::inbox::link_as_refund)).
    pub refund: Result<(), RefundBlocked>,
}

/// Why a suggested pair can't be linked as a refund now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundBlocked {
    /// The pair can't be rejected, which linking as a refund does first.
    NotRejectable(Blocked),
    /// The pair isn't one money in and one money out.
    NotInAndOut,
    /// The two are in different currencies.
    Currency,
    /// The money in is split, shared or a duplicate, so it takes no link.
    InflowTakesNoLink(TransactionId),
    /// The money out is a duplicate or too large for a link.
    PurchaseTakesNoLink(TransactionId),
    /// Either side has less left to link than the refund would take.
    NothingLeft,
    /// The money out isn't spending (no category of kind `spending`, its
    /// own or a split line's): most likely a transfer out, so the card
    /// doesn't offer a refund. Not a link rule: [`inbox::link_as_refund`]
    /// (crate::inbox::link_as_refund) doesn't check it.
    PurchaseNotSpending(TransactionId),
}

impl Pair<'_> {
    /// Whether `action` is allowed now.
    pub fn allows(&self, action: Action) -> Availability {
        match action {
            Action::Confirm => self.confirm,
            Action::Reject => self.reject,
            Action::Unpair => self.unpair,
        }
    }

    /// Whether the money moves out of `row`'s account: the arrow reads
    /// from this row's account to the partner's.
    pub fn outgoing(row: &RegisterRow) -> bool {
        row.amount.is_negative()
    }
}

/// The suggested or confirmed pair `row` is in, or `None`.
pub fn pair<'a>(register: &'a Register, row: &RegisterRow) -> Option<Pair<'a>> {
    let found = row.pair?;
    let mut members: Vec<&RegisterRow> = register
        .rows()
        .iter()
        .filter(|other| other.pair.is_some_and(|pair| pair.group == found.group))
        .collect();
    members.sort_by_key(|member| member.id);
    let checks: Vec<PairRow> = members.iter().map(|member| pair_row(member)).collect();
    let allowed = |action| transfer::availability(found.status, &checks, action);
    let partner = match members.as_slice() {
        [a, b] if a.id == row.id => Some(*b),
        [a, b] if b.id == row.id => Some(*a),
        _ => None,
    };
    let refund = match partner {
        Some(partner) => refund_available(register, row, partner, allowed(Action::Reject)),
        None => Err(RefundBlocked::NotRejectable(Blocked::NotTwoSided)),
    };
    Some(Pair {
        group: found.group,
        status: found.status,
        partner,
        confirm: allowed(Action::Confirm),
        reject: allowed(Action::Reject),
        unpair: allowed(Action::Unpair),
        refund,
    })
}

/// Whether the pair of `row` and `partner` can be linked as a refund: the
/// pair can be rejected (`reject`), and the checks
/// [`link::create`](crate::link::create) runs on a refund of the money in
/// for its whole amount hold, as does the link dialog's rule that a
/// duplicate takes no link. On top of those, the money out must be
/// spending ([`RefundBlocked::PurchaseNotSpending`], M3.P11): detection
/// pairs every matching in and out, and a plain move between the user's
/// accounts shouldn't ask whether it's a refund.
fn refund_available(
    register: &Register,
    row: &RegisterRow,
    partner: &RegisterRow,
    reject: Availability,
) -> Result<(), RefundBlocked> {
    reject.map_err(RefundBlocked::NotRejectable)?;
    let (inflow, purchase) = match (row.amount.is_positive(), partner.amount.is_positive()) {
        (true, false) => (row, partner),
        (false, true) => (partner, row),
        _ => return Err(RefundBlocked::NotInAndOut),
    };
    if !purchase.amount.is_negative() {
        return Err(RefundBlocked::NotInAndOut);
    }
    if inflow.amount.currency() != purchase.amount.currency() {
        return Err(RefundBlocked::Currency);
    }
    let duplicate = |row: &RegisterRow| {
        row.excluded
            .is_some_and(crate::exclusion::ExclusionReason::leaves_balance)
    };
    if matches!(inflow.category, RowCategory::Split(_))
        || inflow.share.my_share.is_some()
        || inflow.share.label.is_some()
        || duplicate(inflow)
    {
        return Err(RefundBlocked::InflowTakesNoLink(inflow.id));
    }
    if purchase.amount.minor() < -MAX_LINKED_PURCHASE_MINOR || duplicate(purchase) {
        return Err(RefundBlocked::PurchaseTakesNoLink(purchase.id));
    }
    let spending = register
        .row_categories(purchase)
        .iter()
        .any(|category| category.kind == CategoryKind::Spending);
    if !spending {
        return Err(RefundBlocked::PurchaseNotSpending(purchase.id));
    }
    let amount = inflow.amount.minor();
    if inflow.share.left.minor() < amount || purchase.share.left.minor() < amount {
        return Err(RefundBlocked::NothingLeft);
    }
    Ok(())
}

/// What [`transfer::availability`] needs of a register row.
fn pair_row(row: &RegisterRow) -> PairRow {
    PairRow {
        id: row.id,
        excluded: row.excluded.is_some(),
        split: matches!(row.category, RowCategory::Split(_)),
        mirrored: matches!(
            row.source,
            RowSource::Import {
                origin: Origin::Mirrored,
                ..
            }
        ),
        linked: row
            .links
            .iter()
            .any(|link| link.status == LinkStatus::Confirmed),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use tempfile::TempDir;

    use super::*;
    use crate::db::Database;
    use crate::exclusion::{self, ExclusionReason};
    use crate::id::TransactionId;
    use crate::transfer::Blocked;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    /// Two accounts and a suggested £50 pair between them, as detection
    /// leaves it, plus an unpaired row.
    struct Fixture {
        db: Database,
        _dir: TempDir,
        out: TransactionId,
        inn: TransactionId,
        lone: TransactionId,
        group: TransferGroupId,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("orre.db")).unwrap();
        let conn = db.conn();
        conn.execute_batch(
            "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP'), \
                                                             ('Savings', 'savings', 'GBP');
             INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) VALUES \
                 (1, '2026-11-02', -5000, 'GBP', 'To savings', 'to savings', 'manual'), \
                 (2, '2026-11-02', 5000, 'GBP', 'From current', 'from current', 'manual'), \
                 (1, '2026-11-03', -900, 'GBP', 'Lunch', 'lunch', 'manual');
             INSERT INTO transfer_group (status) VALUES ('suggested');
             UPDATE \"transaction\" SET transfer_group_id = 1 WHERE id IN (1, 2);",
        )
        .unwrap();
        Fixture {
            db,
            _dir: dir,
            out: TransactionId::new(1),
            inn: TransactionId::new(2),
            lone: TransactionId::new(3),
            group: TransferGroupId::new(1),
        }
    }

    fn register(db: &Database) -> Register {
        Register::load(&db.reader().unwrap(), day(14)).unwrap()
    }

    /// Gives row `id` a spending category, as a purchase has.
    fn spend(db: &Database, id: TransactionId) {
        db.conn()
            .execute(
                "UPDATE \"transaction\" SET category_source = 'user', category_id = \
                 (SELECT id FROM category WHERE kind = 'spending' ORDER BY id LIMIT 1) \
                 WHERE id = ?1",
                [id],
            )
            .unwrap();
    }

    #[test]
    fn a_suggested_pair_names_the_other_side_and_can_be_decided() {
        let f = fixture();
        let register = register(&f.db);
        let out = register.row(f.out).unwrap();
        let pair = pair(&register, out).unwrap();
        assert_eq!(pair.group, f.group);
        assert_eq!(pair.status, TransferStatus::Suggested);
        assert_eq!(pair.partner.map(|p| p.id), Some(f.inn));
        assert_eq!(pair.confirm, Ok(()));
        assert_eq!(pair.reject, Ok(()));
        assert_eq!(
            pair.unpair,
            Err(Blocked::WrongStatus {
                found: TransferStatus::Suggested
            })
        );
        assert!(Pair::outgoing(out));
        assert!(super::pair(&register, register.row(f.lone).unwrap()).is_none());
    }

    #[test]
    fn an_excluded_side_blocks_confirming_only() {
        let mut f = fixture();
        exclusion::exclude(&mut f.db, f.inn, ExclusionReason::Other, Some("Test")).unwrap();
        let register = register(&f.db);
        let pair = pair(&register, register.row(f.out).unwrap()).unwrap();
        assert_eq!(pair.confirm, Err(Blocked::Excluded(f.inn)));
        assert_eq!(pair.reject, Ok(()));
    }

    #[test]
    fn a_confirmed_pair_can_be_unpaired_and_a_rejected_one_is_gone() {
        let mut f = fixture();
        let decision = transfer::confirm(&mut f.db, f.group).unwrap();
        let read = register(&f.db);
        let pair = pair(&read, read.row(f.inn).unwrap()).unwrap();
        assert_eq!(pair.status, TransferStatus::Confirmed);
        assert_eq!(pair.partner.map(|p| p.id), Some(f.out));
        assert_eq!(pair.unpair, Ok(()));

        transfer::undo(&mut f.db, &decision).unwrap();
        transfer::reject(&mut f.db, f.group).unwrap();
        let read = register(&f.db);
        assert!(super::pair(&read, read.row(f.inn).unwrap()).is_none());
    }

    #[test]
    fn a_suggested_pair_of_money_in_and_out_can_be_a_refund() {
        let mut f = fixture();
        // A plain move between accounts: the money out isn't spending.
        let read = register(&f.db);
        for id in [f.out, f.inn] {
            let pair = pair(&read, read.row(id).unwrap()).unwrap();
            assert_eq!(pair.refund, Err(RefundBlocked::PurchaseNotSpending(f.out)));
        }
        // A purchase: it can be a refund.
        spend(&f.db, f.out);
        let read = register(&f.db);
        for id in [f.out, f.inn] {
            let pair = pair(&read, read.row(id).unwrap()).unwrap();
            assert_eq!(pair.refund, Ok(()));
        }
        // What it offers, the write does.
        crate::inbox::link_as_refund(&mut f.db, f.group).unwrap();
    }

    #[test]
    fn a_pair_that_cannot_be_linked_offers_no_refund() {
        let f = fixture();
        // Two outflows: not money in and out.
        f.db.conn()
            .execute(
                "UPDATE \"transaction\" SET amount_minor = -5000 WHERE id = 2",
                [],
            )
            .unwrap();
        let read = register(&f.db);
        let pair = pair(&read, read.row(f.out).unwrap()).unwrap();
        assert_eq!(pair.refund, Err(RefundBlocked::NotInAndOut));

        // A pair that can't be rejected can't become a refund.
        let f = fixture();
        f.db.conn()
            .execute("UPDATE transfer_group SET status = 'confirmed'", [])
            .unwrap();
        let read = register(&f.db);
        let pair = super::pair(&read, read.row(f.out).unwrap()).unwrap();
        assert!(matches!(
            pair.refund,
            Err(RefundBlocked::NotRejectable(Blocked::WrongStatus { .. }))
        ));

        // The purchase has less left than the refund would take.
        let f = fixture();
        spend(&f.db, f.out);
        f.db.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (2, '2026-11-03', 100, 'GBP', 'Other', 'other', 'manual')",
                [],
            )
            .unwrap();
        f.db.conn()
            .execute(
                "INSERT INTO transaction_link (kind, inflow_id, target_id, amount_minor, status) \
                 VALUES ('refund', 4, 1, 100, 'confirmed')",
                [],
            )
            .unwrap();
        let read = register(&f.db);
        let pair = super::pair(&read, read.row(f.inn).unwrap()).unwrap();
        assert_eq!(pair.refund, Err(RefundBlocked::NothingLeft));
    }

    /// The register's answer and the database's ([`transfer::pair_of`])
    /// agree.
    #[test]
    fn agrees_with_pair_of() {
        let mut f = fixture();
        exclusion::exclude(&mut f.db, f.out, ExclusionReason::BankError, None).unwrap();
        let read = register(&f.db);
        let ours = pair(&read, read.row(f.out).unwrap()).unwrap();
        let theirs = transfer::pair_of(&f.db, f.out).unwrap().unwrap();
        assert_eq!(ours.confirm, theirs.confirm);
        assert_eq!(ours.reject, theirs.reject);
        assert_eq!(ours.unpair, theirs.unpair);
    }
}
