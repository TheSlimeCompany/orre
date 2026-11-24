# M4.1 — Monthly overview

Status: in progress (2026-11-24), branch `lead/m4-1-monthly-overview`.

Source: `docs/ROADMAP.md` → M4.1 ("Monthly overview: income vs spending vs net (bar/line),
category breakdown, MoM change"). The look is `docs/DESIGN.md` → "Overview" (Finance
screens) and "Data visualisation" (ranked bar list, income vs spending, chart chrome),
drawn in Paper on Overview (`22V-0`). DESIGN.md is the spec for looks and copy; this file
is how to build it and the decisions the build makes.

## Ordering decision (user, 2026-11-24)

The roadmap put M4.11 (the month start) before M4.1. The user chose to build M4.1 first on
calendar months. So every month the Overview reads goes through `reporting::Period` and
its methods (`containing`, `range`, `next`, `previous`, `through`). Nothing here computes a
month boundary with `with_day(1)` or similar, so M4.11 changes the cut in one place.

## Problem

The Overview tab is the first thing in the title bar and still says it isn't built. The
dashboard should answer "how am I doing this month?" at a glance: what I spent, earned
and kept; how that compares with last month; where the money went; and the last year's
shape.

## What to build

### Core: `orre_core::reporting::overview` (read model, no GPUI)

One call reads everything the screen shows, for one selected month, all accounts or one
account (a pot is its own account, as on Transactions), as of `today`:

- **Months**: the chart's 12 months, each with income, spending and net in the display
  currency (same classification as `monthly_summary`: `Basis::Personal`, `Side`).
  Window: the 12 months ending at `today`'s month, unless the selected month is before
  that window, in which case the 12 months ending at the selected month.
- **Selected and previous month figures**: Spent, Income, Net for the selected month and
  the month before (the previous month may be outside the window).
- **Changes vs last month**: Spent and Income as a whole-number percentage of the
  previous month, rounded half away from zero, computed in integer arithmetic (no floats
  in core). Previous month zero → no percentage (the app says "None in Sep"). Net change
  is the signed money difference (a percentage of a net that can change sign misleads).
- **Spending by category** for the selected month: one entry per top-level group. A group
  with a palette slot (`reporting::palette::category_palette` for `today`) is named and
  keeps that slot; every other group, the `other` group included, adds into one "Other"
  entry; uncategorised spending is its own entry. Named entries ordered by amount, largest
  first; then Other, then Uncategorised, whatever their size. Each entry has its amount
  and a whole-number share of the month's spending. A group whose refunds outweigh its
  purchases (amount ≤ 0) is still listed (its money is in the total), ordered by amount.
- **Recurring · next 30 days**: detected series of money out (`recurring::find_with_reader`,
  not lapsed) for the account filter, in the display currency, due from `today` through
  `today + 30 days`, earliest first, with merchant, cadence, next date, typical amount
  (signed), category and slot; the total shown unsigned. A series whose expected date
  passed with no import showing it rolls forward along its cadence when at most 30 days
  overdue. One row per series. Looks forward from `today` whatever month is selected. No
  price-rise flag yet (the detector doesn't report one; M4.3).
- **Display currency**: amounts in different currencies are never added. The Overview
  shows one currency: the one with the most counted lines (income and spending) in the
  12-month window, ties broken by currency code. When any other currency has counted
  lines in the window, the result says which (`other_currencies`), and the app says they
  aren't shown. No lines at all: the first account's currency, or GBP with no accounts.
- **Range for the switcher**: the earliest month with a counted line (for the account
  filter), so the app can stop the switcher there; the latest is `today`'s month.
- **Empty**: whether there are any transactions at all (for the empty state).

Tests (in core): window rules (selected inside / before the window), percentage rounding
and zero previous month, net change sign, category grouping (named, Other, Uncategorised
ordering; ≤ 0 group), account filter, the display-currency choice and the other-currency
flag, recurring window bounds and lapsed series left out, that months agree with
`monthly_summary` for all accounts.

MCP is unchanged. No migration.

### App: the Overview screen

- `View::Overview` becomes built: the shell shows the Overview screen with the accounts
  panel on the left (selection filters it, as Transactions; the shell already shares the
  panel's selection), no inspector; the canvas takes the full width.
- Layout and copy per DESIGN.md → "Overview": month switcher, figures row (Spent display
  figure, Income and Net figures, captions with change vs last month: "+8% vs Sep",
  "Same as Sep" when it rounds to 0, "−16% vs Sep", "None in Sep"; Net: "+£212.40 vs
  Sep"), compact agent card at the right end of the figures row while suggestions are
  pending ("Review" opens the Inbox), income and spending chart (title, caption "Last 12
  months" or "12 months to Mar 2025" when the window doesn't end at today's month,
  legend), lower row with "Spending by category · October" (ranked bar list) and
  "Recurring · Next 30 days" (list rows, total on the right).
- Chart: bars, net line with halo, gridlines, axis, hover tooltip, selected month overlay
  and label; clicking a month selects it. The month switcher's chevrons step months,
  disabled at the earliest month with data and at today's month.
- Other currencies note: a caption under the figures, `text-muted`: "Amounts in EUR
  aren't included" (listing the codes).
- Empty state (no transactions): the Empty-state component, "No transactions yet" with
  the kbd row "⌘I Import a statement", as Transactions.
- Loads on a background thread like the other views, and reloads on the shell's live
  updates (ledger topic) and after the app's own writes.
- Test-only/dev `describe()` and fixture like the other views; headless tests with
  `#[gpui_kit::test]`; the snapshot example renders `overview` (and a hover/empty state)
  for comparison with Paper `22V-0`; the fixture follows DESIGN.md's Overview fixture
  data.

## Decisions to record (docs/architecture/reporting.md, new decision record)

1. Built before M4.11 on calendar months, through `Period` only (user).
2. Category colours stay ranked at read time through `reporting::palette`, shared by
   every screen, so the Overview, Transactions and Inbox agree. Storing them on
   `category` (planned for M4.1 in the Transactions view record, point 2) is deferred:
   it needs a migration and a rule for when a stored colour changes. That's for the user
   to decide.
3. The display currency rule and the other-currencies note.
4. The chart's window rule.
5. Net change as money, Spent/Income change as a percentage.
6. Recurring list without the price-rise flag until M4.3.

## Acceptance criteria

1. `orre_core::reporting::overview` exists with the read model above and its tests;
   it computes no floats and no month boundaries outside `Period`.
2. `View::Overview` is built: the shell renders the Overview screen for the selected
   account or all accounts; it reloads after changes.
3. The screen has the month switcher, the three figures with the changes vs last month,
   the 12-month income/spending bar chart with the net line, hover and selection, and
   the category ranked bar list with category colours matching Transactions.
4. The empty state and the other-currencies note appear when they should.
5. Headless view tests cover switching months, clicking a month in the chart, the
   account filter, the empty state; the snapshot example renders `overview`.
6. The decision record is in `docs/architecture/reporting.md` and listed in
   `docs/ARCHITECTURE.md` → "Contents"; `docs/CODEMAP.md` lists the new modules;
   ROADMAP M4.1 is ticked (noting what's deferred).
7. `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and
   `cargo test` pass; invariants test passes.
