//! The valuations canvas (`docs/DESIGN.md` → "Accounts you keep yourself";
//! Paper `BSI-0`): what Transactions shows instead of its table while an
//! asset or liability account is selected, since those hold values, not
//! transactions.
//!
//! - **Toolbar** (52px, bottom hairline): "Valuations" and how many values
//!   the user entered, and outline "Add valuation ⌘N".
//! - **Chart** (16px inset, 160px plot): "Value", "Held until the next
//!   valuation · Mar 2024 – today", and a 1.5px `text` step line: each value
//!   holds flat until the next, as net worth reads it, and runs on to
//!   today. No area fill, so the y axis fits the values; six `hairline`
//!   gridlines labelled in mono 10/12 `text-muted`, x labels on round
//!   months. Each value is a 6px `text-secondary` point; the latest is
//!   selected (a 6px `text` dot in a 2px `canvas` ring, its date and value
//!   on a `molten` label).
//! - **Table**: DATE · NOTE · CHANGE · VALUE, newest first; the oldest row
//!   is "· first value", CHANGE "—". A caption under it on how a value
//!   counts.
//!
//! Composed for M3.17 (decided with the user, 2026-11-14): a liability says
//! "Balances", "Add balance" and "Owed" where an asset says "Valuations",
//! "Add valuation" and "Value", its values negative as stored; one value
//! draws a flat line from it to today; with no values the toolbar sits over
//! the Empty state component (`13L-0`). A value without a note reads
//! "Valuation" (or "Balance") in `text-muted`.
//!
//! The chart's scale ([`Chart`]) and the table's rows ([`rows`]) are plain
//! functions with tests; money stays in minor units, and only positions on
//! screen are fractions.

use chrono::{Datelike as _, Months, NaiveDate};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, PathBuilder, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, point, px, relative,
};
use orre_core::account::{AccountKind, StoredBalance};
use orre_core::money::{Currency, Money};
use orre_core::reporting::account_tree::AccountNode;

use crate::format::{Sign, currency_prefix, list_date, money};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::button::Button;
use crate::ui::empty_state::EmptyState;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::{Accounts, AddValuation};

/// The canvas's key context, where ⌘N adds a valuation.
pub const CONTEXT: &str = "OrreValuations";

/// What the canvas calls things, by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Words {
    /// "Valuations" / "Balances": the toolbar's title.
    pub title: &'static str,
    /// "value" / "balance", for counts.
    pub noun: &'static str,
    pub nouns: &'static str,
    /// "Add valuation" / "Add balance".
    pub add: &'static str,
    /// "Value" / "Owed": the chart's title.
    pub chart: &'static str,
    /// "Valuation" / "Balance": a row without a note.
    pub unnamed: &'static str,
}

impl Words {
    pub const fn of(kind: AccountKind) -> Words {
        match kind {
            AccountKind::Liability => Words {
                title: "Balances",
                noun: "balance",
                nouns: "balances",
                add: "Add balance",
                chart: "Owed",
                unnamed: "Balance",
            },
            _ => Words {
                title: "Valuations",
                noun: "value",
                nouns: "values",
                add: "Add valuation",
                chart: "Value",
                unnamed: "Valuation",
            },
        }
    }
}

/// One row of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuationRow {
    pub date: NaiveDate,
    pub value: Money,
    /// Since the value before it; `None` for the first value.
    pub change: Option<Money>,
    pub note: Option<String>,
}

/// The table's rows, newest first, from the entered values (newest first,
/// as [`orre_core::account::entered_balances`] lists them).
pub fn rows(balances: &[StoredBalance]) -> Vec<ValuationRow> {
    balances
        .iter()
        .enumerate()
        .map(|(at, balance)| ValuationRow {
            date: balance.date,
            value: balance.balance,
            change: balances
                .get(at + 1)
                .and_then(|older| balance.balance.checked_sub(older.balance).ok()),
            note: balance.note.clone(),
        })
        .collect()
}

/// A row of the table as drawn, lane by lane.
pub(super) struct RowText {
    /// "12 Oct 2026".
    pub date: String,
    /// The note, or "Valuation" (`text-muted`) without one.
    pub note: String,
    /// "· first value", after the oldest row's note.
    pub first: Option<String>,
    /// Since the value before it, or "—".
    pub change: String,
    /// Whether there is a change ("—" is `text-muted`).
    pub changed: bool,
    pub value: String,
}

impl RowText {
    /// `row` as drawn; `first` for the oldest.
    pub(super) fn of(row: &ValuationRow, words: Words, first: bool) -> RowText {
        let change = row.change.filter(|_| !first);
        RowText {
            date: row.date.format("%-d %b %Y").to_string(),
            note: row.note.clone().unwrap_or_else(|| words.unnamed.to_owned()),
            first: first.then(|| format!("· first {}", words.noun)),
            change: change.map_or_else(|| "—".to_owned(), |change| money(change, Sign::Always)),
            changed: change.is_some(),
            value: money(row.value, Sign::Negative),
        }
    }
}

/// The toolbar's caption: how many values, and how net worth uses them.
pub(super) fn toolbar_caption(words: Words, count: usize) -> String {
    if count == 0 {
        format!(
            "No {} yet · net worth uses the latest one on or before each date",
            words.nouns
        )
    } else {
        format!(
            "{count} {} you entered · net worth uses the latest one on or before each date",
            if count == 1 { words.noun } else { words.nouns }
        )
    }
}

/// The caption under the table: from when the account counts.
pub(super) fn table_note(node: &AccountNode, rows: &[ValuationRow]) -> String {
    let first = rows
        .last()
        .map(|row| row.date.format("%-d %b %Y").to_string());
    let name = super::name(&node.account.name);
    match (node.account.kind, first) {
        (AccountKind::Liability, Some(first)) => format!(
            "A balance here sets what you owe on its date, so it isn't checked against transactions. {name} counts toward net worth from its first balance, {first}. Payments toward it stay spending on the account they leave."
        ),
        (_, Some(first)) => format!(
            "A valuation sets the balance on its date, so it isn't checked against transactions. {name} counts toward net worth from its first value, {first}. Payments toward it, like a deposit, stay on the account they left."
        ),
        (_, None) => String::new(),
    }
}

/// The empty state's title, with no values.
pub(super) fn empty_title(words: Words) -> String {
    format!("No {} yet", words.nouns)
}

/// The empty state's body.
pub(super) fn empty_body(node: &AccountNode) -> String {
    format!(
        "Add what {} is {}, and Orre holds it until the next one.",
        super::name(&node.account.name),
        if node.account.kind == AccountKind::Liability {
            "owed"
        } else {
            "worth"
        }
    )
}

/// The chart's scale: dates across, values up, as fractions of the plot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// The first day of the first value's month: the plot's left edge.
    pub start: NaiveDate,
    /// The first day of the month after today (or after the last value):
    /// the right edge.
    pub end: NaiveDate,
    pub today: NaiveDate,
    /// The top gridline, in minor units.
    pub top: i64,
    /// Between gridlines, in minor units; there are [`Chart::LINES`].
    pub step: i64,
    /// The values, oldest first, in minor units.
    pub points: Vec<(NaiveDate, i64)>,
    /// The months labelled on the x axis.
    pub months: Vec<NaiveDate>,
}

impl Chart {
    /// Six gridlines, five steps apart.
    pub const LINES: i64 = 6;

    /// The scale for `rows` (newest first), `None` with no rows.
    pub fn new(rows: &[ValuationRow], today: NaiveDate) -> Option<Chart> {
        let points: Vec<(NaiveDate, i64)> = rows
            .iter()
            .rev()
            .map(|row| (row.date, row.value.minor()))
            .collect();
        let (first, _) = *points.first()?;
        let last = points.last().map_or(today, |(date, _)| *date).max(today);
        let start = first.with_day(1)?;
        let end = last.with_day(1)?.checked_add_months(Months::new(1))?;
        let min = points.iter().map(|(_, minor)| *minor).min()?;
        let max = points.iter().map(|(_, minor)| *minor).max()?;
        let (top, step) = scale(min, max);
        Some(Chart {
            start,
            end,
            today,
            top,
            step,
            months: month_labels(start, end),
            points,
        })
    }

    /// The gridlines' values, top to bottom.
    pub fn ticks(&self) -> Vec<i64> {
        (0..Self::LINES).map(|n| self.top - n * self.step).collect()
    }

    /// How far across the plot `date` is, 0 to 1.
    pub fn x(&self, date: NaiveDate) -> f32 {
        let span = (self.end - self.start).num_days().max(1) as f32;
        ((date - self.start).num_days() as f32 / span).clamp(0., 1.)
    }

    /// How far down the plot `minor` is, 0 (top) to 1.
    pub fn y(&self, minor: i64) -> f32 {
        let span = (self.step * (Self::LINES - 1)) as f32;
        ((self.top - minor) as f32 / span).clamp(0., 1.)
    }
}

/// The top gridline and the step for values from `min` to `max`: the
/// smallest round step (1, 2 or 5 × 10ⁿ minor units) whose five steps hold
/// them, the top on a multiple of it. A flat line (one value, or the same
/// value each time) gets a range of a twentieth of its size, and any line
/// at least 100 minor units, so it sits inside.
fn scale(min: i64, max: i64) -> (i64, i64) {
    let range = (max - min).unsigned_abs();
    let span = if range == 0 {
        min.unsigned_abs().max(max.unsigned_abs()) / 20
    } else {
        range
    }
    .max(100);
    let mut steps = (0..19u32).flat_map(|power| [1u64, 2, 5].map(|base| base * 10u64.pow(power)));
    let mut step = steps.next().unwrap_or(1);
    loop {
        let fits = step * 5 >= span;
        let as_signed = i64::try_from(step).unwrap_or(i64::MAX);
        let top = ceil_to(max, as_signed);
        if fits && top - 5 * as_signed <= min {
            return (top, as_signed);
        }
        match steps.next() {
            Some(next) => step = next,
            None => return (max, as_signed),
        }
    }
}

/// `value` rounded up to a multiple of `step`.
fn ceil_to(value: i64, step: i64) -> i64 {
    -(-value).div_euclid(step) * step
}

/// The months to label between `start` and `end` (both left out): every
/// 1, 2, 3, 6, 12, 24 or 60 months, whichever first gives at most six.
fn month_labels(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
    let mut months = Vec::new();
    let mut month = start;
    while let Some(next) = month.checked_add_months(Months::new(1)) {
        if next >= end {
            break;
        }
        months.push(next);
        month = next;
    }
    for every in [1u32, 2, 3, 6, 12, 24, 60] {
        let picked: Vec<NaiveDate> = months
            .iter()
            .copied()
            .filter(|date| {
                let index = date.year() * 12 + date.month0() as i32;
                index.rem_euclid(every as i32) == 0
            })
            .collect();
        if picked.len() <= 6 {
            return picked;
        }
    }
    Vec::new()
}

/// The gridlines' labels: `£390k`, `−£212.5k`, `£1.2m`, `£40`. Thousands
/// or millions by the size of the values, and as many decimals (up to
/// two) as the step needs.
pub fn tick_labels(ticks: &[i64], step: i64, currency: Currency) -> Vec<String> {
    let unit = 10i64.pow(u32::from(currency.minor_units()));
    let size = ticks
        .iter()
        .map(|tick| tick.unsigned_abs())
        .max()
        .unwrap_or(0);
    let scale = if size >= 1_000_000 * unit.unsigned_abs() {
        1_000_000 * unit
    } else if size >= 1_000 * unit.unsigned_abs() {
        1_000 * unit
    } else {
        unit
    };
    let suffix = match scale / unit {
        1_000_000 => "m",
        1_000 => "k",
        _ => "",
    };
    let decimals = if step % scale == 0 {
        0
    } else if (step * 10) % scale == 0 {
        1
    } else {
        2
    };
    let prefix = currency_prefix(currency);
    let prefix = if prefix.chars().count() > 1 {
        format!("{prefix} ")
    } else {
        prefix
    };
    ticks
        .iter()
        .map(|&minor| {
            let whole = minor.unsigned_abs() / scale.unsigned_abs();
            let rest = minor.unsigned_abs() % scale.unsigned_abs();
            let number = match decimals {
                0 => whole.to_string(),
                places => {
                    let shift = 10u64.pow(places);
                    let fraction = rest * shift / scale.unsigned_abs();
                    format!("{whole}.{fraction:0width$}", width = places as usize)
                }
            };
            let sign = if minor < 0 { "\u{2212}" } else { "" };
            format!("{sign}{prefix}{number}{suffix}")
        })
        .collect()
}

/// The chart's selection label: 10/12 mono 600.
const SELECTION: TextRole = TextRole {
    weight: weight::STRONG,
    ..typography::MONO_MICRO
};

/// The plot's height: 160px.
const PLOT: f32 = 160.;

/// The y labels' column: 32px.
fn y_column() -> gpui_kit::Pixels {
    spacing::S4 * 2.
}

/// The canvas for the selected asset or liability account.
pub struct ValuationsView {
    accounts: Entity<Accounts>,
    focus: FocusHandle,
}

impl ValuationsView {
    pub fn new(accounts: Entity<Accounts>, cx: &mut Context<Self>) -> Self {
        cx.observe(&accounts, |_, _, cx| cx.notify()).detach();
        Self {
            accounts,
            focus: cx.focus_handle(),
        }
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        self.accounts.update(cx, |accounts, cx| {
            accounts.request_add_valuation(cx);
        });
    }

    fn render_toolbar(&self, words: Words, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let caption = toolbar_caption(words, count);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL + spacing::S6)
            .py(spacing::S3)
            .px(spacing::S4)
            .border_b(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .flex_none()
                    .text_role(typography::TITLE)
                    .text_color(color::TEXT)
                    .child(words.title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(caption),
            )
            .child(
                Button::new("add-valuation", words.add)
                    .shortcut("⌘N")
                    .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
            )
            .into_any_element()
    }

    fn render_chart(
        &self,
        node: &AccountNode,
        words: Words,
        chart: &Chart,
        today: NaiveDate,
    ) -> AnyElement {
        let currency = node.account.currency;
        let ticks = tick_labels(&chart.ticks(), chart.step, currency);
        let labels = ticks.into_iter().enumerate().map(|(at, label)| {
            let fraction = at as f32 / (Chart::LINES - 1) as f32;
            div()
                .absolute()
                .right_0()
                .top(px(PLOT * fraction) - typography::MONO_MICRO.line_height / 2.)
                .whitespace_nowrap()
                .text_role(typography::MONO_MICRO)
                .text_color(color::TEXT_MUTED)
                .child(label)
        });
        let grid = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom(-size::HAIRLINE)
            .flex()
            .flex_col()
            .justify_between()
            .children((0..Chart::LINES).map(|_| {
                div()
                    .flex_none()
                    .h_0()
                    .border_t(size::HAIRLINE)
                    .border_color(color::HAIRLINE)
            }));
        let steps: Vec<(f32, f32)> = chart
            .points
            .iter()
            .map(|(date, minor)| (chart.x(*date), chart.y(*minor)))
            .collect();
        let end = chart.x(chart.today);
        let line = {
            let steps = steps.clone();
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    let at = |(x, y): (f32, f32)| {
                        point(
                            bounds.origin.x + bounds.size.width * x,
                            bounds.origin.y + bounds.size.height * y,
                        )
                    };
                    let Some(&first) = steps.first() else {
                        return;
                    };
                    let mut path = PathBuilder::stroke(size::STROKE);
                    path.move_to(at(first));
                    let mut level = first.1;
                    for &(x, y) in &steps[1..] {
                        path.line_to(at((x, level)));
                        path.line_to(at((x, y)));
                        level = y;
                    }
                    path.line_to(at((end, level)));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color::TEXT);
                    }
                },
            )
            .absolute()
            .size_full()
        };
        let last = steps.len().saturating_sub(1);
        let dots = steps.iter().enumerate().map(|(at, &(x, y))| {
            let selected = at == last;
            let dot = if selected {
                let ring = spacing::S0_5;
                div()
                    .absolute()
                    .left(-(size::DOT / 2. + ring))
                    .top(-(size::DOT / 2. + ring))
                    .size(size::DOT + ring * 2.)
                    .rounded_full()
                    .bg(color::TEXT)
                    .border(ring)
                    .border_color(color::CANVAS)
            } else {
                div()
                    .absolute()
                    .left(-size::DOT / 2.)
                    .top(-size::DOT / 2.)
                    .size(size::DOT)
                    .rounded_full()
                    .bg(color::TEXT_SECONDARY)
            };
            div()
                .absolute()
                .left(relative(x))
                .top(relative(y))
                .size_0()
                .child(dot)
        });
        let latest = chart.points.last().map(|(date, minor)| {
            div()
                .absolute()
                .right(spacing::S0_5)
                .top(px(PLOT * chart.y(*minor)) - spacing::S6)
                .px(spacing::S1)
                .rounded(radius::LABEL)
                .bg(color::MOLTEN)
                .whitespace_nowrap()
                .text_role(SELECTION)
                .text_color(color::WHITE)
                .child(format!(
                    "{} · {}",
                    list_date(*date, today),
                    money(Money::new(*minor, currency), Sign::Negative)
                ))
        });
        let months = chart.months.iter().map(|month| {
            div()
                .absolute()
                .left(relative(chart.x(*month)))
                .top_0()
                .size_0()
                .child(
                    div()
                        .absolute()
                        .left(-spacing::S4 * 2.)
                        .w(spacing::S4 * 4.)
                        .flex()
                        .justify_center()
                        .whitespace_nowrap()
                        .text_role(typography::MONO_MICRO)
                        .text_color(color::TEXT_MUTED)
                        .child(month.format("%b %Y").to_string()),
                )
        });
        let since = chart.start.format("%b %Y");
        let verb = if node.account.kind == AccountKind::Liability {
            "Held until the next balance"
        } else {
            "Held until the next valuation"
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(spacing::S3)
            .pt(spacing::S4)
            .px(spacing::S4)
            .pb(spacing::S5)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .h(size::CONTROL_SM)
                    .child(
                        div()
                            .text_role(typography::SECTION_TITLE)
                            .text_color(color::TEXT)
                            .child(words.chart),
                    )
                    .child(
                        div()
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child(format!("{verb} · {since} – today")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(spacing::S2)
                    .h(px(PLOT))
                    .child(div().relative().flex_none().w(y_column()).children(labels))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h(px(PLOT))
                            .child(grid)
                            .child(line)
                            .children(dots)
                            .children(latest),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .h(typography::MONO_MICRO.line_height)
                    .ml(y_column() + spacing::S2)
                    .children(months),
            )
            .into_any_element()
    }

    fn render_table(&self, node: &AccountNode, words: Words, rows: &[ValuationRow]) -> AnyElement {
        let header_cell = |label: &'static str| {
            div()
                .flex()
                .text_color(color::TEXT_MUTED)
                .child(Tracked::new(typography::SECTION_LABEL, label))
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S3)
            .h(size::CONTROL)
            .px(spacing::S4)
            .border_t(size::HAIRLINE)
            .border_b(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .w(spacing::S6 * 3.)
                    .child(header_cell("Date")),
            )
            .child(div().flex().flex_1().min_w_0().child(header_cell("Note")))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .w(size::COLUMN_AMOUNT)
                    .child(header_cell("Change")),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .w(size::COLUMN_AMOUNT)
                    .child(header_cell("Value")),
            );
        let last = rows.len().saturating_sub(1);
        let body_rows = rows.iter().enumerate().map(|(at, row)| {
            let first = at == last;
            let text = RowText::of(row, words, first);
            let note = div()
                .flex()
                .flex_1()
                .min_w_0()
                .gap(spacing::S1)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_role(typography::BODY)
                // A value without a note reads "Valuation", quieter.
                .child(
                    div()
                        .text_color(if row.note.is_some() {
                            color::TEXT
                        } else {
                            color::TEXT_MUTED
                        })
                        .child(text.note),
                )
                .when_some(text.first, |this, first| {
                    this.child(div().text_color(color::TEXT_MUTED).child(first))
                });
            let change_color = if text.changed {
                color::TEXT_SECONDARY
            } else {
                color::TEXT_MUTED
            };
            let change = text.change;
            div()
                .id(SharedString::from(format!("valuation-{}", row.date)))
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S3)
                .h(size::CONTROL)
                .px(spacing::S2)
                .rounded(radius::MD)
                .child(
                    div()
                        .flex_none()
                        .w(spacing::S6 * 3.)
                        .whitespace_nowrap()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(text.date),
                )
                .child(note)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .justify_end()
                        .w(size::COLUMN_AMOUNT)
                        .whitespace_nowrap()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(change_color)
                        .child(change),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .justify_end()
                        .w(size::COLUMN_AMOUNT)
                        .whitespace_nowrap()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT)
                        .child(text.value),
                )
        });
        let note = table_note(node, rows);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(
                div()
                    .id("valuations-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(spacing::S2)
                    .pt(spacing::S1)
                    .children(body_rows)
                    .child(
                        div()
                            .flex_none()
                            .mt(spacing::S1)
                            .max_w(size::READING_MEASURE + spacing::S2 * 2.)
                            .px(spacing::S2)
                            .pt(spacing::S2)
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child(note),
                    ),
            )
            .into_any_element()
    }
}

impl Focusable for ValuationsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ValuationsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.accounts.read(cx);
        let today = accounts.data().today;
        let selected = accounts.selected_node().cloned();
        let detail = selected
            .as_ref()
            .and_then(|node| accounts.data().detail(node.account.id).cloned())
            .unwrap_or_default();
        let root = div()
            .id("valuations")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
            )
            .on_action(cx.listener(|this, _: &AddValuation, _, cx| this.add(cx)))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS);
        let Some(node) = selected else {
            return root;
        };
        let words = Words::of(node.account.kind);
        let rows = rows(&detail.balances);
        let root = root.child(self.render_toolbar(words, rows.len(), cx));
        match Chart::new(&rows, today) {
            Some(chart) => root
                .child(self.render_chart(&node, words, &chart, today))
                .child(self.render_table(&node, words, &rows)),
            None => root.child(
                div().flex().flex_1().items_center().justify_center().child(
                    EmptyState::new(empty_title(words))
                        .body(empty_body(&node))
                        .shortcut("⌘N", words.add),
                ),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use orre_core::reconciliation::BalanceSource;

    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    /// D6.9's House (`BSI-0`), newest first.
    fn house() -> Vec<StoredBalance> {
        [
            (date(2026, 10, 12), 38_500_000, Some("Remortgage survey")),
            (date(2026, 4, 20), 37_900_000, Some("Online estimate")),
            (
                date(2025, 8, 9),
                36_800_000,
                Some("Estate agent's valuation"),
            ),
            (date(2025, 2, 2), 36_150_000, Some("Online estimate")),
            (date(2024, 3, 14), 35_200_000, Some("Purchase price")),
        ]
        .into_iter()
        .map(|(date, minor, note)| StoredBalance {
            id: None,
            date,
            balance: gbp(minor),
            source: BalanceSource::Valuation,
            note: note.map(str::to_owned),
        })
        .collect()
    }

    #[test]
    fn rows_carry_the_change_since_the_value_before() {
        let rows = rows(&house());
        let changes: Vec<Option<i64>> = rows
            .iter()
            .map(|row| row.change.map(|change| change.minor()))
            .collect();
        assert_eq!(
            changes,
            [
                Some(600_000),
                Some(1_100_000),
                Some(650_000),
                Some(950_000),
                None
            ]
        );
    }

    #[test]
    fn the_chart_scales_as_paper_draws_it() {
        let chart = Chart::new(&rows(&house()), date(2026, 11, 10)).unwrap();
        // £340k to £390k in £10k steps.
        assert_eq!(
            chart.ticks(),
            [
                39_000_000, 38_000_000, 37_000_000, 36_000_000, 35_000_000, 34_000_000
            ]
        );
        assert_eq!(chart.start, date(2024, 3, 1));
        assert_eq!(chart.end, date(2026, 12, 1));
        // Paper's plot is 800px: the first point at 10.9, the latest at
        // 760.5 and 121.6 / 16 from the top of 160.
        assert!((chart.x(date(2024, 3, 14)) * 800. - 10.9).abs() < 1.);
        assert!((chart.x(date(2026, 10, 12)) * 800. - 760.5).abs() < 1.);
        assert!((chart.y(35_200_000) * 160. - 121.6).abs() < 0.1);
        assert!((chart.y(38_500_000) * 160. - 16.).abs() < 0.1);
        // Every six months, on January and July.
        assert_eq!(
            chart.months,
            [
                date(2024, 7, 1),
                date(2025, 1, 1),
                date(2025, 7, 1),
                date(2026, 1, 1),
                date(2026, 7, 1)
            ]
        );
        assert_eq!(
            tick_labels(&chart.ticks(), chart.step, Currency::GBP),
            ["£390k", "£380k", "£370k", "£360k", "£350k", "£340k"]
        );
    }

    #[test]
    fn a_liability_and_a_single_value_fit_inside_the_plot() {
        let owed = [StoredBalance {
            id: None,
            date: date(2026, 10, 1),
            balance: gbp(-21_264_000),
            source: BalanceSource::Valuation,
            note: None,
        }];
        let chart = Chart::new(&rows(&owed), date(2026, 11, 14)).unwrap();
        let ticks = chart.ticks();
        assert!(ticks[0] >= -21_264_000 && ticks[5] <= -21_264_000);
        assert!(chart.y(-21_264_000) > 0. && chart.y(-21_264_000) < 1.);
        assert!(tick_labels(&ticks, chart.step, Currency::GBP)[0].starts_with("\u{2212}£"));
        assert!(Chart::new(&[], date(2026, 11, 14)).is_none());
        // A short span labels every month.
        assert_eq!(
            month_labels(date(2026, 8, 1), date(2026, 12, 1)),
            [date(2026, 9, 1), date(2026, 10, 1), date(2026, 11, 1)]
        );
    }

    #[test]
    fn tick_labels_use_a_decimal_only_when_the_step_needs_one() {
        assert_eq!(
            tick_labels(&[21_250_000, 21_200_000], 50_000, Currency::GBP),
            ["£212.5k", "£212.0k"]
        );
        assert_eq!(
            tick_labels(&[4_000, 3_000], 1_000, Currency::GBP),
            ["£40", "£30"]
        );
        assert_eq!(
            tick_labels(&[120_000_000, 100_000_000], 20_000_000, Currency::GBP),
            ["£1.2m", "£1.0m"]
        );
        assert_eq!(tick_labels(&[500_000], 100_000, Currency::EUR), ["€5k"]);
        assert_eq!(
            tick_labels(&[-500_000], 100_000, Currency::EUR),
            ["\u{2212}€5k"]
        );
    }

    #[test]
    fn words_follow_the_kind() {
        assert_eq!(Words::of(AccountKind::Asset).add, "Add valuation");
        assert_eq!(Words::of(AccountKind::Liability).title, "Balances");
        assert_eq!(Words::of(AccountKind::Liability).chart, "Owed");
    }
}
