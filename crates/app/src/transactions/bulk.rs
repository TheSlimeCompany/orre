//! The bulk-action bar's "Mark as transfer" and "Create rule" (`DESIGN.md`
//! → "Bulk actions": "3 selected · Categorise · Mark as transfer · Create
//! rule · ✕"; nothing more is drawn, so both are composed from what the
//! inspector already does).
//!
//! - **Mark as transfer** is enabled while exactly two rows are selected
//!   that core's [`transfer::markable`] accepts: different accounts, one
//!   currency, opposite amounts, neither in a pair, excluded, split or
//!   linked. Clicking records them as a confirmed transfer and settles an
//!   agent's pending suggestion for the pair ([`inbox::mark_transfer`],
//!   over [`transfer::mark`]); the undo bar offers to reverse it
//!   ([`transfer::unmark`]), as the inspector's transfer decisions do.
//! - **Create rule** builds core's [`rule::rule_from`] for every selected
//!   row. When they all give the same rule, Rules opens on its unsaved
//!   preview, as "Create rule from this" does for one row; otherwise the
//!   error is said under the selection's Category select, as Categorise's
//!   is.

use gpui_kit::Context;
use orre_core::id::TransactionId;
use orre_core::inbox::{self, TransferMarked};
use orre_core::link::LinkStatus;
use orre_core::reporting::register::{RegisterRow, RowCategory};
use orre_core::rule::{self, RuleDefinition, RuleOrigin};
use orre_core::transfer::{self, MarkSide, Marked};

use crate::accounts::store::StoreError;

use super::write::{Offer, Policy, Said};
use super::{TransactionsEvent, TransactionsView, UndoAction};

/// When some selected row gives no rule.
pub const NO_RULE: &str =
    "Orre can’t find a name and a category in each of these rows to build a rule on.";
/// When the selected rows give different rules.
pub const DIFFERENT_RULES: &str =
    "These rows don’t share a name and a category to build a rule on.";

/// What [`transfer::markable`] needs of a register row.
pub fn mark_side(row: &RegisterRow) -> MarkSide {
    MarkSide {
        id: row.id,
        account: row.account,
        amount: row.amount,
        group: row.pair.map(|pair| pair.status),
        excluded: row.excluded.is_some(),
        split: matches!(row.category, RowCategory::Split(_)),
        linked: row
            .links
            .iter()
            .any(|link| link.status == LinkStatus::Confirmed),
    }
}

/// The two rows "Mark as transfer" pairs: `Some` only for exactly two rows
/// that core accepts as one transfer.
pub fn markable_pair(rows: &[&RegisterRow]) -> Option<[TransactionId; 2]> {
    match rows {
        [a, b] => transfer::markable(&mark_side(a), &mark_side(b))
            .ok()
            .map(|()| [a.id, b.id]),
        _ => None,
    }
}

/// The one rule every row gives, or why there isn't one: [`NO_RULE`] when
/// any row gives none (or there are no rows), else [`DIFFERENT_RULES`]
/// when two differ.
pub fn shared_rule(
    rules: impl IntoIterator<Item = Option<RuleDefinition>>,
) -> Result<RuleDefinition, &'static str> {
    let mut shared: Option<RuleDefinition> = None;
    let mut differ = false;
    for rule in rules {
        let rule = rule.ok_or(NO_RULE)?;
        match &shared {
            None => shared = Some(rule),
            Some(first) => differ |= *first != rule,
        }
    }
    match shared {
        None => Err(NO_RULE),
        Some(_) if differ => Err(DIFFERENT_RULES),
        Some(rule) => Ok(rule),
    }
}

/// The undo bar's message after marking: "Marked as transfer: Current
/// account → Savings", money out first.
pub fn marked_message(from: &str, to: &str) -> String {
    format!("Marked as transfer: {from} → {to}")
}

impl TransactionsView {
    /// The selected pair "Mark as transfer" would record, if any.
    pub(super) fn markable_selection(&self) -> Option<[TransactionId; 2]> {
        markable_pair(&self.selected())
    }

    /// "Mark as transfer": records the selected pair as a confirmed
    /// transfer; the undo bar offers to reverse it.
    pub(super) fn mark_selected_as_transfer(&mut self, cx: &mut Context<Self>) {
        let Some([a, b]) = self.markable_selection() else {
            return;
        };
        let name = |id: TransactionId| {
            self.register
                .row(id)
                .and_then(|row| self.register.account(row.account))
                .map_or_else(String::new, |account| account.name.clone())
        };
        let a_out = self
            .register
            .row(a)
            .is_some_and(|row| row.amount.minor() < 0);
        let (from, to) = if a_out {
            (name(a), name(b))
        } else {
            (name(b), name(a))
        };
        let message = marked_message(&from, &to);
        let policy = Policy::new(Said::Category)
            .sentence()
            .log("couldn't mark a transfer pair")
            .undo(move |done: &TransferMarked| {
                Offer::Show(UndoAction::Marked(done.marked), message.into())
            })
            .emit(TransactionsEvent::Changed);
        self.write(policy, cx, |db| inbox::mark_transfer(db, a, b))
            .ok();
    }

    /// The undo bar's Undo after "Mark as transfer". If it fails, the bar
    /// offers it again with the reason; the table and the checks read
    /// again either way.
    pub(super) fn unmark_transfer(&mut self, marked: Marked, cx: &mut Context<Self>) {
        let policy = Policy::new(Said::Category)
            .sentence()
            .log("couldn't undo marking a transfer pair")
            .retry(UndoAction::Marked(marked))
            .emit(TransactionsEvent::Changed)
            .always();
        self.write(policy, cx, |db| transfer::unmark(db, &marked))
            .ok();
    }

    /// "Create rule": opens Rules on the rule every selected row gives,
    /// not saved; otherwise says why there's none.
    pub(super) fn create_rule_from_selection(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<TransactionId> = self.selected().iter().map(|row| row.id).collect();
        // Only a read: nothing to read again. A rule found takes away the
        // error under the Category select (an earlier "don't share" would
        // otherwise stay for rows that do).
        let policy = Policy::new(Said::Category).no_reload();
        if let Ok(definition) = self.write(policy, cx, |db| -> Result<_, StoreError> {
            let rules = ids
                .iter()
                .map(|id| rule::rule_from(db, *id))
                .collect::<Result<Vec<_>, _>>()?;
            shared_rule(rules).map_err(Into::into)
        }) {
            cx.emit(TransactionsEvent::CreateRule(definition, RuleOrigin::Edit));
        }
    }
}

#[cfg(test)]
mod tests {
    use orre_core::category::CategoryKey;
    use orre_core::rule::{AmountBounds, MatchField, MatchKind, Pattern, RuleAction};

    use super::*;

    fn definition(stem: &str, category: &str) -> RuleDefinition {
        RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Contains, stem).unwrap(),
            AmountBounds::ANY,
            None,
            vec![RuleAction::SetCategory(CategoryKey::new(category).unwrap())],
        )
        .unwrap()
    }

    #[test]
    fn rows_that_give_one_rule_share_it() {
        let pret = definition("pret", "eating_out");
        assert_eq!(
            shared_rule([Some(pret.clone()), Some(pret.clone()), Some(pret.clone())]),
            Ok(pret.clone())
        );
        assert_eq!(shared_rule([Some(pret.clone())]), Ok(pret));
    }

    #[test]
    fn rows_without_a_rule_or_with_different_ones_share_none() {
        let pret = definition("pret", "eating_out");
        assert_eq!(shared_rule([Some(pret.clone()), None]), Err(NO_RULE));
        assert_eq!(shared_rule(Vec::new()), Err(NO_RULE));
        assert_eq!(
            shared_rule([Some(pret.clone()), Some(definition("tesco", "groceries"))]),
            Err(DIFFERENT_RULES)
        );
        // The same name, a different category: a different rule.
        assert_eq!(
            shared_rule([Some(pret), Some(definition("pret", "groceries"))]),
            Err(DIFFERENT_RULES)
        );
        // A row without one is said first, whatever the others give.
        assert_eq!(
            shared_rule([
                Some(definition("pret", "eating_out")),
                Some(definition("tesco", "groceries")),
                None
            ]),
            Err(NO_RULE)
        );
    }

    #[test]
    fn the_wording() {
        assert_eq!(
            marked_message("Current account", "Savings"),
            "Marked as transfer: Current account → Savings"
        );
    }
}
