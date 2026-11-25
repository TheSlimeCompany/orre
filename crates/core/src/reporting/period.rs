//! Months ([`Period`], `YYYY-MM`) cut under the user's [`MonthStart`], and
//! inclusive date ranges ([`DateRange`]).
//!
//! A month runs from one month start to the day before the next
//! (`docs/architecture/reporting.md` → "Decision record: the month start").
//! The start is a day of the month, 1 to 28, or its last day
//! ([`StartDay`]), and a start on a Saturday or Sunday is kept or moved
//! back to the Friday before ([`Weekends`]). No bank holidays.
//!
//! A [`Period`] is a month's name, `YYYY-MM`: reading, writing and stepping
//! names (`next`, `previous`, `through`) never needs the rule. Its dates
//! do: [`Period::first_day`], [`Period::last_day`], [`Period::range`] and
//! [`Period::containing`] take the [`MonthStart`], so every caller says
//! which months it means.
//!
//! - **Naming.** A month's *nominal start* is the start day in a calendar
//!   month. Days 1 to 15 name the month after the calendar month of its
//!   nominal start; days 16 to 28 and the last day after the next calendar
//!   month. With the 25th, 25 Oct to 24 Nov is `2026-11`; with the 16th,
//!   16 Mar to 15 Apr is April.
//! - **Weekends.** The first day is the nominal start, moved back to the
//!   Friday before when [`Weekends::FridayBefore`] is chosen and it falls
//!   on a weekend. The name always comes from the nominal start, so a move
//!   never renames a month: with the 1st and the Friday move, `2026-08`
//!   starts on Friday 31 Jul 2026.
//! - **Months tile the days.** A month's last day is the day before the
//!   next month's first, so months never overlap and leave no gap, and
//!   they are never empty: nominal starts are at least 28 days apart and a
//!   move is at most 2 days.
//! - **The edges.** Names run from `0001-01` to `9999-12`, and their dates
//!   from 0001-01-01 to 9999-12-31, which is what a date stored as
//!   `YYYY-MM-DD` can hold: `0001-01` always starts on 0001-01-01 and
//!   `9999-12` always ends on 9999-12-31, so under most rules they are
//!   shorter or longer than the rest. [`Period::containing`] is infallible:
//!   a date before 0001-01-01 is put in `0001-01`, and one after 9999-12-31
//!   in `9999-12`; for those dates alone the month's range doesn't contain
//!   them.

use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};

use super::Error;

/// The day of the month a month starts on: day 1 to 28, or the last day.
/// Days 29 to 31 aren't in every month; [`StartDay::LAST`] covers month-end
/// pay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StartDay(Day);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Day {
    /// 1 to [`StartDay::MAX_DAY`].
    Of(u8),
    Last,
}

impl StartDay {
    /// The 1st: calendar months.
    pub const FIRST: StartDay = StartDay(Day::Of(1));

    /// The last day of the month.
    pub const LAST: StartDay = StartDay(Day::Last);

    /// The latest numbered start day.
    pub const MAX_DAY: u32 = 28;

    /// The latest numbered start day that names a month after the calendar
    /// month it starts in; later days, and the last day, name it after the
    /// next one.
    pub const LAST_SAME_MONTH_NAME: u32 = 15;

    /// Day `day` of the month; `None` unless it is 1 to [`Self::MAX_DAY`].
    pub fn day(day: u32) -> Option<StartDay> {
        u8::try_from(day)
            .ok()
            .filter(|&day| (1..=Self::MAX_DAY).contains(&u32::from(day)))
            .map(|day| StartDay(Day::Of(day)))
    }

    /// The day of the month, 1 to [`Self::MAX_DAY`]; `None` for the last
    /// day.
    pub fn day_of_month(self) -> Option<u32> {
        match self.0 {
            Day::Of(day) => Some(u32::from(day)),
            Day::Last => None,
        }
    }

    pub fn is_last(self) -> bool {
        self.0 == Day::Last
    }

    /// Whether a month starting on this day is named after the calendar
    /// month after the one its nominal start is in (module docs).
    pub fn names_next_month(self) -> bool {
        match self.0 {
            Day::Of(day) => u32::from(day) > Self::LAST_SAME_MONTH_NAME,
            Day::Last => true,
        }
    }
}

/// What happens to a month start that falls on a Saturday or Sunday.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Weekends {
    /// The month starts on the day, weekend or not.
    #[default]
    Keep,
    /// The month starts on the Friday before, as UK payroll pays.
    FridayBefore,
}

/// The rule that cuts months: one for the whole database, stored in the
/// `reporting.month_start` setting ([`crate::setting::month_start`]). The
/// default is [`MonthStart::CALENDAR`], the 1st with weekends kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MonthStart {
    day: StartDay,
    weekends: Weekends,
}

impl MonthStart {
    /// Calendar months: the 1st, weekends kept. For callers that mean a
    /// calendar month rather than the user's month.
    pub const CALENDAR: MonthStart = MonthStart {
        day: StartDay::FIRST,
        weekends: Weekends::Keep,
    };

    pub const fn new(day: StartDay, weekends: Weekends) -> MonthStart {
        MonthStart { day, weekends }
    }

    pub fn day(self) -> StartDay {
        self.day
    }

    pub fn weekends(self) -> Weekends {
        self.weekends
    }

    /// The first day of the month whose name is the calendar month starting
    /// on `name` (a 1st), before the edges' clamp ([`Period::first_day`]).
    /// Any year chrono holds: unclamped, the edges reach years 0 and 10000.
    fn first_day_of(self, name: NaiveDate) -> NaiveDate {
        let nominal = match self.day.0 {
            Day::Of(day) if u32::from(day) <= StartDay::LAST_SAME_MONTH_NAME => {
                name.with_day(u32::from(day))
            }
            Day::Of(day) => name
                .checked_sub_months(Months::new(1))
                .and_then(|before| before.with_day(u32::from(day))),
            Day::Last => name.pred_opt(),
        }
        .expect("a start day of 1 to 28, or the last, is in every month");
        let back = match (self.weekends, nominal.weekday()) {
            (Weekends::FridayBefore, Weekday::Sat) => 1,
            (Weekends::FridayBefore, Weekday::Sun) => 2,
            _ => 0,
        };
        nominal
            .checked_sub_days(Days::new(back))
            .expect("two days back stays in chrono's range")
    }
}

impl Default for MonthStart {
    fn default() -> MonthStart {
        MonthStart::CALENDAR
    }
}

/// A month's name, `YYYY-MM` (years 1–9999). Its dates come from a
/// [`MonthStart`]; the name doesn't (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Period {
    /// The 1st of the calendar month the name is: the key, not necessarily
    /// a day of the month.
    name: NaiveDate,
}

impl Period {
    /// The first name, `0001-01`.
    pub const MIN: Period = Period {
        name: NaiveDate::from_ymd_opt(1, 1, 1).expect("0001-01-01 is a date"),
    };

    /// The last name, `9999-12`.
    pub const MAX: Period = Period {
        name: NaiveDate::from_ymd_opt(9999, 12, 1).expect("9999-12-01 is a date"),
    };

    /// Month `month` (1–12) of `year` (1–9999).
    pub fn new(year: i32, month: u32) -> Result<Period, Error> {
        if !(1..=9999).contains(&year) {
            return Err(Error::InvalidPeriod);
        }
        NaiveDate::from_ymd_opt(year, month, 1)
            .map(|name| Period { name })
            .ok_or(Error::InvalidPeriod)
    }

    /// The month `date` is in under `start`; [`Period::MIN`] or
    /// [`Period::MAX`] for a date before or after every month (module
    /// docs).
    pub fn containing(date: NaiveDate, start: MonthStart) -> Period {
        if date < Period::MIN.first_day(start) {
            return Period::MIN;
        }
        if date > Period::MAX.last_day(start) {
            return Period::MAX;
        }
        // Named after the calendar month of `date`, the one after (a start
        // after the 15th, or a weekend move) or the one before: the latest
        // name of them that starts on or before `date`. `date` is in
        // 0001-01-01 to 9999-12-31, so some name always does: `0001-01`
        // starts on the first of those days.
        let calendar = date.with_day(1).expect("every month has a first day");
        [
            calendar.checked_add_months(Months::new(1)),
            Some(calendar),
            calendar.checked_sub_months(Months::new(1)),
        ]
        .into_iter()
        .flatten()
        .map(|name| Period { name })
        .filter(|period| (Period::MIN..=Period::MAX).contains(period))
        .find(|period| period.first_day(start) <= date)
        .unwrap_or(Period::MIN)
    }

    pub fn year(&self) -> i32 {
        self.name.year()
    }

    /// 1–12.
    pub fn month(&self) -> u32 {
        self.name.month()
    }

    /// The month's first day under `start`: its nominal start, moved back
    /// off a weekend when `start` says so; 0001-01-01 for `0001-01`.
    pub fn first_day(&self, start: MonthStart) -> NaiveDate {
        if *self == Period::MIN {
            return Period::MIN.name;
        }
        start.first_day_of(self.name)
    }

    /// The month's last day under `start`: the day before the next month's
    /// first; 9999-12-31 for `9999-12`.
    pub fn last_day(&self, start: MonthStart) -> NaiveDate {
        if *self == Period::MAX {
            return NaiveDate::from_ymd_opt(9999, 12, 31).expect("9999-12-31 is a date");
        }
        self.name
            .checked_add_months(Months::new(1))
            .map(|next| start.first_day_of(next))
            .and_then(|first| first.pred_opt())
            .expect("the month after 9999-12 is in chrono's range")
    }

    /// Every day of the month under `start`.
    pub fn range(&self, start: MonthStart) -> DateRange {
        DateRange {
            from: self.first_day(start),
            to: self.last_day(start),
        }
    }

    /// The month after, `None` after 9999-12.
    pub fn next(&self) -> Option<Period> {
        self.name
            .checked_add_months(Months::new(1))
            .filter(|name| name.year() <= 9999)
            .map(|name| Period { name })
    }

    /// The month before, `None` before 0001-01.
    pub fn previous(&self) -> Option<Period> {
        self.name
            .checked_sub_months(Months::new(1))
            .filter(|name| name.year() >= 1)
            .map(|name| Period { name })
    }

    /// Every month from this one through `last`, both included; empty when
    /// `last` is earlier.
    pub fn through(self, last: Period) -> impl Iterator<Item = Period> {
        std::iter::successors(Some(self), |period| period.next())
            .take_while(move |period| *period <= last)
    }

    /// `YYYY-MM`, as stored in `budget.month`.
    pub fn key(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year(), self.month())
    }
}

impl FromStr for Period {
    type Err = Error;

    /// Exactly `YYYY-MM`: four digits, a hyphen, two digits.
    fn from_str(text: &str) -> Result<Period, Error> {
        let bytes = text.as_bytes();
        let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
        if bytes.len() != 7 || bytes[4] != b'-' || !digits(0..4) || !digits(5..7) {
            return Err(Error::InvalidPeriod);
        }
        let year = text[0..4].parse().map_err(|_| Error::InvalidPeriod)?;
        let month = text[5..7].parse().map_err(|_| Error::InvalidPeriod)?;
        Period::new(year, month)
    }
}

/// Reads a date written exactly `YYYY-MM-DD`.
pub fn parse_date(text: &str) -> Result<NaiveDate, Error> {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, byte)| i == 4 || i == 7 || byte.is_ascii_digit());
    if !shape {
        return Err(Error::InvalidDate);
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .filter(|date| date.year() >= 1)
        .ok_or(Error::InvalidDate)
}

/// Days from `from` through `to`, both included; never empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateRange {
    from: NaiveDate,
    to: NaiveDate,
}

impl DateRange {
    /// `from` through `to`; refused when `from` is after `to`.
    pub fn new(from: NaiveDate, to: NaiveDate) -> Result<DateRange, Error> {
        if from > to {
            return Err(Error::EmptyRange { from, to });
        }
        Ok(DateRange { from, to })
    }

    /// Reads two `YYYY-MM-DD` dates.
    pub fn parse(from: &str, to: &str) -> Result<DateRange, Error> {
        DateRange::new(parse_date(from)?, parse_date(to)?)
    }

    /// The first day of `first` through the last day of `last`, both under
    /// `start`.
    pub fn months(first: Period, last: Period, start: MonthStart) -> Result<DateRange, Error> {
        DateRange::new(first.first_day(start), last.last_day(start))
    }

    pub fn from(&self) -> NaiveDate {
        self.from
    }

    pub fn to(&self) -> NaiveDate {
        self.to
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        self.from <= date && date <= self.to
    }

    /// Every month under `start` the range touches, in order.
    pub fn periods(&self, start: MonthStart) -> impl Iterator<Item = Period> {
        Period::containing(self.from, start).through(Period::containing(self.to, start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        parse_date(text).unwrap()
    }

    fn period(text: &str) -> Period {
        text.parse().unwrap()
    }

    fn on(day: u32, weekends: Weekends) -> MonthStart {
        MonthStart::new(StartDay::day(day).unwrap(), weekends)
    }

    /// Every start day, 1 to 28 and the last, with both weekend choices.
    fn every_start() -> Vec<MonthStart> {
        (1..=StartDay::MAX_DAY)
            .map(|day| StartDay::day(day).unwrap())
            .chain([StartDay::LAST])
            .flat_map(|day| {
                [Weekends::Keep, Weekends::FridayBefore].map(|w| MonthStart::new(day, w))
            })
            .collect()
    }

    fn dates(period: Period, start: MonthStart) -> (NaiveDate, NaiveDate) {
        (period.first_day(start), period.last_day(start))
    }

    #[test]
    fn periods_read_and_write_yyyy_mm() {
        let calendar = MonthStart::CALENDAR;
        let period: Period = "2026-02".parse().unwrap();
        assert_eq!((period.year(), period.month()), (2026, 2));
        assert_eq!(period.to_string(), "2026-02");
        assert_eq!(period.key(), "2026-02");
        assert_eq!(period.first_day(calendar), date("2026-02-01"));
        assert_eq!(period.last_day(calendar), date("2026-02-28"));
        assert_eq!(
            "2024-02".parse::<Period>().unwrap().last_day(calendar),
            date("2024-02-29")
        );
        for bad in [
            "",
            "2026-2",
            "2026-13",
            "2026-00",
            "0000-01",
            "26-11",
            "2026/11",
            "2026-11-01",
            " 2026-11",
            "+202-11",
            "２０２６-11",
        ] {
            assert!(
                matches!(bad.parse::<Period>(), Err(Error::InvalidPeriod)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn periods_step_and_iterate() {
        let nov = Period::new(2026, 11).unwrap();
        assert_eq!(nov.next(), Some(Period::new(2026, 12).unwrap()));
        assert_eq!(nov.next().unwrap().next(), Period::new(2027, 1).ok());
        assert_eq!(
            Period::new(2026, 1).unwrap().previous(),
            Period::new(2025, 12).ok()
        );
        assert_eq!(Period::new(9999, 12).unwrap().next(), None);
        assert_eq!(Period::new(1, 1).unwrap().previous(), None);
        assert_eq!(Period::MIN, period("0001-01"));
        assert_eq!(Period::MAX, period("9999-12"));
        let months: Vec<String> = Period::new(2026, 11)
            .unwrap()
            .through(Period::new(2027, 2).unwrap())
            .map(|p| p.key())
            .collect();
        assert_eq!(months, ["2026-11", "2026-12", "2027-01", "2027-02"]);
        assert_eq!(nov.through(Period::new(2026, 10).unwrap()).count(), 0);
        assert_eq!(
            Period::containing(date("2026-11-30"), MonthStart::CALENDAR),
            nov
        );
    }

    #[test]
    fn start_days_are_1_to_28_or_the_last() {
        assert_eq!(StartDay::day(0), None);
        assert_eq!(StartDay::day(29), None);
        assert_eq!(StartDay::day(31), None);
        assert_eq!(StartDay::day(256 + 1), None);
        assert_eq!(StartDay::day(1), Some(StartDay::FIRST));
        assert_eq!(StartDay::day(28).unwrap().day_of_month(), Some(28));
        assert_eq!(StartDay::LAST.day_of_month(), None);
        assert!(StartDay::LAST.is_last());
        assert!(!StartDay::day(15).unwrap().names_next_month());
        assert!(StartDay::day(16).unwrap().names_next_month());
        assert!(StartDay::LAST.names_next_month());
        assert_eq!(MonthStart::default(), MonthStart::CALENDAR);
        assert_eq!(MonthStart::CALENDAR.day(), StartDay::FIRST);
        assert_eq!(MonthStart::CALENDAR.weekends(), Weekends::Keep);
    }

    #[test]
    fn months_never_overlap_leave_gaps_or_run_empty() {
        let first = period("2023-01");
        let last = period("2029-12");
        for start in every_start() {
            for month in first.through(last) {
                let range = month.range(start);
                assert!(range.from() <= range.to(), "{start:?} {month}");
                let next = month.next().unwrap();
                assert_eq!(
                    next.first_day(start),
                    range.to().succ_opt().unwrap(),
                    "{start:?} {month}"
                );
                for day in range.from().iter_days().take_while(|d| *d <= range.to()) {
                    assert_eq!(Period::containing(day, start), month, "{start:?} {day}");
                }
            }
        }
    }

    #[test]
    fn a_start_up_to_the_15th_names_its_own_month() {
        let keep = Weekends::Keep;
        assert_eq!(
            dates(period("2026-03"), on(15, keep)),
            (date("2026-03-15"), date("2026-04-14"))
        );
        assert_eq!(
            Period::containing(date("2026-03-20"), on(15, keep)),
            period("2026-03")
        );
    }

    #[test]
    fn a_start_from_the_16th_names_the_next_month() {
        let keep = Weekends::Keep;
        // 16 Mar to 15 Apr has more days in March but is April.
        assert_eq!(
            dates(period("2026-04"), on(16, keep)),
            (date("2026-03-16"), date("2026-04-15"))
        );
        assert_eq!(
            Period::containing(date("2026-03-20"), on(16, keep)),
            period("2026-04")
        );
        // The 25th: 25 Oct to 24 Nov is November.
        assert_eq!(
            dates(period("2026-11"), on(25, keep)),
            (date("2026-10-25"), date("2026-11-24"))
        );
        assert_eq!(
            Period::containing(date("2026-11-24"), on(25, keep)),
            period("2026-11")
        );
        assert_eq!(
            Period::containing(date("2026-11-25"), on(25, keep)),
            period("2026-12")
        );
        // The last day: 31 Jan to 27 Feb is February (2026 isn't leap).
        assert_eq!(
            dates(period("2026-02"), MonthStart::new(StartDay::LAST, keep)),
            (date("2026-01-31"), date("2026-02-27"))
        );
        assert_eq!(
            dates(period("2024-03"), MonthStart::new(StartDay::LAST, keep)),
            (date("2024-02-29"), date("2024-03-30"))
        );
    }

    #[test]
    fn a_weekend_start_moves_to_the_friday_without_renaming_the_month() {
        // 1 Aug 2026 is a Saturday.
        let friday = on(1, Weekends::FridayBefore);
        assert_eq!(
            dates(period("2026-08"), friday),
            (date("2026-07-31"), date("2026-08-31"))
        );
        assert_eq!(
            Period::containing(date("2026-07-31"), friday),
            period("2026-08")
        );
        assert_eq!(period("2026-07").last_day(friday), date("2026-07-30"));
        // 25 Oct 2026 is a Sunday: November starts on Friday 23 Oct, and
        // is still November.
        let friday = on(25, Weekends::FridayBefore);
        assert_eq!(period("2026-11").first_day(friday), date("2026-10-23"));
        assert_eq!(
            Period::containing(date("2026-10-23"), friday),
            period("2026-11")
        );
        // Across every start, a month's name is the same with or without
        // the move.
        for start in every_start() {
            let kept = MonthStart::new(start.day(), Weekends::Keep);
            for month in period("2023-01").through(period("2029-12")) {
                let nominal = month.first_day(kept);
                assert_eq!(Period::containing(nominal, start), month, "{start:?}");
                assert_eq!(Period::containing(nominal, kept), month, "{start:?}");
            }
        }
    }

    #[test]
    fn the_edges_never_panic_and_clamp() {
        for start in every_start() {
            for day in [
                NaiveDate::MIN,
                date("0001-01-01"),
                NaiveDate::from_ymd_opt(0, 12, 1).unwrap(),
                NaiveDate::from_ymd_opt(0, 12, 31).unwrap(),
            ] {
                let month = Period::containing(day, start);
                assert_eq!(month, Period::MIN, "{start:?} {day}");
            }
            for day in [
                NaiveDate::MAX,
                date("9999-12-31"),
                NaiveDate::from_ymd_opt(10000, 1, 1).unwrap(),
            ] {
                let month = Period::containing(day, start);
                assert_eq!(month, Period::MAX, "{start:?} {day}");
            }
            // The first and last months hold every day from 0001-01-01 up
            // to the next month, and from their first day to 9999-12-31, so
            // every date a `YYYY-MM-DD` can hold is in exactly one month.
            let first = Period::MIN.range(start);
            let last = Period::MAX.range(start);
            assert_eq!(first.from(), date("0001-01-01"), "{start:?}");
            assert_eq!(last.to(), date("9999-12-31"), "{start:?}");
            assert_eq!(
                first.to().succ_opt(),
                Some(Period::MIN.next().unwrap().first_day(start)),
                "{start:?}"
            );
            assert_eq!(
                last.from().pred_opt(),
                Some(Period::MAX.previous().unwrap().last_day(start)),
                "{start:?}"
            );
            assert!(first.from() <= first.to() && last.from() <= last.to());
            let edges = [first.from(), first.to(), last.from(), last.to()];
            for day in edges {
                let expected = if day < date("5000-01-01") {
                    Period::MIN
                } else {
                    Period::MAX
                };
                assert_eq!(Period::containing(day, start), expected, "{start:?} {day}");
            }
            for day in [date("9999-12-24"), date("9999-12-25"), date("0001-01-02")] {
                assert!(Period::containing(day, start).range(start).contains(day));
            }
        }
        assert_eq!(
            Period::MIN.first_day(on(25, Weekends::Keep)),
            date("0001-01-01")
        );
        assert_eq!(
            Period::MAX.last_day(on(10, Weekends::Keep)),
            date("9999-12-31")
        );
        // Months tile 9999 under a late start too: 25 Nov to 31 Dec.
        assert_eq!(
            Period::MAX.range(on(25, Weekends::Keep)).from(),
            date("9999-11-25")
        );
        assert_eq!(
            Period::containing(date("9999-12-31"), on(25, Weekends::Keep)),
            Period::MAX
        );
    }

    #[test]
    fn ranges_are_inclusive_and_never_empty() {
        let calendar = MonthStart::CALENDAR;
        let range = DateRange::parse("2026-10-15", "2026-12-01").unwrap();
        assert!(range.contains(date("2026-10-15")));
        assert!(range.contains(date("2026-12-01")));
        assert!(!range.contains(date("2026-12-02")));
        let months: Vec<String> = range.periods(calendar).map(|p| p.key()).collect();
        assert_eq!(months, ["2026-10", "2026-11", "2026-12"]);
        let months: Vec<String> = range
            .periods(on(25, Weekends::Keep))
            .map(|p| p.key())
            .collect();
        assert_eq!(months, ["2026-10", "2026-11", "2026-12"]);
        // With the 16th, 16 Oct opens November and 1 Dec is in December.
        let months: Vec<String> = DateRange::parse("2026-10-16", "2026-12-01")
            .unwrap()
            .periods(on(16, Weekends::Keep))
            .map(|p| p.key())
            .collect();
        assert_eq!(months, ["2026-11", "2026-12"]);
        assert!(DateRange::parse("2026-11-01", "2026-11-01").is_ok());
        assert!(matches!(
            DateRange::parse("2026-11-02", "2026-11-01"),
            Err(Error::EmptyRange { .. })
        ));
        let year = DateRange::months(
            Period::new(2026, 1).unwrap(),
            Period::new(2026, 12).unwrap(),
            calendar,
        )
        .unwrap();
        assert_eq!(
            (year.from(), year.to()),
            (date("2026-01-01"), date("2026-12-31"))
        );
        let year = DateRange::months(period("2026-01"), period("2026-12"), on(25, Weekends::Keep))
            .unwrap();
        assert_eq!(
            (year.from(), year.to()),
            (date("2025-12-25"), date("2026-12-24"))
        );
        for bad in [
            "2026-11-31",
            "2026-1-01",
            "13/11/2026",
            "2026-11-01T00:00",
            "",
        ] {
            assert!(
                matches!(parse_date(bad), Err(Error::InvalidDate)),
                "{bad:?}"
            );
        }
    }
}
