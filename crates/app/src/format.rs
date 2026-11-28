//! How the app writes money and dates, and reads them back from fields
//! (`docs/DESIGN.md` → "Money").
//!
//! Money is the en-GB form for now: `£1,284.10`, a true minus (U+2212) and
//! an explicit plus where the sign is the point, the currency's own minor
//! units deciding the decimals (no decimals for JPY). Locale-aware
//! separators wait for settings. A currency without a symbol here is
//! written with its code: `CHF 12.00`.
//!
//! Dates are the short forms the designs use: `9 Nov` in lists for this
//! year and `2 Oct 25` for an earlier one, and `9 Nov 2026` in a date field.
//! A date field reads that form back, and also `9 Nov` (this year), `9 Nov
//! 26` and `9/11/26` (this century), ISO `2026-11-09` and `9/11/2026`.
//! A month cut by the user's month start is `23 Oct – 24 Nov`, with
//! weekdays in Settings' preview (M4.11).

use chrono::{Datelike as _, NaiveDate};
use orre_core::money::{Currency, DecimalMark, Money, ParseAmountError};
use orre_core::reporting::period::DateRange;

/// How a formatted amount shows its sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// `−` for money out, `+` for money in: single transactions, and totals
    /// that can go either way.
    Always,
    /// `−` only below zero: balances, and totals whose label gives the
    /// direction.
    Negative,
}

/// The symbol written before an amount in `currency`, if Orre knows one.
fn symbol(currency: Currency) -> Option<&'static str> {
    match currency.code() {
        "GBP" => Some("£"),
        "EUR" => Some("€"),
        "USD" => Some("$"),
        _ => None,
    }
}

/// `amount` for display: `£1,284.10`, `−£24.10`, `+€34.00`, `CHF 12.00`.
pub fn money(amount: Money, sign: Sign) -> String {
    let currency = amount.currency();
    let (prefix, number) = signed_number(amount, sign);
    match symbol(currency) {
        Some(symbol) => format!("{prefix}{symbol}{number}"),
        None => format!("{prefix}{} {number}", currency.code()),
    }
}

/// `amount` as an amount field holds it, with no currency (the field
/// names it): `−1,284.10`, `34.00`. [`parse_amount`] reads it back.
pub fn field_amount(amount: Money) -> String {
    let (prefix, number) = signed_number(amount, Sign::Negative);
    format!("{prefix}{number}")
}

/// The sign `sign` asks for, and the unsigned number grouped in thousands
/// with the currency's decimals.
fn signed_number(amount: Money, sign: Sign) -> (&'static str, String) {
    let currency = amount.currency();
    let digits = amount.to_decimal_string();
    let unsigned = digits.trim_start_matches('-');
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let mut grouped = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    // `to_decimal_string` drops trailing zeros' worth of nothing: pad the
    // fraction to the currency's minor units.
    let places = usize::from(currency.minor_units());
    let fraction = format!("{fraction:0<places$}");
    let number = if places == 0 {
        grouped
    } else {
        format!("{grouped}.{fraction}")
    };
    let prefix = if amount.is_negative() {
        "\u{2212}"
    } else if sign == Sign::Always && amount.is_positive() {
        "+"
    } else {
        ""
    };
    (prefix, number)
}

/// The text before an amount field's value: the currency's symbol, or its
/// code.
pub fn currency_prefix(currency: Currency) -> String {
    symbol(currency).map_or_else(|| currency.code().to_owned(), str::to_owned)
}

/// A date in a list: `9 Nov` in `today`'s year, `2 Oct 25` in another.
pub fn list_date(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        date.format("%-d %b").to_string()
    } else {
        date.format("%-d %b %y").to_string()
    }
}

/// A month's dates where the user's month start makes them worth saying:
/// `23 Oct – 24 Nov` (M4.11; Paper `KEQ-0`). No year: a month is never
/// more than a few weeks from the month it's named after.
pub fn day_range(range: DateRange) -> String {
    format!(
        "{} – {}",
        range.from().format("%-d %b"),
        range.to().format("%-d %b")
    )
}

/// [`day_range`] with weekdays: `Fri 23 Oct – Tue 24 Nov` (Settings' month
/// preview, Paper `K57-0`).
pub fn weekday_range(range: DateRange) -> String {
    format!(
        "{} – {}",
        range.from().format("%a %-d %b"),
        range.to().format("%a %-d %b")
    )
}

/// `range`'s dates ([`day_range`]) when they aren't the calendar months
/// they're named after, which `calendar` covers; `None` when they are, so a
/// month on the 1st says nothing it doesn't need to.
pub fn dates_if_moved(range: Option<DateRange>, calendar: Option<DateRange>) -> Option<String> {
    let range = range?;
    (Some(range) != calendar).then(|| day_range(range))
}

/// A date in a date field: `9 Nov 2026`.
pub fn field_date(date: NaiveDate) -> String {
    date.format("%-d %b %Y").to_string()
}

/// Reads a date field: `9 Nov 2026`, `9 November 2026`, `9 Nov` (in
/// `today`'s year), `2026-11-09` or `9/11/2026` (day first). `None` when it
/// isn't a date.
pub fn parse_date(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    // chrono's `%Y` takes any number of digits, so `9 Nov 26` would be the
    // year 26: four-digit years only there, and two digits are this
    // century's (`%y`), as lists write an earlier year (`2 Oct 25`).
    for format in ["%d %b %Y", "%d %B %Y", "%Y-%m-%d", "%d/%m/%Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&text, format)
            && date.year() >= 1000
        {
            return Some(date);
        }
    }
    for format in ["%d %b %y", "%d %B %y", "%d/%m/%y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&text, format) {
            return Some(date.with_year(2000 + date.year() % 100).unwrap_or(date));
        }
    }
    let with_year = format!("{text} {}", today.year());
    ["%d %b %Y", "%d %B %Y"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(&with_year, format).ok())
}

/// Reads an amount field in `currency`: `1,284.10`, `-24.10`, `−24.10`. A
/// symbol typed before it is ignored. Never rounds: more decimals than the
/// currency has is an error.
pub fn parse_amount(text: &str, currency: Currency) -> Result<Money, ParseAmountError> {
    let mut text = text.trim();
    let prefix = currency_prefix(currency);
    for sign in ["-", "\u{2212}", "+"] {
        if let Some(rest) = text.strip_prefix(sign)
            && let Some(rest) = rest.trim_start().strip_prefix(prefix.as_str())
        {
            return Money::parse_decimal(&format!("{sign}{rest}"), currency, DecimalMark::Point);
        }
    }
    if let Some(rest) = text.strip_prefix(prefix.as_str()) {
        text = rest;
    }
    Money::parse_decimal(text, currency, DecimalMark::Point)
}

/// A form's message for an amount beyond Orre's cap ("Amounts can't exceed
/// 10,000,000,000.00 GBP."), or `None` for any other error, which the form
/// words itself.
pub fn beyond_the_cap(error: &ParseAmountError) -> Option<String> {
    matches!(error, ParseAmountError::OutOfRange { .. })
        .then(|| crate::import::session::sentence(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn writes_a_months_dates_and_only_when_they_moved() {
        let range = DateRange::new(day(2026, 10, 23), day(2026, 11, 24)).unwrap();
        assert_eq!(day_range(range), "23 Oct – 24 Nov");
        assert_eq!(weekday_range(range), "Fri 23 Oct – Tue 24 Nov");
        let november = DateRange::new(day(2026, 11, 1), day(2026, 11, 30)).ok();
        assert_eq!(dates_if_moved(november, november), None);
        assert_eq!(
            dates_if_moved(Some(range), november).as_deref(),
            Some("23 Oct – 24 Nov")
        );
        assert_eq!(dates_if_moved(None, None), None);
    }

    #[test]
    fn writes_money_with_a_true_minus_separators_and_the_currency() {
        assert_eq!(money(gbp(-645), Sign::Always), "\u{2212}£6.45");
        assert_eq!(money(gbp(3400), Sign::Always), "+£34.00");
        assert_eq!(money(gbp(128_410), Sign::Negative), "£1,284.10");
        assert_eq!(money(gbp(-2_410), Sign::Negative), "\u{2212}£24.10");
        assert_eq!(money(gbp(123_456_789), Sign::Negative), "£1,234,567.89");
        assert_eq!(money(gbp(0), Sign::Always), "£0.00");
        assert_eq!(money(gbp(50), Sign::Negative), "£0.50");
        assert_eq!(
            money(Money::new(1450, Currency::EUR), Sign::Negative),
            "€14.50"
        );
        let yen = Currency::new("JPY").unwrap();
        assert_eq!(
            money(Money::new(-1500, yen), Sign::Negative),
            "\u{2212}JPY 1,500"
        );
        let franc = Currency::new("CHF").unwrap();
        assert_eq!(money(Money::new(1200, franc), Sign::Negative), "CHF 12.00");
    }

    #[test]
    fn a_field_amount_has_no_currency_and_reads_back() {
        for minor in [-128_410, 2_300, 0, -5] {
            let text = field_amount(gbp(minor));
            assert_eq!(parse_amount(&text, Currency::GBP), Ok(gbp(minor)), "{text}");
        }
        assert_eq!(field_amount(gbp(-128_410)), "\u{2212}1,284.10");
        assert_eq!(field_amount(gbp(2_300)), "23.00");
    }

    #[test]
    fn writes_list_and_field_dates() {
        let today = day(2026, 11, 14);
        assert_eq!(list_date(day(2026, 11, 9), today), "9 Nov");
        assert_eq!(list_date(day(2025, 10, 2), today), "2 Oct 25");
        assert_eq!(field_date(day(2026, 11, 9)), "9 Nov 2026");
    }

    #[test]
    fn reads_the_date_forms_a_field_accepts() {
        let today = day(2026, 11, 14);
        for text in [
            "9 Nov 2026",
            " 9  nov 2026 ",
            "09 November 2026",
            "9 Nov",
            "2026-11-09",
            "9/11/2026",
        ] {
            assert_eq!(parse_date(text, today), Some(day(2026, 11, 9)), "{text}");
        }
        // Two-digit years, as lists write them, are this century's.
        for text in ["9 Nov 26", "9/11/26", "09 November 26"] {
            assert_eq!(parse_date(text, today), Some(day(2026, 11, 9)), "{text}");
        }
        assert_eq!(parse_date("2 Oct 25", today), Some(day(2025, 10, 2)));
        assert_eq!(
            parse_date(&list_date(day(2025, 10, 2), today), today),
            Some(day(2025, 10, 2))
        );
        for text in [
            "",
            "31 Feb 2026",
            "Nov 9",
            "tomorrow",
            "9/13/2026",
            "9 Nov 026",
            "0026-11-09",
        ] {
            assert_eq!(parse_date(text, today), None, "{text}");
        }
        // What a field shows reads back.
        assert_eq!(
            parse_date(&field_date(day(2024, 2, 29)), today),
            Some(day(2024, 2, 29))
        );
    }

    #[test]
    fn reads_amounts_exactly() {
        let read = |text| parse_amount(text, Currency::GBP).map(|m| m.minor());
        assert_eq!(read("1,284.10"), Ok(128_410));
        assert_eq!(read("-24.10"), Ok(-2_410));
        assert_eq!(read("\u{2212}24.1"), Ok(-2_410));
        assert_eq!(read("£12"), Ok(1_200));
        assert_eq!(read("-£12"), Ok(-1_200));
        assert!(read("12.345").is_err());
        assert!(read("").is_err());
        assert!(read("twelve").is_err());
        // What the app writes reads back.
        assert_eq!(read(&money(gbp(-2_410), Sign::Negative)), Ok(-2_410));
    }
}
