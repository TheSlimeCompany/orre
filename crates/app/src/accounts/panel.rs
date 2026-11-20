//! The accounts panel (`docs/DESIGN.md` → "Finance screens" → "Accounts
//! panel"; Paper `19T-0`, states board `8D8-0`): net worth, the accounts
//! with their pots nested under them, and the categories. The left panel of
//! every view that filters by account (Overview, Transactions, Trends,
//! Budgets, Net worth).
//!
//! A row shows the account's latest balance: `danger` when an account that
//! can be overdrawn is below zero, a 6px `danger` dot before it when a
//! balance check failed, and "No balance" until one is entered. Clicking a
//! row selects the account, and clicking it again goes back to all
//! accounts. The `+` opens New account. The panel is a Tab stop: there the
//! arrow keys move the selection through the rows, in panel order, and
//! Escape goes back to all accounts; the selected row draws the inset focus
//! ring while the panel has keyboard focus. Enter on a flagged account
//! opens its failing balance check, as the dot does; on any other account
//! it does nothing (`docs/DESIGN.md` → "Keyboard paths").
//!
//! Composed for M3.2 (not drawn): with no accounts, the Accounts section
//! says so in a `text-muted` row, and net worth reads "—"; accounts in more
//! than one currency show one net worth line per currency, since nothing is
//! converted until exchange rates arrive (M4.6). Asset and liability
//! accounts are listed under ASSETS AND LIABILITIES (D6.9), whose `+` opens
//! New account with Kind set to Asset (M3.17); with none, the section is
//! its label row and `+` alone (decided with the user, 2026-11-14). A
//! liability's negative balance isn't `danger`. ⌘N here adds a valuation
//! to a selected asset or liability.
//!
//! CATEGORIES lists each top-level group that holds transactions with how
//! many it holds, all time and in every account (core's
//! `Register::category_counts`, read with the Transactions register), most
//! first and Other last, then Uncategorised. Clicking a row shows
//! Transactions filtered to it (a group includes its subcategories) and
//! marks the row selected; clicking the selected row there clears it. The
//! `+` opens Rules on a new category. The arrow keys move through the
//! accounts only. Composed, not drawn (user, 2026-11-17).

use gpui_kit::base::actions::{Cancel, Confirm};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use orre_core::account::AccountKind;
use orre_core::id::AccountId;
use orre_core::reporting::account_tree::AccountNode;

use crate::format::{Sign, money};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font, size, spacing, typography};
use crate::ui::button::IconButton;
use crate::ui::icon::Glyph;
use crate::ui::list::{Keys, ListKeysExt as _};
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::marker::Marker;
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use super::data::AccountsData;
use super::{Accounts, AddValuation, CategoryPick};

/// What CATEGORIES shows, from the Transactions register.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelCategories {
    /// The groups that hold transactions, in the panel's order.
    pub groups: Vec<PanelCategory>,
    /// Transactions that need a category.
    pub uncategorised: usize,
    /// The row Transactions is filtered to, if any.
    pub picked: Option<CategoryPick>,
}

/// A group under CATEGORIES.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelCategory {
    /// Its category ID, as the Category filter takes it.
    pub id: i64,
    pub label: SharedString,
    pub marker: Marker,
    /// How many transactions it holds, its subcategories' included.
    pub rows: usize,
}

/// The element id of the Categories section's `+`.
pub const ADD_CATEGORY: &str = "add-category";

/// The element id of a group's row under CATEGORIES.
pub fn category_row_id(id: i64) -> SharedString {
    format!("category-{id}").into()
}

/// The element id of the Uncategorised row.
pub const UNCATEGORISED_ROW: &str = "uncategorised";

/// The panel's key context, where the arrow keys move the selection.
pub const CONTEXT: &str = "OrreAccountsPanel";

/// The panel's movement keys: ↑↓ through the accounts.
pub const KEYS: Keys = Keys::new(CONTEXT);

/// Escape in the panel: back to all accounts.
pub const CLEAR_KEY: &str = "escape";

/// Enter in the panel: the selected account's flagged dot.
pub const CHECK_KEY: &str = "enter";

/// ⌘N in the panel (and on the valuations canvas): add a valuation.
pub const ADD_VALUE_KEY: &str = "secondary-n";

/// The Accounts section's title.
pub const ACCOUNTS: &str = "Accounts";

/// The Assets and liabilities section's title.
pub const VALUED: &str = "Assets and liabilities";

/// What the Accounts section says with no accounts.
pub const NO_ACCOUNTS: &str = "No accounts yet";

/// What a row says after the name until a balance is entered.
pub const NO_BALANCE: &str = "No balance";

/// What net worth reads with no accounts.
pub const NO_NET_WORTH: &str = "—";

/// The Accounts section's `+`, by what it does.
pub const ADD_ACCOUNT_LABEL: &str = "Add account";

/// The Assets and liabilities section's `+`.
pub const ADD_VALUED_LABEL: &str = "Add asset or liability";

/// The top-level accounts, split as the panel lists them: the ones that
/// hold transactions (under ACCOUNTS), then the assets and liabilities.
pub fn sections(data: &AccountsData) -> (Vec<&AccountNode>, Vec<&AccountNode>) {
    let (valued, kept) = data
        .tree
        .accounts
        .iter()
        .partition(|node| !node.account.kind.holds_transactions());
    (kept, valued)
}

/// Net worth, one line per currency; none with no accounts.
pub fn net_worth(data: &AccountsData) -> Vec<String> {
    data.tree
        .net_worth
        .iter()
        .map(|total| money(total, Sign::Negative))
        .collect()
}

/// A row's latest balance, and whether it's `danger` (an account that can
/// be overdrawn, below zero); `None` until one is entered ([`NO_BALANCE`]).
pub fn latest(node: &AccountNode) -> Option<(String, bool)> {
    node.latest
        .map(|latest| (money(latest.balance, Sign::Negative), node.is_overdrawn()))
}

/// Every account in the panel's order: accounts, each followed by its pots,
/// then the assets and liabilities.
pub fn order(data: &AccountsData) -> Vec<AccountId> {
    let (kept, valued) = sections(data);
    kept.into_iter()
        .flat_map(|node| std::iter::once(node).chain(&node.pots))
        .chain(valued)
        .map(|node| node.account.id)
        .collect()
}

/// The net worth figure: Geist Mono 13/16 600.
const NET_WORTH: TextRole = TextRole {
    family: font::MONO,
    ..typography::CARD_TITLE
};

/// The element id of the Accounts section's `+`.
pub const ADD_ACCOUNT: &str = "add-account";

/// The element id of the Assets and liabilities section's `+`.
pub const ADD_VALUED_ACCOUNT: &str = "add-valued-account";

/// The icon for an account of `kind`.
pub fn glyph(kind: AccountKind) -> Glyph {
    match kind {
        AccountKind::Pot => Glyph::Pot,
        AccountKind::Cash => Glyph::Cash,
        AccountKind::Asset => Glyph::Asset,
        AccountKind::Liability => Glyph::Liability,
        AccountKind::Current | AccountKind::Savings | AccountKind::CreditCard => Glyph::Account,
    }
}

/// The element id of an account's row.
pub fn row_id(node: &AccountNode) -> SharedString {
    format!("account-{}", node.account.id).into()
}

/// A list under a section label: 6px inline, 4px top and 10px bottom
/// padding.
fn list_section(label: SectionLabelRow) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(spacing::S1)
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(label)
}

/// A row standing for no rows: 28px, 8px inline padding, `text-muted`.
fn note_row(text: impl Into<SharedString>) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .min_h(size::CONTROL)
        .px(spacing::S2)
        .text_role(typography::BODY)
        .text_color(color::TEXT_MUTED)
        .child(text.into())
}

fn account_row(
    node: &AccountNode,
    nested: bool,
    accounts: &Entity<Accounts>,
    selected: bool,
    ring: bool,
    focus: &FocusHandle,
) -> ListRow {
    let id = node.account.id;
    let accounts = accounts.clone();
    let focus = focus.clone();
    let flagged = accounts.clone();
    let row = ListRow::new(row_id(node), super::name(&node.account.name))
        .icon(glyph(node.account.kind))
        .nested(nested)
        .selected(selected)
        .focus_visible(selected && ring)
        .flagged(node.status == orre_core::reconciliation::Status::Mismatch)
        .on_flag_click(move |_, _, cx| {
            flagged.update(cx, |accounts, cx| accounts.request_checks(id, cx))
        })
        .on_click(move |_, window, cx| {
            // The panel takes focus, so the arrow keys go on from here.
            focus.focus(window, cx);
            accounts.update(cx, |accounts, cx| accounts.toggle(id, cx))
        });
    match latest(node) {
        Some((balance, overdrawn)) => row.trailing(balance).trailing_tone(if overdrawn {
            TrailingTone::Danger
        } else {
            TrailingTone::Muted
        }),
        None => row.trailing_words(NO_BALANCE),
    }
}

/// The panel, 272px, `chrome` with a `hairline` on its canvas side.
/// `focus` is the panel's own handle, kept by its owner.
pub fn render(
    accounts: &Entity<Accounts>,
    categories: &PanelCategories,
    focus: &FocusHandle,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let state = accounts.read(cx);
    let data = state.data();
    let selected = state.selected();
    let ring = focus.is_focused(window) && window.last_input_was_keyboard();
    #[cfg(test)]
    state.record_panel_ring(ring);

    let totals = net_worth(data);
    let net_worth = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S0_5)
        .pt(spacing::S3)
        .px(spacing::S3_5)
        .pb(spacing::S2)
        .child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child("Net worth"),
        )
        .map(|this| {
            if totals.is_empty() {
                this.child(
                    div()
                        .text_role(NET_WORTH)
                        .text_color(color::TEXT)
                        .child(NO_NET_WORTH),
                )
            } else {
                this.children(totals.into_iter().map(|total| {
                    div()
                        .text_role(NET_WORTH)
                        .text_color(color::TEXT)
                        .child(total)
                }))
            }
        });

    let (kept, valued) = sections(data);
    let add = {
        let accounts = accounts.clone();
        IconButton::new(ADD_ACCOUNT, Glyph::Plus, ADD_ACCOUNT_LABEL).on_click(move |_, _, cx| {
            accounts.update(cx, |accounts, cx| accounts.request_new_account(cx))
        })
    };
    let rows = kept.iter().flat_map(|node| {
        std::iter::once(account_row(
            node,
            false,
            accounts,
            selected == Some(node.account.id),
            ring,
            focus,
        ))
        .chain(node.pots.iter().map(|pot| {
            account_row(
                pot,
                true,
                accounts,
                selected == Some(pot.account.id),
                ring,
                focus,
            )
        }))
    });
    let accounts_section = list_section(SectionLabelRow::new(ACCOUNTS).action(add))
        .children(rows)
        .when(
            state.loaded() && kept.is_empty() && state.load_error().is_none(),
            |this| this.child(note_row(NO_ACCOUNTS)),
        )
        .when_some(state.load_error().cloned(), |this, error| {
            this.child(
                div()
                    .px(spacing::S2)
                    .text_role(typography::CAPTION)
                    .text_color(color::DANGER)
                    .child(error),
            )
        });

    let add_valued = {
        let accounts = accounts.clone();
        IconButton::new(ADD_VALUED_ACCOUNT, Glyph::Plus, ADD_VALUED_LABEL).on_click(
            move |_, _, cx| {
                accounts.update(cx, |accounts, cx| accounts.request_new_valued_account(cx))
            },
        )
    };
    let valued_section = list_section(SectionLabelRow::new(VALUED).action(add_valued)).children(
        valued.iter().map(|node| {
            account_row(
                node,
                false,
                accounts,
                selected == Some(node.account.id),
                ring,
                focus,
            )
        }),
    );

    let add_category = {
        let accounts = accounts.clone();
        IconButton::new(ADD_CATEGORY, Glyph::Plus, "Add category").on_click(move |_, _, cx| {
            accounts.update(cx, |accounts, cx| accounts.request_new_category(cx))
        })
    };
    let pick = |pick: CategoryPick| {
        let accounts = accounts.clone();
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
            accounts.update(cx, |accounts, cx| accounts.request_category(pick, cx))
        }
    };
    let categories_section = list_section(SectionLabelRow::new("Categories").action(add_category))
        .children(categories.groups.iter().map(|group| {
            let picked = CategoryPick::Category(group.id);
            ListRow::new(category_row_id(group.id), group.label.clone())
                .marker(group.marker)
                .selected(categories.picked == Some(picked))
                .trailing(group.rows.to_string())
                .trailing_tone(TrailingTone::Faint)
                .on_click(pick(picked))
        }))
        .child(
            ListRow::new(UNCATEGORISED_ROW, "Uncategorised")
                .marker(Marker::None)
                .dimmed(true)
                .selected(categories.picked == Some(CategoryPick::Uncategorised))
                .trailing(categories.uncategorised.to_string())
                .trailing_tone(TrailingTone::Faint)
                .on_click(pick(CategoryPick::Uncategorised)),
        );

    let (step, clear, add_value) = (accounts.clone(), accounts.clone(), accounts.clone());
    let (checks, panel) = (accounts.clone(), focus.clone());
    div()
        .id("left-panel")
        .key_context(CONTEXT)
        .track_focus(&focus.clone().tab_stop(true))
        .on_list_move(&KEYS, move |to, _, cx| {
            step.update(cx, |accounts, cx| accounts.select_next(*to, cx))
        })
        .on_action(move |_: &Cancel, _, cx| {
            clear.update(cx, |accounts, cx| accounts.select(None, cx))
        })
        // Enter on the panel itself (not a `+` in it): the selected
        // account's flagged dot.
        .on_action(move |_: &Confirm, window, cx| {
            if !panel.is_focused(window) {
                cx.propagate();
                return;
            }
            checks.update(cx, |accounts, cx| {
                accounts.request_selected_checks(cx);
            });
        })
        // ⌘N adds the record the selected account takes (D6.9): a valuation
        // here, a transaction on a bank or cash account (M3.10).
        .on_action(move |_: &AddValuation, window, cx| {
            if !add_value.update(cx, |accounts, cx| accounts.request_add_valuation(cx)) {
                window.dispatch_action(Box::new(crate::shell::NewTransaction), cx);
            }
        })
        .flex()
        .flex_col()
        .flex_none()
        .w(size::PANEL)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_r(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(net_worth)
        .child(accounts_section)
        .child(valued_section)
        .child(categories_section)
}
