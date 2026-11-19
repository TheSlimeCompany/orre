# Coffer — Architecture: Reporting

Budgets, the forecast, and how spending is counted over the ledger.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Reporting

`coffer_core::reporting` (queries over a `Reader`), `coffer_core::budget` and
`coffer_core::schedule`. The MCP read tools and the app call these; none logs
transaction text. Amounts are `Money` in integer minor units, and totals are per currency
(`Totals`), never added across currencies: conversion waits for M4.6. The rule in force
is below; the records under "Decision records" are the history.

### The ledger and the spending rule

- **One source.** Every report reads transactions through `reporting::Ledger`, a query
  over the `ledger_line` view (V9): one line per unsplit transaction, per split line,
  and per confirmed link's part on the money in. It keeps only lines in reports: not
  excluded, not in a confirmed transfer, and for a link's part, its purchase isn't
  either; a line of zero is dropped.
- **Refunds** count on their own date, as money in under the purchase's category
  (`links-and-checks.md` → "refunds, reimbursements and shared costs", point 1).
- **Basis.** `Basis::Personal` (`personal_minor`, the default) counts the user's share:
  half of a shared cost, nothing of a purchase being claimed back. `Basis::Gross` is the
  whole line. Spending, budgets and MCP reads use the user's share; balances and net
  worth are always gross (same record, point 3).
- **Side** (`Side::of`, applied by `LedgerLine::side`) is the one rule for income,
  spending or neither: a category of kind `income` is income, `spending` is spending
  whatever the sign (money in there lowers spending), `transfer` is neither; an
  uncategorised line goes by the sign of the amount that counts, and zero is neither
  (decision record "one spending rule over the ledger", point 1).
- **Transfers.** A confirmed pair is out of reports whatever its legs' categories; a
  suggested pair counts by its own category until confirmed (`transactions.md` → "two
  kinds of transfer, and marking a suggested pair", point 1).

### Reports

- `summary::monthly_summary`: income, spending and net per month, empty months included.
- `spending::spending_by_category`: one month's spending lines per category, nested
  under their groups (the group's own, its subcategories', and a group total).
- `budget_status::budget_status`: see "Budgets" below.
- `net_worth::net_worth_history` and `account_tree::account_tree`: see "Net worth".
- `rows`, `catalog` and `register` list rows, accounts, rules and reports, and the
  Transactions table. `palette::category_palette` ranks category colours with the
  register's ranking (`inbox.md`, decision record "Inbox", point 6).

### Previews of a change

A preview tallies the ledger lines a change touches before and after it, the change made
in a rolled-back DB transaction (`change::dry_run`). `Ledger::transactions(ids)` scopes
the ledger to those transactions' lines and the confirmed link parts that net against
them; `reporting::Tally` sums lines by side, category and currency, and
`Tally::change` gives the difference. The Inbox's "If you approve" and the review
queue's "If you merge" (`checks::merge_effect`) both read it, so they move what reports
move (decision record "one spending rule over the ledger", points 2, 3 and 5). When the
change would be refused, the preview shows none (point 4).

### Net worth

- **Balances** come from `balance::balances_at`: gross, from the account's stated
  balances (opening balance, entered, from a file, or a valuation) and its transactions,
  leaving out rows excluded as `duplicate` or `superseded`. A valuation account
  (`asset`, `liability`) holds its last valuation.
- **What counts** is `net_worth::counts`, shared by `account_tree` and
  `net_worth_history`: an account with a stated balance counts, and so does a pot, from
  its transfers; any other account with none is left out until a balance is entered.
  `net_worth_history` still lists such an account, flagged `unanchored`, adding nothing
  (`AccountWorth::is_counted`) (decision record "one net worth rule").
- A liability always subtracts. `net_worth_history` leaves a closed account out of every
  month that ends after its `closed_on`; `account_tree` lists open accounts only, each at
  its latest balance, on its own last active day.

### Budgets

What is built (`coffer_core::budget`, `coffer budgets`, the MCP tool `budget_status`):

- A budget belongs to one `spending` category, a group or a subcategory
  (`Error::NotSpending` otherwise). It is standing (every month) or for one month,
  which overrides the standing one that month; at most one of each per category.
- Its group (`BudgetGroup`) is `fixed`, `flex` or `non_monthly`. The schema's CHECKs fix
  rollover by group: `fixed` never, `non_monthly` always; `flex` doesn't yet
  (`BudgetGroup::rollover`). A non-monthly budget may store an
  `opening_set_aside_minor`.
- `budget_status` reports, per budget, the user's share spent in the month in its
  category, a group's including its subcategories, with refunds lowering it. Only the
  budget's currency counts against it; other currencies are listed apart
  (`spent_other_currencies`). Remaining is budget minus spent, negative when over.
  Rollover and the set-aside are reported, not applied.

Decided but not built (M4.8, M4.9; decision record "budgets and the forecast"): rollover
both ways, derived from `ledger_line` and never stored (point 3), with a toggle for
`flex`; pace linear by day of the month (point 4); a non-monthly amount seeded once from
the category's yearly schedules, accruing 1/12 a month (point 5); the forecast (below).

### Schedules and the forecast

- **Missed** is derived in `schedule` (`OccurrenceState`): an occurrence's date plus
  `tolerance_days` has passed, nothing matched or skipped it, and the account's imports
  cover that date. Until then it is upcoming, and nothing is flagged (decision record
  "budgets and the forecast", point 2). Skips and the user's matches are stored
  (`schedule_skip`, `schedule_match`), and a series marked "Not recurring" in
  `recurring_dismissal`.
- **The forecast** (M4.8) is not built. As decided: the last known balance plus schedule
  occurrences, plus each Flex budget's remainder spread evenly over the rest of the
  month and across accounts by the last 3 months' spending, in integer minor units
  (point 1); a missed occurrence counted from tomorrow until matched or skipped (point
  2); the horizon is the account's next scheduled income, else 30 days (point 6).

## Decision records

Oldest first.

### Decision record: budgets and the forecast (2026-11-10)

The open questions from the D6.10 designs (`DESIGN.md` → "Budgets: groups, rollover,
pace and upcoming"), decided by the user on the designer's recommendation:

1. **Flex spending leaves accounts in proportion to history.** The forecast spreads each
   Flex budget's remainder evenly over the rest of the month. It splits that remainder
   across accounts by where the category was spent over the last 3 months. An account
   with no Flex spending in that window (savings, pots, Joint in the fixture) gets none.
   Later months spend their full Flex budget the same way. Every step is integer minor
   units, with remainders spread so each day's amounts sum exactly. Fixed and
   non-monthly budgets add nothing to the forecast beyond their schedules, which avoids
   counting them twice.
2. **A payment is missed only once imports cover its date.** A schedule occurrence is
   *missed* when its date plus `tolerance_days` has passed, nothing matched it, and the
   account's latest imported transaction is on or after that date. Until then it is only
   *not imported yet*, and nothing is flagged. A missed occurrence stays in the forecast,
   counted from tomorrow, until the user matches it to a transaction or skips it.
   Missed is derived, not stored. A skip is stored (`schedule_skip(schedule_id,
   occurrence_date, skipped_at)`), and so is a user's match (`schedule_match(schedule_id,
   occurrence_date, transaction_id)`). A series the user marks "Not recurring" is not suggested again.
3. **Rollover carries both ways, derived from history.** With rollover on, a month's
   carry-in is the previous month's LEFT, positive or negative, so it accumulates.
   Nothing about it is stored: it's recomputed from `ledger_line` and the budgets, so
   editing or recategorising a past transaction flows into later months. Turning
   rollover off starts the budget fresh from the next month; turning it on starts
   carrying from the current month. Fixed budgets don't offer rollover, and non-monthly
   budgets always roll over.
4. **Pace is linear by day of the month in v1.** On day *d* of an *n*-day month, pace is
   *d*/*n* of the month's amount (budget plus rollover). Pace weighted by past months'
   daily pattern can come later; it's a reporting choice, not a schema one.
5. **A non-monthly amount is seeded once, then belongs to the user.** A new non-monthly
   budget's yearly amount starts as the total of the category's active yearly
   schedules. After that it's the user's number and doesn't follow schedule changes.
   A new yearly schedule in that category asks before adding its amount (the schedule
   dialog's "Set aside" toggle). The set-aside amount accrues from the month the budget
   is created. It starts at £0 unless the user enters what's already saved, which is
   stored as an opening set-aside. The 1/12 accrual stays a default (decision record of
   2026-11-09, point 4).
6. **Without scheduled income, the horizon is 30 days.** "Lowest before payday" uses the
   account's next scheduled income as the horizon. An account with none shows "Lowest in
   the next 30 days", and the Payday figure is left out. `cash_flow_forecast` (M4.8)
   reports the same horizon and says which one it used.

### Decision record: one spending rule over the ledger (2026-11-16)

From an architecture review of `reporting`. Nothing the user sees changes except the
Inbox's "If you approve" amounts (point 3) and the review queue's "If you merge" (point
5).

1. **Whether a line counts as income, spending or neither is decided in one place:**
   `reporting::Side::of(category kind, counted amount)`, applied by
   `LedgerLine::side(basis)`. The category's kind decides; an uncategorised line goes by
   the sign of the amount that counts, and zero is neither. Summary, spending by
   category, budgets, the Inbox preview and the merge preview use it. The copies in
   `summary`, `inbox::preview` (`is_spending`, `is_income`) and `checks`
   (`spending_of`) are gone. `register::rank_groups` asks a different question (which
   groups get colours, decision record "the Transactions view", point 2) and is left
   alone.
2. **A preview of a change reads the ledger as reports do.** `Ledger::transactions(ids)`
   scopes the ledger to those transactions' lines and the confirmed link parts that net
   against them (a refund's part moves with its purchase); `Ledger::lines_in` runs it on
   the connection of a `change::dry_run`. `reporting::Tally` sums lines per side,
   category and currency, and a preview shows the change from before to after. Amounts
   are the user's share ("Spending is your share by default", M2 C.1 and C.2).
3. **The Inbox's "If you approve"** (spending changes for a categorisation or a rule,
   and the income and spending a transfer pair or refund link takes away) comes from
   one rolled-back approval made as the Inbox makes it (a rule applied to history,
   decision record "Inbox", point 2), with the touched rows' ledger read
   before and after. Before, it summed whole transaction amounts: recategorising a
   shared £100 purchase showed £100 moving where reports move £50, and a purchase being
   claimed back moved its full amount where reports move nothing. A refund link lists
   every category whose spending moves, in the refund's month, as a categorisation
   does: a split purchase's refund nets against each line's category, and a refund
   already in another spending category moves out of it.
4. **When approving would be refused, the preview shows no change:** no spending
   changes, and no income or spending amounts on a transfer or link effect.
   `Impact::blocked` or `RuleImpact::history_blocked` says why. A ledger total that
   overflows is `ApproveError::Reporting`.
5. **The merge's "If you merge" spending** is the same tally of the pair's ledger before
   and after the planned merge, so it now also counts a refund of the excluded row
   leaving reports.
6. A wide rule's per-merchant `total` stays a gross listing sum; it lists the rows, it
   doesn't report spending. `inbox::impact`'s own "moves income" test is not changed
   here.

### Decision record: one net worth rule (2026-11-19)

The accounts panel and `net_worth_history` disagreed about an account with no stated
balance (nothing entered, no balance from a file, no valuation). The panel showed "No
balance" and left it out of net worth; the history summed it from zero, added it to the
totals and only marked it `unanchored`.

1. **One rule, in both (user).** An account other than a pot with no stated balance is
   left out of net worth until a balance is entered, because its sum from zero is right
   only if it started empty. A pot is counted from its transfers even without one
   ("Decision record: sub-accounts"). A valuation account is still left out before its
   first valuation. `net_worth::counts` is the one function that says which, used by
   `account_tree` and `net_worth_history`, so they can't drift.
2. **What changed in `net_worth_history`.** Such an account is still listed for each
   month, flagged `unanchored`, so nothing is silently dropped, but its `counted` is
   zero and it adds nothing to the month's totals (`AccountWorth::is_counted` says
   which). A pot stays counted from its transfers, as before, and is still flagged
   `unanchored`. The MCP tool's description says so.
3. **Rejected: count it from zero and mark it everywhere.** An account opened
   mid-history with money already in it would understate or overstate net worth without
   the user seeing a number to doubt.
