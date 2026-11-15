//! Rules from Transactions (M3.13): "Create a rule from this edit?" under
//! Details (`docs/DESIGN.md` → "Transactions additions" → "Rule from an
//! edit"; Paper `3WT-0`), and the Rule section (Paper `18E-0`).
//!
//! - **The prompt** appears after the user sets one row's category, when
//!   no rule decides that row's category and the proposed rule matches at
//!   least one earlier transaction (core's [`rule::propose`]). It names the
//!   rule, the matches, and how many have a category the user set. "Create
//!   rule…" opens Rules on the apply preview with the rule filled in; "Not
//!   now" is remembered for the pattern. It goes away when the selection
//!   changes. Neutral, not moldavite: it's Coffer's own prompt.
//! - **The Rule section**: "No rule matched" with outline "Create rule from
//!   this" (any row with a category); when a rule set the category, "A rule
//!   set this category" with outline "Open rule" (composed, `DESIGN.md` →
//!   "Rules" → "Built in M3.13").
//!
//! The view asks the shell through [`TransactionsEvent`]; nothing here
//! writes a rule.

use coffer_core::id::TransactionId;
use coffer_core::reporting::register::{RegisterRow, RowCategory};
use coffer_core::rule::{self, RuleOrigin};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::inbox::text::{NAME_SHOWN, Part, plain, rich};
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::inspector::{section, section_title};
use crate::ui::text::TextRoleExt as _;

use super::write::{Clears, Policy, Said};
use super::{Source, TransactionsEvent, TransactionsView};

/// The prompt for one row: core's proposal and what the card says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub proposal: rule::Proposal,
    /// The category it sets, as the card names it.
    pub category: String,
}

impl TransactionsView {
    /// Asks core for a rule after `transaction`'s category was set (also
    /// what snapshots call). Shows nothing when there's none to propose.
    pub fn propose_rule(&mut self, transaction: TransactionId, cx: &mut Context<Self>) {
        let Source::Database(store) = &self.source else {
            return;
        };
        self.editor.rule_prompt = match store.propose_rule(transaction) {
            Ok(Some(proposal)) => {
                let category = self
                    .register
                    .categories()
                    .iter()
                    .find(|category| category.key == proposal.category)
                    .map_or_else(
                        || proposal.category.as_str().to_owned(),
                        |c| c.label.clone(),
                    );
                Some(Prompt { proposal, category })
            }
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(%error, "couldn't propose a rule");
                None
            }
        };
        cx.notify();
    }

    /// "Create rule…".
    fn create_rule_from_prompt(&mut self, cx: &mut Context<Self>) {
        if let Some(prompt) = self.editor.rule_prompt.take() {
            cx.emit(TransactionsEvent::CreateRule(
                prompt.proposal.definition,
                RuleOrigin::Edit,
            ));
            cx.notify();
        }
    }

    /// "Not now": remembered for the pattern.
    fn dismiss_rule_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.editor.rule_prompt.take() else {
            return;
        };
        // Nothing to say if it isn't remembered: the prompt still goes.
        let policy = Policy::new(Said::Logged)
            .log("couldn't remember Not now")
            .no_reload();
        self.write(policy, cx, |store| {
            store.dismiss_rule(&prompt.proposal.definition)
        })
        .ok();
    }

    /// "Create rule from this".
    fn create_rule_from_row(&mut self, transaction: TransactionId, cx: &mut Context<Self>) {
        // Only a read: nothing to read again, and an earlier error stays.
        let policy = Policy::new(Said::Category)
            .clears(Clears::Nothing)
            .no_reload();
        if let Ok(definition) = self.write(policy, cx, |store| {
            store
                .rule_from(transaction)?
                .ok_or_else(|| "Coffer can’t find a name in this row to build a rule on.".into())
        }) {
            cx.emit(TransactionsEvent::CreateRule(definition, RuleOrigin::Edit));
        }
    }
}

/// The prompt's card for `row`, while it's the row the prompt is for
/// (Paper `4CJ-0`): `float`, 1px `hairline`, `radius-surface`, 12px
/// padding.
pub(super) fn card(
    view: &TransactionsView,
    row: &RegisterRow,
    window: &Window,
    cx: &mut Context<TransactionsView>,
) -> Option<AnyElement> {
    let prompt = view.editor.rule_prompt.as_ref()?;
    if prompt.proposal.transaction != row.id {
        return None;
    }
    let definition = &prompt.proposal.definition;
    let field = match definition.field() {
        rule::MatchField::Merchant => "Merchant",
        rule::MatchField::Description => "Bank text",
        rule::MatchField::SourceCategory => "Bank category",
        rule::MatchField::SourceType => "Bank type",
    };
    let proposal = &prompt.proposal;
    let matches = format!(
        "{} earlier {} match",
        proposal.matches,
        if proposal.matches == 1 {
            "transaction"
        } else {
            "transactions"
        }
    );
    let others = proposal
        .matches
        .saturating_sub(proposal.uncategorized + proposal.user_set);
    let mut split = Vec::new();
    if proposal.uncategorized > 0 {
        split.push(format!(
            "{} {} uncategorised",
            proposal.uncategorized,
            if proposal.uncategorized == 1 {
                "is"
            } else {
                "are"
            }
        ));
    }
    if others > 0 {
        split.push(format!(
            "{others} {} another category",
            if others == 1 { "has" } else { "have" }
        ));
    }
    if proposal.user_set > 0 {
        split.push(format!(
            "{} {} a category you set, which the rule would change",
            proposal.user_set,
            if proposal.user_set == 1 {
                "has"
            } else {
                "have"
            }
        ));
    }
    let detail = if split.is_empty() {
        String::new()
    } else {
        format!("{}.", split.join("; "))
    };
    // "Not now": a ghost compact button (24px, radius 6, 8px padding,
    // caption 500 `text-secondary`), drawn on `3WT-0`.
    let not_now = div()
        .id("rule-prompt-not-now")
        .flex()
        .flex_none()
        .items_center()
        .h(size::CONTROL_SM)
        .px(spacing::S2)
        .rounded(radius::MD)
        .hover(|style| style.bg(color::HOVER))
        .active(|style| style.bg(color::CONTROL_ACTIVE))
        .text_role(typography::CAPTION)
        .font_weight(weight::MEDIUM)
        .text_color(color::TEXT_SECONDARY)
        .on_click(cx.listener(|this, _, _, cx| this.dismiss_rule_prompt(cx)))
        .child("Not now");
    Some(
        div()
            .flex()
            .flex_col()
            .gap(spacing::S2_5)
            .p(spacing::S3)
            .rounded(radius::SURFACE)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S1)
                    .child(section_title("Create a rule from this edit?"))
                    .child(div().text_role(typography::CAPTION).child(rich(
                        window,
                        typography::CAPTION,
                        color::TEXT_MUTED,
                        vec![
                            Part::text(format!(
                                "{field} contains ",
                            )),
                            Part::mono(plain(definition.pattern().as_str(), NAME_SHOWN))
                                .color(color::TEXT_SECONDARY),
                            Part::text(format!(" → {}", prompt.category)),
                        ],
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::S0_5)
                    .child(
                        div()
                            .text_role(typography::BODY)
                            .text_color(color::TEXT_STRONG)
                            .child(matches),
                    )
                    .when(!detail.is_empty(), |this| {
                        this.child(
                            div()
                                .text_role(typography::CAPTION)
                                .text_color(color::TEXT_MUTED)
                                .child(detail),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::S1_5)
                    .child(
                        Button::new("rule-prompt-create", "Create rule…")
                            .with_variant(ButtonVariant::Outline)
                            .with_surface(color::FLOAT)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.create_rule_from_prompt(cx)),
                            ),
                    )
                    .child(not_now),
            )
            .into_any_element(),
    )
}

/// The Rule section (Paper `18E-0`, `1NI-0`).
pub(super) fn rule_section(row: &RegisterRow, cx: &mut Context<TransactionsView>) -> AnyElement {
    let id = row.id;
    let categorised = matches!(row.category, RowCategory::Category(_));
    let (words, button): (&str, Option<Button>) = match row.rule {
        Some(rule) => (
            "A rule set this category",
            Some(
                Button::new("rule-open", "Open rule")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(
                        cx.listener(move |_, _, _, cx| cx.emit(TransactionsEvent::OpenRule(rule))),
                    ),
            ),
        ),
        None => (
            "No rule matched",
            categorised.then(|| {
                Button::new("rule-create-from-row", "Create rule from this")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(cx.listener(move |this, _, _, cx| this.create_rule_from_row(id, cx)))
            }),
        ),
    };
    section(true)
        .gap(spacing::S2)
        .child(section_title("Rule"))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(spacing::S2)
                .child(
                    div()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child(SharedString::from(words)),
                )
                .children(button),
        )
        .into_any_element()
}
