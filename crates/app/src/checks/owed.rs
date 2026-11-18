//! Owed to you (ROADMAP M3.11; `docs/DESIGN.md` → "Refunds, reimbursements
//! and shared costs" → Owed to you; Paper `CCF-0`, the empty queue on
//! `EB0-0`): the third CHECKS queue.
//!
//! - **Expense claims**, oldest first: a 16px status lane (the `link` icon
//!   on claims a payment matches) · the purchase · PAID FROM · SPENT · OWED.
//!   Under them, one strip per payment Coffer matched to some claims
//!   exactly, with "Link as reimbursement" and "Not this" (each one undoable
//!   change batch, the undo bar at the foot).
//! - **Shared costs**, one row per person: COSTS ("2 since 30 Oct", the
//!   outstanding ones), LAST PAID, OWED. A row selects the person.
//! - A closing caption.
//! - **Inspector**: the person, what they owe (unsigned: the label says the
//!   direction), their outstanding costs (their part of each), payments
//!   with "Link a payment…", and Name, which renames them everywhere.
//!
//! People are local labels and merchants untrusted text: shown plain.

use coffer_core::link::owed::{ClaimMatch, ClaimRow, OwedQueue, Person};
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, font, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::field::{FieldRow, error_caption, text_input};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{caption, header, section, section_title_row};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::Checks;
use super::text::{self, NAME_SHOWN, Part, plain, rich, unsigned};
use super::view::{click, empty_queue};

/// SPENT and LAST PAID: 72px (`--spacing-6` × 3, Paper `CRZ-0`).
fn date_lane() -> gpui_kit::Pixels {
    spacing::S6 * 3.
}

fn lane(width: gpui_kit::Pixels) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(width)
        .overflow_hidden()
        .whitespace_nowrap()
}

fn lane_label(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .text_color(color::TEXT_MUTED)
        .child(Tracked::new(typography::SECTION_LABEL, label))
}

/// A table row's frame (Paper `CUF-0`): 28px, 8px padding, 12px gaps, the
/// 16px status lane first.
fn row_frame(status: Option<Glyph>) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(size::ICON_LG)
                .children(status.map(|glyph| glyph.render().text_color(color::TEXT_MUTED))),
        )
}

/// A section's header row (Paper `CRT-0`): the title in caption 600
/// `text-secondary`, its count in mono, then the lane labels.
fn section_header(title: &'static str, count: usize, lanes: [&'static str; 3]) -> AnyElement {
    row_frame(None)
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(spacing::S2)
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_role(typography::CAPTION)
                        .font_weight(weight::STRONG)
                        .text_color(color::TEXT_SECONDARY)
                        .child(title),
                )
                .child(
                    div()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(count.to_string()),
                ),
        )
        .child(lane(size::COLUMN_ACCOUNT).child(lane_label(lanes[0])))
        .child(lane(date_lane()).child(lane_label(lanes[1])))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label(lanes[2])),
        )
        .into_any_element()
}

fn claim_row(claim: &ClaimRow, today: chrono::NaiveDate) -> AnyElement {
    row_frame(claim.matched.then_some(Glyph::Link))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(plain(&claim.merchant, NAME_SHOWN)),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child(plain(&claim.account_name, NAME_SHOWN)),
        )
        .child(
            lane(date_lane())
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(text::date(claim.date, today)),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(unsigned(claim.owed)),
        )
        .into_any_element()
}

/// The strip naming a matched payment (Paper `CX8-0`), indented to the
/// text lane: "+£150.60 from ACME LTD EXPENSES on 9 Nov matches the two
/// marked claims exactly. Link it and both clear."
/// What a match strip says, and the undo bar's message after linking it.
pub(super) fn match_text(found: &ClaimMatch, today: chrono::NaiveDate) -> (Vec<Part>, String) {
    let claims = found.claims.len();
    let (matches, clears) = match claims {
        1 => (
            "matches the marked claim exactly".to_owned(),
            "Link it and it clears.",
        ),
        2 => (
            "matches the two marked claims exactly".to_owned(),
            "Link it and both clear.",
        ),
        many => (
            format!("matches the {many} marked claims exactly"),
            "Link it and they all clear.",
        ),
    };
    let merchant = plain(&found.merchant, NAME_SHOWN);
    let words = vec![
        Part::mono(text::signed(found.amount)).color(color::TEXT_STRONG),
        Part::text(format!(" from {merchant}")).color(color::TEXT_STRONG),
        Part::text(" on "),
        Part::mono(text::date(found.date, today)),
        Part::text(format!(" {matches}. {clears}")),
    ];
    let message = format!(
        "Linked {merchant} to {}",
        text::count(claims, "claim", "claims")
    );
    (words, message)
}

/// A match strip's buttons.
pub(super) const LINK: &str = "Link as reimbursement";
pub(super) const NOT_THIS: &str = "Not this";

fn match_strip(
    found: &ClaimMatch,
    handle: &Entity<Checks>,
    today: chrono::NaiveDate,
    window: &Window,
) -> AnyElement {
    let (words, message) = match_text(found, today);
    let words = rich(window, typography::CAPTION, color::TEXT_MUTED, words);
    let (link, reject) = (handle.clone(), handle.clone());
    let (links, rejected) = (found.links.clone(), found.links.clone());
    let key = found.inflow;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .pl(spacing::S2 + size::ICON_LG + spacing::S3)
        .pr(spacing::S2)
        .pt(spacing::S1_5)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::CAPTION)
                .child(words),
        )
        .child(
            Button::new(SharedString::from(format!("owed-link-{key}")), LINK)
                .with_variant(ButtonVariant::Outline)
                .on_click(click(move |_, _, cx| {
                    let (links, message) = (links.clone(), message.clone());
                    link.update(cx, |checks, cx| checks.link_match(links, message, cx))
                })),
        )
        .child(
            Button::new(SharedString::from(format!("owed-reject-{key}")), NOT_THIS)
                .with_variant(ButtonVariant::Ghost)
                .on_click(click(move |_, _, cx| {
                    let links = rejected.clone();
                    reject.update(cx, |checks, cx| checks.reject_match(links, cx))
                })),
        )
        .into_any_element()
}

/// COSTS: "2 since 30 Oct", "1 on 8 Nov".
pub(super) fn costs_parts(person: &Person, today: chrono::NaiveDate) -> Vec<Part> {
    let first = person.costs.first().map(|cost| cost.date);
    match (person.costs.len(), first) {
        (0, _) | (_, None) => vec![Part::text("—")],
        (1, Some(date)) => vec![Part::text("1 on "), Part::mono(text::date(date, today))],
        (count, Some(date)) => vec![
            Part::text(format!("{count} since ")),
            Part::mono(text::date(date, today)),
        ],
    }
}

fn costs_words(person: &Person, today: chrono::NaiveDate, window: &Window) -> AnyElement {
    rich(
        window,
        typography::BODY,
        color::TEXT_MUTED,
        costs_parts(person, today),
    )
    .into_any_element()
}

/// LAST PAID: the day of their last payment, or "—".
pub(super) fn last_paid(person: &Person, today: chrono::NaiveDate) -> String {
    person
        .last_paid()
        .map_or_else(|| "—".to_owned(), |date| text::date(date, today))
}

fn person_row(
    person: &Person,
    selected: bool,
    handle: &Entity<Checks>,
    focus: &FocusHandle,
    today: chrono::NaiveDate,
    window: &Window,
) -> AnyElement {
    let lift = |rest| if selected { color::TEXT } else { rest };
    let (label, currency) = (person.label.clone(), person.owed.currency());
    let handle = handle.clone();
    let focus = focus.clone();
    row_frame(None)
        .id(SharedString::from(format!(
            "owed-person-{}-{currency}",
            person.label.as_str()
        )))
        .when(selected, |this| this.bg(color::MOLTEN_SOFT))
        .when(!selected, |this| this.hover(|style| style.bg(color::HOVER)))
        .on_click(move |_, window, cx| {
            focus.focus(window, cx);
            handle.update(cx, |checks, cx| {
                checks.toggle_person(label.clone(), currency, cx)
            })
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(plain(person.label.as_str(), NAME_SHOWN)),
        )
        .child(
            lane(size::COLUMN_ACCOUNT)
                .text_color(lift(color::TEXT_MUTED))
                .child(costs_words(person, today, window)),
        )
        .child(
            lane(date_lane())
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(last_paid(person, today)),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(unsigned(person.owed)),
        )
        .into_any_element()
}

/// The toolbar's caption: "£225.73 to come back · 3 expense claims and 2
/// people".
pub fn toolbar_caption(queue: &OwedQueue) -> Vec<Part> {
    let mut parts = Vec::new();
    for (index, total) in queue.totals.iter().enumerate() {
        if index > 0 {
            parts.push(Part::text(" and "));
        }
        parts.push(Part::mono(unsigned(*total)));
    }
    if queue.totals.is_empty() {
        parts.push(Part::text("Nothing to come back"));
        return parts;
    }
    parts.push(Part::text(" to come back"));
    let claims = text::count(queue.claims.len(), "expense claim", "expense claims");
    let people = text::count(queue.people.len(), "person", "people");
    parts.push(Part::text(format!(" · {claims} and {people}")));
    parts
}

/// What the canvas says instead of claims and people: why they couldn't
/// be read, or that nothing is owed.
pub(super) fn empty(checks: &Checks) -> Option<(&'static str, SharedString)> {
    if let Some(error) = checks.load_error() {
        return Some(("Owed to you couldn’t be read", error.clone()));
    }
    let queue = &checks.data().owed;
    (queue.claims.is_empty() && queue.people.is_empty()).then(|| {
        (
            "Nothing owed to you",
            "Mark a purchase as shared or to claim back, and it waits here until the money \
             comes back."
                .into(),
        )
    })
}

/// The table's sections' titles.
pub(super) const CLAIMS: &str = "Expense claims";
pub(super) const SHARED: &str = "Shared costs";

pub fn render_body(
    handle: &Entity<Checks>,
    checks: &Checks,
    focus: &FocusHandle,
    window: &Window,
) -> AnyElement {
    let today = checks.today();
    if let Some((title, body)) = empty(checks) {
        return empty_queue(Glyph::OwedLarge, title, body).into_any_element();
    }
    let queue = &checks.data().owed;
    let mut sections: Vec<AnyElement> = Vec::new();
    if !queue.claims.is_empty() {
        sections.push(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(spacing::S2)
                .pb(spacing::S3)
                .child(section_header(
                    CLAIMS,
                    queue.claims.len(),
                    ["Paid from", "Spent", "Owed"],
                ))
                .children(queue.claims.iter().map(|claim| claim_row(claim, today)))
                .children(
                    queue
                        .matches
                        .iter()
                        .map(|found| match_strip(found, handle, today, window)),
                )
                .into_any_element(),
        );
    }
    if !queue.people.is_empty() {
        let divided = !sections.is_empty();
        sections.push(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(spacing::S2)
                .pb(spacing::S3)
                .when(divided, |this| {
                    this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
                })
                .child(section_header(
                    SHARED,
                    queue.people.len(),
                    ["Costs", "Last paid", "Owed"],
                ))
                .children(queue.people.iter().map(|person| {
                    person_row(
                        person,
                        checks.is_person_selected(person),
                        handle,
                        focus,
                        today,
                        window,
                    )
                }))
                .into_any_element(),
        );
    }
    div()
        .id("checks-owed-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(spacing::S2)
                .px(spacing::S2)
                .pb(spacing::S4)
                .children(sections)
                .child(
                    div()
                        .pt(spacing::S4)
                        .px(spacing::S2)
                        .border_t(size::HAIRLINE)
                        .border_color(color::HAIRLINE)
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(
                            "Mark a purchase as shared or to claim back in its inspector. It \
                             stays here until a repayment is linked; a part payment leaves the \
                             rest owed. People are labels on this computer and are never sent \
                             to an AI client.",
                        ),
                ),
        )
        .into_any_element()
}

/// A 28px list row in the inspector (Paper `E6O-0`, `E7P-0`): mono date in
/// a 48px lane, the name, the mono amount.
fn inspector_row(date: String, name: String, amount: String, tone: gpui_kit::Rgba) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .child(
            div()
                .flex_none()
                .w(spacing::S6 * 2.)
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(date),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(tone)
                .child(name),
        )
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_AMOUNT)
                .text_color(tone)
                .child(amount),
        )
        .into_any_element()
}

/// The inspector's header for a person: their name, what they owe, and
/// "Owes you for 2 shared costs".
pub(super) fn header_text(person: &Person) -> (String, String, String) {
    (
        plain(person.label.as_str(), NAME_SHOWN),
        unsigned(person.owed),
        format!(
            "Owes you for {}",
            text::count(person.costs.len(), "shared cost", "shared costs")
        ),
    )
}

/// A payment's name in the inspector: "Sam K · bank transfer".
pub(super) fn payment_what(payment: &coffer_core::link::owed::Payment) -> String {
    match payment.payment_type.as_deref().map(str::trim) {
        Some(kind) if !kind.is_empty() => format!(
            "{} · {}",
            plain(&payment.merchant, NAME_SHOWN),
            plain(&kind.to_lowercase(), NAME_SHOWN)
        ),
        _ => plain(&payment.merchant, NAME_SHOWN),
    }
}

/// The inspector with nothing selected.
pub(super) const NOTHING_SELECTED: &str =
    "Select a person to see what they owe, link a payment from them or rename them.";

/// The inspector (Paper `CCW-0`). `name` is the Name field, made by the
/// view; `name_error` why the last rename was refused.
pub fn render_inspector(
    handle: &Entity<Checks>,
    checks: &Checks,
    name: Option<&Entity<InputState>>,
    name_error: Option<SharedString>,
    window: &Window,
) -> AnyElement {
    let today = checks.today();
    let mut sections: Vec<AnyElement> = Vec::new();
    match checks.selected_person() {
        Some(person) => {
            let shown = plain(person.label.as_str(), NAME_SHOWN);
            let (title, figure, words) = header_text(person);
            sections.push(
                header(title, Some(figure), Some(caption(words).into_any_element()))
                    .into_any_element(),
            );
            sections.push(
                section(true)
                    .gap(spacing::S2)
                    .child(section_title_row(
                        "Costs",
                        Some(
                            div()
                                .text_role(typography::CAPTION)
                                .text_color(color::TEXT_MUTED)
                                .child(format!("{shown}'s part"))
                                .into_any_element(),
                        ),
                    ))
                    .children(person.costs.iter().map(|cost| {
                        inspector_row(
                            text::date(cost.date, today),
                            plain(&cost.merchant, NAME_SHOWN),
                            unsigned(cost.part),
                            color::TEXT,
                        )
                    }))
                    .into_any_element(),
            );
            let request = handle.clone();
            let settled = person.last_paid().map(|date| {
                vec![
                    Part::text("Settled everything up to "),
                    Part::mono(text::date(date, today)),
                    Part::text(". "),
                ]
            });
            let mut words = settled.unwrap_or_default();
            words.push(Part::text(
                "Paid in cash? Add it to your Cash account, then link it here.",
            ));
            sections.push(
                section(true)
                    .gap(spacing::S2)
                    .child(section_title_row(
                        format!("Payments from {shown}"),
                        Some(
                            Button::new("owed-link-payment", "Link a payment…")
                                .with_variant(ButtonVariant::Outline)
                                .on_click(click(move |_, _, cx| {
                                    request.update(cx, |checks, cx| checks.request_link_payment(cx))
                                }))
                                .into_any_element(),
                        ),
                    ))
                    .children(person.payments.iter().map(|payment| {
                        inspector_row(
                            text::date(payment.date, today),
                            payment_what(payment),
                            text::signed(payment.amount),
                            color::TEXT_SECONDARY,
                        )
                    }))
                    .child(div().text_role(typography::CAPTION).child(rich(
                        window,
                        typography::CAPTION,
                        color::TEXT_MUTED,
                        words,
                    )))
                    .into_any_element(),
            );
            let field = name.map(|name| {
                text_input(name)
                    .w_full()
                    .gap(spacing::S1_5)
                    .prefix(Glyph::Lock.render().text_color(color::TEXT_MUTED))
                    .font_family(font::UI)
            });
            sections.push(
                section(true)
                    .gap(spacing::S2)
                    .children(field.map(|field| FieldRow::new("Name", field)))
                    .children(name_error.map(error_caption))
                    .child(
                        div()
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child(
                                "A label on this computer only. It's never sent to an AI client.",
                            ),
                    )
                    .into_any_element(),
            );
        }
        None => sections.push(
            section(false)
                .child(
                    div()
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(NOTHING_SELECTED),
                )
                .into_any_element(),
        ),
    }
    div()
        .id("checks-inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
        .into_any_element()
}
