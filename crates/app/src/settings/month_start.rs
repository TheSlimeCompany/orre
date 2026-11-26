//! Month start (ROADMAP M4.11; `docs/DESIGN.md` → "Month start"; Paper
//! `K57-0`, states `KEQ-0`): the rule every report cuts months by
//! (`docs/architecture/reporting.md` → "Decision record: the month start").
//!
//! The canvas sets the rule and shows what it does: Starts on (a select,
//! the 1st to the 28th, then "Last day of the month" under a hairline),
//! Weekends (Leave it / Use the Friday before, with the bank-holiday
//! caption), a preview of last, this and next month as of today (name,
//! dates with weekdays, day count, and why a start moved off a weekend),
//! and what changing it does. The inspector says the rule and lists what
//! follows it and what keeps its own dates.
//!
//! A choice applies at once, with no Save or confirm (decided by the user,
//! 2026-11-25): nothing is rewritten, so it can be changed back. The
//! shell then reloads every view whose months it cuts
//! ([`crate::shell::live::views`]).
//!
//! The words are plain functions of the rule and today, so the view draws
//! them and `describe()` reads them, and tests check them without a window.

use chrono::NaiveDate;
use gpui_kit::InteractiveElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, IntoElement, ParentElement as _, SharedString, Styled as _, Window,
    div,
};
use orre_core::reporting::Period;
use orre_core::reporting::period::{MonthStart, StartDay, Weekends};

use crate::format;
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::field::SelectField;
use crate::ui::menu::{MenuGroup, MenuOption, Select};
use crate::ui::segmented::Segmented;
use crate::ui::text::TextRoleExt as _;

use super::SettingsView;
use super::table::{self, Run, caption, inspector, inspector_header, plain, rich, toolbar};

pub const TITLE: &str = "Month start";
pub const CAPTION: &str = "The day your month begins · reports, budgets, the Inbox and \
     Transactions’ month presets all follow it";

/// The panel's section and row (Paper `KC1-0`; the section is "Calendar",
/// not the artboard's "Reports", decided by the user, 2026-11-25).
pub const PANEL_SECTION: &str = "Calendar";

pub const STARTS_ON: &str = "Starts on";
pub const WEEKENDS: &str = "Weekends";
pub const LEAVE_IT: &str = "Leave it";
pub const FRIDAY_BEFORE: &str = "Use the Friday before";
pub const WEEKENDS_CAPTION: &str = "A start that falls on a Saturday or Sunday moves back to \
     the Friday, as UK payroll does. Bank holidays don’t move it: Orre doesn’t know them.";
pub const YOUR_MONTHS: &str = "Your months";
pub const CHANGE_BACK: &str = "Changing this re-cuts every month, past ones too. Nothing is rewritten, so you can change it back.";

/// The inspector's title, over the start day.
pub const STARTS_ON_THE: &str = "Your month starts on the";
pub const FOLLOWS_IT: &str = "Follows it";
/// What cuts its months by the rule: the view, and what of it.
pub const FOLLOWS: [(&str, &str); 6] = [
    ("Overview", "its month and totals"),
    ("Budgets", "spent, rollover, pace"),
    ("Transactions", "the Date chip’s month presets"),
    ("Trends", "a bar a month"),
    ("Inbox", "“less in Nov” on a preview"),
    ("AI clients", "months with their dates"),
];
pub const KEEPS_ITS_OWN: &str = "Keeps its own dates";
/// What doesn't follow it, and the dates it keeps.
pub const KEEPS: [(&str, &str); 4] = [
    ("Weeks", "Monday to Sunday"),
    ("This year", "1 Jan to 31 Dec"),
    ("Schedules", "due on their own day"),
    ("Exports", "the dates you pick"),
];
pub const SAVED_REVIEWS: &str = "Saved reviews keep the period their author wrote, so a \
     review of October stays about the dates it covered.";

/// The Starts on select's key for the last day.
const LAST_KEY: &str = "last";
/// The Weekends control's keys.
const KEEP_KEY: &str = "keep";
const FRIDAY_KEY: &str = "friday";

/// The start day menu scrolls after this many rows (Paper `KGE-0`).
const MENU_ROWS: f32 = 8.;

/// `day` with its English suffix: 1st, 2nd, 3rd, 4th, 11th, 22nd.
pub fn ordinal(day: u32) -> String {
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

/// The start day as the panel and the inspector say it: "25th", "last day".
pub fn day_label(day: StartDay) -> String {
    day.day_of_month()
        .map_or_else(|| "last day".to_owned(), ordinal)
}

/// The panel row's trailing value: "25th", "Last".
pub fn panel_value(start: MonthStart) -> String {
    start
        .day()
        .day_of_month()
        .map_or_else(|| "Last".to_owned(), ordinal)
}

/// The Starts on field: the day, then "of the month" in `text-muted`.
pub fn field_value(day: StartDay) -> String {
    day.day_of_month()
        .map_or_else(|| "Last day".to_owned(), ordinal)
}

/// `day`'s key in the Starts on menu.
pub fn day_key(day: StartDay) -> SharedString {
    day.day_of_month()
        .map_or_else(|| LAST_KEY.into(), |day| day.to_string().into())
}

/// The start day with menu key `key`.
pub fn day_from_key(key: &str) -> Option<StartDay> {
    if key == LAST_KEY {
        return Some(StartDay::LAST);
    }
    key.parse().ok().and_then(StartDay::day)
}

/// The Starts on menu: the 1st to the 28th, then "Last day of the month"
/// under a hairline (Paper `KGE-0`).
pub fn day_groups() -> Vec<MenuGroup> {
    let days = (1..=StartDay::MAX_DAY)
        .map(|day| MenuOption::new(day.to_string(), ordinal(day)))
        .collect();
    vec![
        MenuGroup::new(None, days),
        MenuGroup::footer(vec![MenuOption::new(LAST_KEY, "Last day of the month")]),
    ]
}

/// The Weekends control's key for `weekends`.
pub fn weekends_key(weekends: Weekends) -> &'static str {
    match weekends {
        Weekends::Keep => KEEP_KEY,
        Weekends::FridayBefore => FRIDAY_KEY,
    }
}

/// The weekend choice with the Weekends control's key `key`.
pub fn weekends_from_key(key: &str) -> Option<Weekends> {
    match key {
        KEEP_KEY => Some(Weekends::Keep),
        FRIDAY_KEY => Some(Weekends::FridayBefore),
        _ => None,
    }
}

/// The caption under the inspector's start day.
pub fn rule_caption(start: MonthStart) -> &'static str {
    match (start == MonthStart::CALENDAR, start.weekends()) {
        (true, _) => "calendar months, as Orre starts",
        (false, Weekends::Keep) => "even when that’s a weekend",
        (false, Weekends::FridayBefore) => "or the Friday before, when that’s a weekend",
    }
}

/// "as of today, Tue 10 Nov": the preview's caption.
pub fn as_of(today: NaiveDate) -> String {
    format!("as of today, {}", today.format("%a %-d %b"))
}

/// A month's name: "November".
fn month_name(month: Period) -> String {
    month
        .first_day(MonthStart::CALENDAR)
        .format("%B")
        .to_string()
}

/// One month of the preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthRow {
    /// "Last month", "This month", "Next month".
    pub label: &'static str,
    /// "November".
    pub name: String,
    /// "Fri 23 Oct – Tue 24 Nov".
    pub dates: String,
    /// "33 days".
    pub days: String,
    /// Why its start isn't its day: "25 Oct is a Sunday, so November starts
    /// on the Friday before".
    pub moved: Option<String>,
    /// Today's month, which has the `hover` fill.
    pub current: bool,
}

/// Why `month`'s start moved off a weekend, when it did.
fn moved(month: Period, start: MonthStart) -> Option<String> {
    let nominal = month.first_day(MonthStart::new(start.day(), Weekends::Keep));
    (nominal != month.first_day(start)).then(|| {
        format!(
            "{} is a {}, so {} starts on the Friday before",
            nominal.format("%-d %b"),
            nominal.format("%A"),
            month_name(month),
        )
    })
}

/// Last, this and next month as of `today` under `start` (Paper `KCS-0`).
pub fn preview(today: NaiveDate, start: MonthStart) -> Vec<MonthRow> {
    let this = Period::containing(today, start);
    [
        ("Last month", this.previous()),
        ("This month", Some(this)),
        ("Next month", this.next()),
    ]
    .into_iter()
    .filter_map(|(label, month)| {
        let month = month?;
        let range = month.range(start);
        let days = (range.to() - range.from()).num_days() + 1;
        Some(MonthRow {
            label,
            name: month_name(month),
            dates: format::weekday_range(range),
            days: format!("{days} days"),
            moved: moved(month, start),
            current: month == this,
        })
    })
    .collect()
}

/// What the helper under the preview says first (Paper `KDJ-0`, `KGB-0`):
/// with calendar months, why to set a start; otherwise how months are
/// named, with this month as the example.
pub fn helper(today: NaiveDate, start: MonthStart) -> Vec<Run> {
    let lead = |text: &str| plain(text).color(color::TEXT_SECONDARY);
    if start == MonthStart::CALENDAR {
        let example = MonthStart::new(
            StartDay::day(25).expect("the 25th is a start day"),
            Weekends::Keep,
        );
        let month = Period::containing(today, example);
        let name = month_name(month);
        return vec![
            lead("Paid on a set day?"),
            plain(format!(
                " Start your month on payday, so a month holds what that pay covered. \
                 Months keep their name: with the 25th, {} is {name}, the month that pay \
                 is for, and {name}’s budget stays {name}’s.",
                format::day_range(month.range(example)),
            )),
        ];
    }
    let month = Period::containing(today, start);
    let name = month_name(month);
    // An early start day names the month its chosen day falls in, not the
    // day a weekend moves the start to: with the 1st and the Friday before,
    // November can start on 30 October.
    let (which, named, pay) = match start.day().day_of_month() {
        None => (
            "the last day".to_owned(),
            "the month it ends in".to_owned(),
            ", the month that pay is for",
        ),
        Some(_) if start.day().names_next_month() => (
            "the 16th or later".to_owned(),
            "the month it ends in".to_owned(),
            ", the month that pay is for",
        ),
        Some(day) => (
            "the 15th or earlier".to_owned(),
            match start.weekends() {
                Weekends::Keep => format!("the month its {} falls in", ordinal(day)),
                Weekends::FridayBefore => format!(
                    "the month its {} falls in, even when a weekend moves the start earlier",
                    ordinal(day)
                ),
            },
            "",
        ),
    };
    vec![
        lead("Months keep their name."),
        plain(format!(
            " A month starting on {which} is named after {named}, so {} is \
             {name}{pay}. Budgets stay attached to their month: {name}’s budget is still \
             {name}’s, now counted over these dates.",
            format::day_range(month.range(start)),
        )),
    ]
}

/// A label lane's text: 12/16 at `weight`.
fn body(weight: gpui_kit::FontWeight) -> TextRole {
    TextRole {
        weight,
        ..typography::BODY
    }
}

/// A 24px inspector row: the name (96px, 12/16 `text`) and what of it
/// (11/14 `text-muted`), Paper `KDP-0`.
fn listed_row(name: &'static str, what: &'static str) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL_SM)
        .child(
            div()
                .flex_none()
                .w(size::COLUMN_AMOUNT)
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(name),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(what),
        )
}

/// A preview row (Paper `KCS-0`): when, the month's name, its dates and
/// day count, and the reason its start moved on a second line. This month
/// has the `hover` fill and brighter text.
fn month_row(row: &MonthRow) -> Div {
    let (when, name, dates, days) = if row.current {
        (
            color::TEXT_STRONG,
            color::TEXT,
            color::TEXT,
            color::TEXT_SECONDARY,
        )
    } else {
        (
            color::TEXT_MUTED,
            color::TEXT_SECONDARY,
            color::TEXT_SECONDARY,
            color::TEXT_MUTED,
        )
    };
    let line = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL_XS)
        .child(
            div()
                .flex_none()
                .w(size::COLUMN_AMOUNT)
                .text_role(typography::BODY)
                .text_color(when)
                .child(row.label),
        )
        .child(
            div()
                .flex_none()
                .w(size::COLUMN_AMOUNT)
                .text_role(body(if row.current {
                    weight::STRONG
                } else {
                    weight::MEDIUM
                }))
                .text_color(name)
                .child(row.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_role(typography::MONO_AMOUNT)
                .text_color(dates)
                .child(row.dates.clone()),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .justify_end()
                .w(size::FIELD_LABEL)
                .whitespace_nowrap()
                .text_role(typography::MONO_VALUE)
                .text_color(days)
                .child(row.days.clone()),
        );
    div()
        .flex()
        .flex_col()
        .flex_none()
        .justify_center()
        .min_h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .when(row.current, |this| this.bg(color::HOVER))
        .when(row.moved.is_some(), |this| {
            this.py(spacing::S1_5).gap(spacing::S0_5)
        })
        .child(line)
        .when_some(row.moved.clone(), |this, moved| {
            this.child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S2)
                    .child(div().flex_none().w(size::COLUMN_AMOUNT * 2. + spacing::S2))
                    .child(caption(moved)),
            )
        })
}

/// A field row: a 64px `text-muted` label, then the control (Paper
/// `KCB-0`).
fn field_row(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .child(
            div()
                .flex_none()
                .w(size::FIELD_LABEL)
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child(label),
        )
        .child(control)
}

impl SettingsView {
    /// The Starts on select, wired to this view.
    fn day_select(&self, cx: &mut Context<Self>) -> Select {
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let day = self.month_start.day();
        let chosen = day_key(day);
        let open_chosen = chosen.clone();
        let groups = day_groups();
        let (moved, revealed) = (groups.clone(), groups.clone());
        Select::new(
            "month-start-day",
            STARTS_ON,
            SelectField::new("month-start-day-field", field_value(day)).tail("of the month"),
            groups,
            Some(chosen),
            &self.day_select,
        )
        .menu_width(size::COLUMN_CATEGORY)
        .max_height(size::CONTROL * MENU_ROWS)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                this.day_select
                    .set_open(open, Some(&open_chosen), window, cx);
                if open {
                    // Opens on the current choice (Paper `KGE-0`).
                    this.day_select.reveal(&revealed, &open_chosen);
                }
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.day_select.set_open(false, None, window, cx);
                if let Some(day) = day_from_key(key) {
                    let weekends = this.month_start.weekends();
                    this.set_month_start(MonthStart::new(day, weekends), cx);
                }
                cx.notify();
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.day_select.move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }

    pub(super) fn render_month_start(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let start = self.month_start;
        let today = self.clock.today;
        let view = cx.entity().downgrade();
        let weekends = Segmented::new("month-start-weekends", weekends_key(start.weekends()))
            .segment(KEEP_KEY, LEAVE_IT)
            .segment(FRIDAY_KEY, FRIDAY_BEFORE)
            .on_choose(move |key, _, cx| {
                if let Some(weekends) = weekends_from_key(key) {
                    let _ = view.update(cx, |this, cx| {
                        let day = this.month_start.day();
                        this.set_month_start(MonthStart::new(day, weekends), cx);
                    });
                }
            });
        let rule = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .px(spacing::S2)
            .child(field_row(
                STARTS_ON,
                div()
                    .flex()
                    .flex_none()
                    .w(size::COLUMN_CATEGORY)
                    .child(self.day_select(cx)),
            ))
            .child(field_row(WEEKENDS, weekends))
            .child(
                div()
                    .pl(size::FIELD_LABEL + spacing::S2)
                    .child(caption(WEEKENDS_CAPTION)),
            );
        let months = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .h(size::CONTROL)
                    .px(spacing::S2)
                    .child(
                        div()
                            .text_role(typography::SECTION_TITLE)
                            .text_color(color::TEXT)
                            .child(YOUR_MONTHS),
                    )
                    .child(caption(as_of(today))),
            )
            .children(preview(today, start).iter().map(month_row));
        let helper = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .px(spacing::S2)
            .pt(spacing::S3)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(rich(
                typography::CAPTION,
                color::TEXT_MUTED,
                helper(today, start),
            ))
            .child(caption(CHANGE_BACK));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(toolbar(TITLE, CAPTION, div()))
            .child(
                div()
                    .id("month-start-body")
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
                            .gap(spacing::S6)
                            .w(size::READING_MEASURE + spacing::S2 * 2.)
                            .px(spacing::S2)
                            .pt(spacing::S5)
                            .pb(spacing::S6)
                            .when_some(self.month_start_error.clone(), |this, error| {
                                this.child(
                                    div()
                                        .px(spacing::S2)
                                        .text_role(typography::CAPTION)
                                        .text_color(color::DANGER)
                                        .child(error),
                                )
                            })
                            .child(rule)
                            .child(months)
                            .child(helper),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_month_start_inspector(&self) -> AnyElement {
        let start = self.month_start;
        let header = inspector_header(
            div()
                .whitespace_nowrap()
                .text_role(typography::TITLE)
                .child(STARTS_ON_THE)
                .into_any_element(),
            Some(day_label(start.day())),
            caption(rule_caption(start)).into_any_element(),
        );
        let follows = table::section(Some(FOLLOWS_IT)).child(
            div()
                .flex()
                .flex_col()
                .children(FOLLOWS.iter().map(|(name, what)| listed_row(name, what))),
        );
        let keeps = table::section(Some(KEEPS_ITS_OWN))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(KEEPS.iter().map(|(name, what)| listed_row(name, what))),
            )
            .child(caption(SAVED_REVIEWS));
        inspector("month-start-inspector")
            .child(header)
            .child(follows)
            .child(keeps)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Datelike as _;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn rule(day: StartDay, weekends: Weekends) -> MonthStart {
        MonthStart::new(day, weekends)
    }

    fn the(day: u32) -> StartDay {
        StartDay::day(day).unwrap()
    }

    #[test]
    fn ordinals_take_their_english_suffixes() {
        let said: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 28]
            .into_iter()
            .map(ordinal)
            .collect();
        assert_eq!(
            said,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "28th"
            ]
        );
    }

    #[test]
    fn the_day_menu_runs_the_1st_to_the_28th_then_the_last_day_under_a_hairline() {
        let groups = day_groups();
        assert_eq!(groups[0].options.len(), 28);
        assert_eq!(groups[0].options[24].label.as_ref(), "25th");
        assert!(groups[1].footer);
        assert_eq!(groups[1].options[0].label.as_ref(), "Last day of the month");
        for option in groups.iter().flat_map(|group| &group.options) {
            let day = day_from_key(&option.key).expect("every key is a day");
            assert_eq!(day_key(day), option.key);
        }
        assert_eq!(day_from_key("29"), None);
        assert_eq!(day_from_key("0"), None);
    }

    /// Paper `K57-0`: the 25th, Friday before, on Tue 10 Nov 2026.
    #[test]
    fn the_preview_shows_last_this_and_next_month_with_weekdays_and_days() {
        let rows = preview(day(2026, 11, 10), rule(the(25), Weekends::FridayBefore));
        let lines: Vec<(&str, &str, &str, &str, bool)> = rows
            .iter()
            .map(|row| {
                (
                    row.label,
                    row.name.as_str(),
                    row.dates.as_str(),
                    row.days.as_str(),
                    row.current,
                )
            })
            .collect();
        assert_eq!(
            lines,
            [
                (
                    "Last month",
                    "October",
                    "Fri 25 Sep – Thu 22 Oct",
                    "28 days",
                    false
                ),
                (
                    "This month",
                    "November",
                    "Fri 23 Oct – Tue 24 Nov",
                    "33 days",
                    true
                ),
                (
                    "Next month",
                    "December",
                    "Wed 25 Nov – Thu 24 Dec",
                    "30 days",
                    false
                ),
            ]
        );
        let moved: Vec<Option<&str>> = rows.iter().map(|row| row.moved.as_deref()).collect();
        assert_eq!(
            moved,
            [
                None,
                Some("25 Oct is a Sunday, so November starts on the Friday before"),
                None
            ]
        );
    }

    /// Paper `KGE-0`: the last day with the Friday before, the case where a
    /// month-end payday falls on a Saturday.
    #[test]
    fn the_last_day_with_the_friday_before_moves_novembers_start_to_friday() {
        let rows = preview(
            day(2026, 11, 10),
            rule(StartDay::LAST, Weekends::FridayBefore),
        );
        let dates: Vec<(&str, &str)> = rows
            .iter()
            .map(|row| (row.dates.as_str(), row.days.as_str()))
            .collect();
        assert_eq!(
            dates,
            [
                ("Wed 30 Sep – Thu 29 Oct", "30 days"),
                ("Fri 30 Oct – Sun 29 Nov", "31 days"),
                ("Mon 30 Nov – Wed 30 Dec", "31 days"),
            ]
        );
        assert_eq!(day(2026, 10, 31).weekday(), chrono::Weekday::Sat);
        assert_eq!(
            rows[1].moved.as_deref(),
            Some("31 Oct is a Saturday, so November starts on the Friday before")
        );
        assert_eq!(rows[1].name, "November");
    }

    #[test]
    fn calendar_months_are_the_calendar_and_say_why_to_set_a_start() {
        let today = day(2026, 11, 10);
        let rows = preview(today, MonthStart::CALENDAR);
        assert_eq!(rows[1].dates, "Sun 1 Nov – Mon 30 Nov");
        assert!(rows.iter().all(|row| row.moved.is_none()));
        assert_eq!(
            Run::joined(&helper(today, MonthStart::CALENDAR)),
            "Paid on a set day? Start your month on payday, so a month holds what that pay \
             covered. Months keep their name: with the 25th, 25 Oct – 24 Nov is November, the \
             month that pay is for, and November’s budget stays November’s."
        );
        assert_eq!(
            rule_caption(MonthStart::CALENDAR),
            "calendar months, as Orre starts"
        );
    }

    #[test]
    fn the_helper_names_this_month_by_the_rule() {
        let today = day(2026, 11, 10);
        assert_eq!(
            Run::joined(&helper(today, rule(the(25), Weekends::FridayBefore))),
            "Months keep their name. A month starting on the 16th or later is named after the \
             month it ends in, so 23 Oct – 24 Nov is November, the month that pay is for. \
             Budgets stay attached to their month: November’s budget is still November’s, \
             now counted over these dates."
        );
        let fifth = Run::joined(&helper(today, rule(the(5), Weekends::Keep)));
        assert!(
            fifth.contains("the 15th or earlier is named after the month its 5th falls in, so 5 Nov – 4 Dec is November."),
            "{fifth}"
        );
        // Sun 1 Nov 2026 moves to Fri 30 Oct, yet the month is November.
        let first = Run::joined(&helper(today, rule(the(1), Weekends::FridayBefore)));
        assert!(
            first.contains(
                "the 15th or earlier is named after the month its 1st falls in, even when a \
                 weekend moves the start earlier, so 30 Oct – 30 Nov is November."
            ),
            "{first}"
        );
    }

    #[test]
    fn the_panel_and_inspector_say_the_day() {
        assert_eq!(panel_value(rule(the(25), Weekends::Keep)), "25th");
        assert_eq!(panel_value(rule(StartDay::LAST, Weekends::Keep)), "Last");
        assert_eq!(day_label(StartDay::LAST), "last day");
        assert_eq!(field_value(StartDay::LAST), "Last day");
        assert_eq!(
            rule_caption(rule(the(25), Weekends::FridayBefore)),
            "or the Friday before, when that’s a weekend"
        );
        assert_eq!(
            weekends_from_key(weekends_key(Weekends::FridayBefore)),
            Some(Weekends::FridayBefore)
        );
    }
}
