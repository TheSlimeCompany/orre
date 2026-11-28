# Roadmap

Orre is a local-first personal finance dashboard: import bank statements,
see where your money goes, and talk to your data through any MCP-capable AI client
(Claude Code, Claude Desktop, Codex). Design decisions live in
[`ARCHITECTURE.md`](ARCHITECTURE.md), and the long-term direction in
[`VISION.md`](../VISION.md); this file tracks what gets built, in what order.

Status legend: ✅ done · 🚧 in progress · ⬜ not started

Tasks are numbered `<milestone>.<n>` (e.g. `M1.3`, `D2.4`; `L` for Later, `R` for the
architecture review, `U` for the UI review, `LU` for live updates, `FT` for full-text search) so they can be referenced. Numbers are stable: append new tasks rather than renumbering.

Each milestone ends with something usable, and has explicit exit criteria.

Tasks M1.20+, M2.10+, M3.9+, M4.8+, M5.8 and L.11+ come from the feature research in
[`research/feature-ideas.md`](research/feature-ideas.md) (2026-11-09). Ideas rejected
there because they conflict with the invariants (bank sync, cloud, multi-user, MCP writes
beyond the inbox, AI in import) are deliberately not on this roadmap.

---

## M0 — Design ✅

- ✅ M0.1 Design interview; decisions recorded in `docs/ARCHITECTURE.md`
- ✅ M0.2 Dev environment (devenv/Nix, Rust stable, GPUI runtime libs)
- ✅ M0.3 GPUI hello-world template builds (`cargo build`)

---

## Design track — runs alongside M1–M2 ✅

Prove the visual system in Paper and in GPUI before the GUI milestones depend on it.
Specs live in [`DESIGN.md`](DESIGN.md). Orre's Paper file ("Orre", created in D1.1) is
separate from "Festive comet", which is a reference file only; D1–D6 are designed in
Orre's file. D1 and D2 are independent and can run in parallel. D2 needs M1.1 (the
Cargo workspace) so the theme lands in `crates/app`. D3 and D4 need D1; D5 needs D2 and D3.

**D1 — Orre Paper file: tokens and components**

- ✅ D1.1 Create the Orre file in Paper (via the Paper MCP); record its URL in `DESIGN.md` →
  "Source of truth"
- ✅ D1.2 Create every token in `DESIGN.md`: the reference set plus the "Proposed" colour,
  font-weight and opacity tokens, with weights at 400/500/600/700. Also names the
  type-scale, spacing, radius and state values the theme already used (96 tokens in all)
- ✅ D1.3 Foundations artboard: colour swatches, type scale, spacing and radii, built from tokens
  only (no literal values). Exception: 1px swatch ring and outline (Paper has no shadow or
  border-width tokens)
- ✅ D1.4 Components artboard: the components `DESIGN.md` uses as specified (buttons in every
  state, list row, transaction row with mono amounts, field, select, toggle, segmented
  control, badge, chip, agent card, …), every node bound to tokens. Added 24 size tokens
  (control heights, geometry, column widths) to make that possible, plus `danger-hover`
  and `danger-pressed` for the solid danger button. Exceptions: border, ring and shadow
  geometry, icon paths, data-driven bar lengths
- ✅ D1.5 Export `design/tokens.json` / `design/tokens.css` from the new file; drop the "Proposed"
  marks; update the D2 drift test, which pins the reference file's 450/550/650 weights.
  122 tokens (hash `572f3e11`); the drift test maps every one, following `var()` aliases.
  "Proposed" now marks only designs not yet drawn in Paper (finance screens, charts)

**D2 — GPUI theme + token showcase (fidelity spike, part 1)**

- ✅ D2.1 Theme module in `crates/app` holding every `DESIGN.md` token as typed values (no
  strings, no literals outside the module)
- ✅ D2.2 Test that checks the Paper-sourced values against `design/tokens.json`, so drift fails
  the build. Both live in `crates/app/src/theme/tokens.rs`
- ✅ D2.3 Bundle fonts: Inter 400/500/600/700 and Geist Mono 400/600 (static instances, SIL OFL,
  licence files included). Files in `assets/fonts/`, loaded and set on the theme by
  `crates/app/src/theme/fonts.rs`
- ✅ D2.4 Map `gpui-component`'s theme onto the tokens; list the components that need a custom
  build instead (likely: toggle, segmented control, agent card).
  `crates/app/src/theme/component.rs`
  maps every stock colour field to a token (a sentinel test fails on any unmapped or
  non-token field), plus radius, rem base, shadows and motion. The component list (custom:
  toggle, segmented control, view tabs, badge, tooltip, progress bar, agent card and the
  other compositions) is in `DESIGN.md` → "Implementing in GPUI"
- ✅ D2.5 Showcase window (`orre --showcase`): colour swatches, type scale, buttons in every
  state, list row (default/hover/selected/focus), transaction rows with mono amounts,
  agent card. Built (`crates/app/src/showcase.rs`, components in `crates/app/src/ui/`), plus a
  stock components
  board (input, select, popover, menu, dialog, title bar, underline tabs, stock focus).
  Visually compared against the Paper artboards; no discrepancies
- ✅ D2.6 Record findings in `DESIGN.md` → "Implementing in GPUI": alpha colours, 1px hairlines,
  shadows, an outside focus ring, tabular mono figures, animation easing. Recorded under
  "Rendering findings (D2.6)", from the GPUI renderer source and the font files. Two defects
  fixed with tests: shadow blur was passed to GPUI as CSS blur (GPUI takes σ = blur/2), and the
  outside focus ring's gap was uneven at 1x. The three open questions are closed (decided
  while the user was away; review): disabled buttons with a fill paint the group's composite
  colours over their surface, so they match Paper (3.6:1 for the neutral primary; no new
  tokens, no Paper change); reduced motion is 0ms for every animation, opacity fades
  included ("Motion" changed; no code needed); the agent card's ⇄ is `Glyph::Transfer`

**D3 — Transactions screen in Paper**

- ✅ D3.1 1440×900, realistic data: long merchant names, pot transfers, a foreign-currency
  payment, a refund, uncategorised and AI-suggested rows, a multi-selection. Two artboards
  in the Orre file: Transactions (`18E-0`, an AI-suggested row selected, its inspector
  with approve/reject) and Transactions · multi-select (`1NP-0`, three rows, bulk-action
  bar, selection summary). Composed from Components clones, all token-bound; one new token,
  `--container-search-field` (re-exported, hash `86e4aeb1`; `size::SEARCH_FIELD`). Fixed a
  2px lane offset on suggested categories (sparkle gap 8 → 6px) in Paper and in
  `ui::transaction_row`, with a layout test. Spec in `DESIGN.md` → "Transactions"

**D4 — Overview screen in Paper**

- ✅ D4.1 Display figures, income-vs-spending chart, category bar list, agent card. Artboard
Overview (`22V-0`, 1440×900): the Transactions shell with the Overview tab active and no
inspector; month switcher (October 2026), Spent/Income/Net figures with deltas, a compact
agent card at the end of the figures row (moved there from the lower right column, which
couldn't hold it and the recurring list), the 12-month chart with October selected and a
July hover tooltip, a ranked bar list of spending by category (six palette colours,
Other, Uncategorised), and recurring payments with a `warning` price rise. All token-bound,
no new tokens. Charts decided: custom, on the `gpui_component::plot` primitives. Spec in
`DESIGN.md` → "Overview", "Data visualisation"
- ✅ D4.2 Adjust tokens if the screens show problems. No token changes: the six chart
colours stay distinct as 3px bars and markers, `warning` and `danger` read in context, and
neutral amounts hold. The screen changed rules instead: a line over bars gets a 1.5px
`canvas` halo (the `text` line on `text-secondary` bars is 1.8:1), bars are a fixed 16px
rather than "group gap 40% of bar width", and totals whose label gives the direction are
unsigned (`DESIGN.md` → "Money", "Chart chrome")

**D5 — Transactions in GPUI (fidelity spike, part 2)**

- ✅ D5.1 Build the D3 screen in GPUI with fixture data; compare it against the Paper screenshot
  at 1x and 2x. `crates/app/src/transactions.rs` (title bar, left panel, toolbar, virtualised
  day-grouped table, inspector for one or several transactions, bulk-action bar, status
  bar), now the window `orre` opens; `crates/app/examples/snapshot.rs` renders it headlessly.
  Colour exact once Paper's Display P3 export is colour-managed, layout exact at 2x,
  baselines within half a pixel. Letter spacing now applied (`ui::tracked`). Four GPUI
  pitfalls found and fixed. Findings in `DESIGN.md` → "Implementing in GPUI" → "Fidelity
  findings (D5)"
- ✅ D5.2 Decision: GPUI can deliver the design → continue; otherwise reconsider the UI toolkit
  before M3. **Decided (user, 2026-11-09): continue with GPUI.** Colour and layout match
  Paper, and the gaps found were in Orre's code, not GPUI. Still to check on a real display
  (hover, scrolling, drag and resize, macOS, long lists) as the app grows

**D6 — Remaining screens**

- ✅ D6.1 Import, Inbox, Trends, Budgets, Net worth, Reviews, accounts panel. Six
  1440×900 artboards in the Orre file, built from the Transactions and Overview shells:
  Inbox (`564-0`), Import · map columns (`5ZQ-0`), Budgets (`6OE-0`), Trends (`781-0`),
  Net worth (`7LH-0`), Reviews (`7YM-0`); plus two states boards: Import · steps (`6J4-0`:
  File and Review, and the reconciled / mismatch / locked-period results) and Accounts
  panel · states (`8D8-0`: row states, the account inspector, the new-account dialog). All
  token-bound. Two new tokens, `--container-dialog-wide` (760px, Import) and
  `--container-reading` (560px, a review's measure), re-exported (hash `6a126241`) with
  `size::DIALOG_WIDE` and `size::READING_MEASURE`. Spec in `DESIGN.md` → "Finance
  screens". Decided while drawing (review):
  - Inbox groups are table sections with lane headers, not one agent card per group; row
    approve/reject show on hover and selection only;
  - a parse failure in the Import preview is not imported but recorded with the import
    as a flag, and a balance mismatch can be imported only as "Import and flag the
    mismatch";
  - Budgets is a table with inline bars so the budgets' ends line up;
  - the Net worth area runs to a £0 baseline;
  - an account with transactions can be closed (hidden, history kept), not deleted.
- ✅ D6.2 Orre logo / wordmark. The strongbox: lid, seam, body and a hasp, filled
  `molten`, on a 16-unit grid so the 16px title-bar size is pixel-aligned. Logo artboard
  (`8MT-0`): construction, title-bar and display lockups, mono variant, app icon (molten
  tile, chrome mark); the three directions explored stay on Logo · concepts (`8JS-0`).
  Every screen's title bar now shows the mark before the wordmark (6px gap), and so does
  the GPUI title bar (`Glyph::Mark`). Exports in `assets/brand/`. Spec in `DESIGN.md` →
  "Logo". On 2026-11-20 the strongbox was replaced by the halo, a ring split by a lens
  on the diagonal (same grid, colours and lockups; the icon tile draws it at 6.571×,
  and the exports in `assets/brand/` are now the VectorCraft set as exported); it stays on
  Logo · strongbox (retired) (`J2Q-0`)
- ✅ D6.3 Transactions additions: split editor (lines, auto-filled remainder, total
  locked), tags, user note beside the read-only imported note, excluded state and
  filter, manual-transaction form, "Create a rule from this edit?" (M3.9, M3.10, M3.13).
  Four 1440×900 artboards duplicated from Transactions:
  - split and tags (`2Q3-0`);
  - excluded, with the Excluded filter chip on (`3FE-0`);
  - rule from an edit (`3WT-0`);
  - manual (cash account) (`4D7-0`).

  Plus a states board (`4X9-0`): unbalanced and balanced split states, the exclude and
  new-transaction dialogs, the undo notification after a bulk change, and the new row
  kinds. All token-bound. One new token, `--container-dialog` (440px), re-exported (hash
  `b442a048`) with `size::DIALOG`. Spec in `DESIGN.md` → "Transactions additions". Its
  three data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision
  record: Transactions additions"):
  - exclusion reason key, note and date, where only duplicates leave the balance;
  - `created_at` on every transaction;
  - the rule prompt needs at least one earlier match.
- ✅ D6.4 Review queues: possible duplicates (side-by-side pair, why flagged,
  merge/dismiss) and reconciliation (assertion results per account, failing window,
  lock/unlock) (M3.12). Two 1440×900 artboards from the Inbox shell, Review · possible
  duplicates (`8SE-0`) and Review · balance checks (`98K-0`), plus a states board,
  Review queues · states (`9MQ-0`: lock and unlock dialogs, the merge undo bar, a merge
  into a locked period, the empty queue). All token-bound, no new tokens. Spec in
  `DESIGN.md` → "Review queues". Decided while drawing (review):
  - both queues sit under a new CHECKS section in the Inbox's left panel, neutral (not
    moldavite), and outside the Inbox tab's suggestion count;
  - the selected pair opens in place side by side; shared values are muted so the
    differences stand out;
  - the inspector cross-links the two queues: a pair names the check it fixes, and a
    failing check names the pair that explains it.

  Its five data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision
  record: review queues"):
  - the oldest balance is the starting point, not a check;
  - a period locks only through a passing check, with no partial unlock;
  - a merge keeps the row with a source ID, then an imported row, then the oldest;
  - the near-duplicate window is 3 days (a default);
  - a dismissed pair stays dismissed for good.
- ✅ D6.5 Refund, reimbursement and settlement linking; outstanding-claims and owed-to-me
  lists (M3.11). Three 1440×900 artboards: Transactions · refund link (`9WS-0`),
  Transactions · shared cost (`BAT-0`) and Review · owed to you (`CCF-0`, Inbox shell);
  plus a states board, Links and shares · states (`EB0-0`: the link dialog, the
  inspectors after linking, claim back partly repaid, the new row kinds, the empty
  queue). All token-bound, no new tokens. Spec in `DESIGN.md` → "Refunds,
  reimbursements and shared costs". Decided while drawing (review):
  - every transaction gets a Links section, and every outflow a Share section ("Just
    me" / "Shared" / "Claim back"), so links and shares are always in the same place;
  - Orre's own link suggestions are neutral cards in the inspector and strips on Owed
    to you, not Inbox items; the AI's `refund_link` suggestions stay in the Inbox (D6.6);
  - the table keeps the bank's amounts and says how a row counts in the category cell
    ("Shopping · refund", "· shared", "· to claim");
  - Owed to you is a third CHECKS row in the Inbox, with expense claims and people in
    one list.

  Its five questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  refunds, reimbursements and shared costs"):
  - a refund counts on its own date, in the purchase's category;
  - link suggestions are deterministic (90 days for refunds, 60 days to the penny for
    reimbursements) and live in the inspector, not the Inbox;
  - spending, budgets and MCP reads use your share; balances and net worth stay gross;
  - an overpayment is capped, and the rest stays unlinked;
  - a link needs both sides in one currency in v1.
- ✅ D6.6 Inbox impact preview and the new suggestion kinds (merchant alias, refund link)
  in the agent card; designed with the D6.1 Inbox (M3.14, M2.14). Two 1440×900 artboards
  from the Inbox with the D6.4 CHECKS panel, Inbox · rule impact (`ABK-0`) and Inbox ·
  merchant name impact (`DL8-0`), plus a states board, Inbox · D6.6 states (`EWZ-0`: the
  refund link, transfer pair and categorisation inspectors, Orre's own suggestion card,
  a wide rule, the undo bar). All token-bound, no new tokens. Spec in `DESIGN.md` →
  "Inbox impact preview". Decided while drawing (review):
  - the selected suggestion opens in place and lists the transactions it would change on
    the group's own lanes, with rows it leaves alone marked "Stays, you set it"; the
    inspector sums it up under "If you approve", like D6.4's "If you merge";
  - groups are ordered by reach (rules, merchant names, transfer pairs, refund links,
    categorisations), and a rule's lane counts the rows it changes, not the ones it
    matches;
  - a wide rule (more than 5 merchants) gets a warning, a by-merchant summary, and is left
    out of Approve all;
  - Orre's own `rule` suggestions use the same card, neutral, outside the moldavite
    badge.

  Its six questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  Inbox impact preview"):
  - approving an agent's rule never changes a category you set;
  - a rule is wide past 5 merchants or when it moves income (a default);
  - Approve all always skips wide rules;
  - a rule's approval settles the categorisations it covers;
  - merchant names don't change which rules match;
  - the impact is computed when shown.
- ✅ D6.7 Rules editor: rule actions, apply-to-history preview, merchant alias editor
  (M3.13). Four 1440×900 artboards from the Review · possible duplicates shell: Rules
  (`B1J-0`), Rules · apply to past transactions (`EFT-0`), Rules · merchant names
  (`G2O-0`) and Rules · change history (`GZT-0`), plus a states board, Rules · states
  (`HFH-0`: delete and undo dialogs, the undo bar, no rules yet, several actions, the
  add-action menu, a pattern that can't be used, a rule that's never reached). All
  token-bound, no new tokens. Spec in `DESIGN.md` → "Rules". Decided while drawing
  (review):
  - Rules is an eighth view tab after Reviews (decided by the user, 2026-11-10), with
    Rules, Merchant names and Change history in its left panel;
  - the rule editor is the inspector (When, Then, Past transactions), and rules are
    reordered by dragging rows; a rule that always loses to one above is flagged "Never
    reached";
  - applying a rule to history previews it on the canvas in two sections, "Will change"
    and "Category you set", which are left out unless "Change these too" is on;
  - merchant names apply at once, without a preview or undo batch, because the display
    name is derived;
  - a menu pattern (for "+ Add action") is drawn for the first time.

  Its eight questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  rules"):
  - rules win field by field;
  - a new rule goes first;
  - a rule overwrites a category you set only with "Change these too";
  - rule edits don't re-run on history; only Apply does;
  - merchant names resolve Is, then the longest Starts with, then Pattern;
  - undo skips rows changed since, and counts them;
  - deleting a rule leaves its values and clears `rule_id`;
  - locked periods guard balances only.

  The tab isn't yet on the artboards drawn before D6.7; the GPUI title bar has it since
  M3.1
- ✅ D6.8 Settings: AI access (clients, access levels, kill switch, access log), backups
  (list, create, restore), export dialog (M3.15, M3.16). Three 1440×900 artboards from
  the Inbox shell: Settings · AI clients (`AQ2-0`), Settings · access log (`DC6-0`) and
  Settings · backups (`F63-0`); plus a states board, Settings · D6.8 states (`GQA-0`: AI
  access off, the enable dialog, restore with checks passed and failed, the export
  dialog, the "Back up now" notification). All token-bound, no new tokens. Spec in
  `DESIGN.md` → "Settings". Decided while drawing (review):
  - Settings isn't an eighth view tab: a settings icon button in the title bar (⌘,)
    opens it, with AI ACCESS and DATA sections in the left panel;
  - a client name that isn't enabled is refused and listed under "Asked to connect",
    and enabling it starts at Aggregate, the least shared level;
  - the kill switch is a toggle in the AI clients toolbar; off is neutral (a banner, the
    clients dimmed, "AI access off" in the status bar), not a warning;
  - the access log shows the client's purpose as quoted, untrusted text, refused calls
    as neutral rows, and free-text arguments as their length only;
  - restoring is a neutral primary action, because a backup of the current state is
    taken first; a backup that fails a check can't be restored.

  Its six questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  Settings, AI access and backups"):
  - the kill switch is read on every call; `mcp_access_log` gains `outcome` (new
    migration);
  - unknown clients are recorded as `requested` and refused;
  - a lower level applies at once, with `tools/list_changed`;
  - argument redaction keeps the structure, and free text as type and length;
  - the access log is kept for 12 months (a default);
  - backup metadata lives in the file name; older schemas migrate, newer are refused.
- ✅ D6.9 Accounts: cash accounts and manual asset/liability accounts with valuations;
  designed with the D6.1 accounts panel (M3.10, M3.17). One 1440×900 artboard,
  Accounts · asset (valuations) (`BSI-0`: House selected, step-line value chart,
  valuations table, account inspector), and a states board, Accounts · D6.9 states
  (`EPF-0`: liability and cash inspectors, New account set to Asset, the Kind menu, the
  cash count, a liability balance, closing an asset). All token-bound, no new tokens.
  Spec in `DESIGN.md` → "Accounts you keep yourself". Decided while drawing (review):
  - an ASSETS AND LIABILITIES section in the left panel; Stocks ISA moves there, cash
    stays under ACCOUNTS;
  - a liability's negative balance isn't `danger` ("Money" now says which negatives
    are);
  - with a valuation account selected, its valuations replace the transaction table,
    and ⌘N adds a valuation;
  - a step line with no area fill, so its y axis needn't start at £0;
  - a short cash count offers to add the difference as a transaction, and never does
    it on its own.

  Its five data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` →
  "Decision record: accounts you keep yourself"):
  - a valuation can carry a private `note` (new migration);
  - valuation accounts hold valuations only;
  - `account.closed_on` for every kind, plus a £0 valuation when closing one;
  - only an account without transactions can be deleted;
  - a cash count is a checked `manual` balance, and Orre never posts the difference.
- ✅ D6.10 Upcoming payments and balance forecast; budget rollover, groups and pace line,
  designed with the D6.1 Budgets screen (M4.8, M4.9). Two 1440×900 artboards from the
  D6.1 Budgets screen: Budgets · groups, rollover and pace (`CXU-0`) and Budgets ·
  upcoming (`GD5-0`). Plus a states board, Budgets and upcoming · states (`HNB-0`): the
  non-monthly inspector, rollover rows, the schedule dialog, the "Review 2 found"
  popover, a missed payment, an overdrawn forecast, and the empty Upcoming. All
  token-bound, no new tokens. Spec in `DESIGN.md` → "Budgets: groups, rollover, pace and
  upcoming". Decided while drawing (review):
  - Upcoming is the second half of the Budgets tab (Budgets | Upcoming), not a ninth tab;
  - groups are table sections (Fixed, Non-monthly, Flex), and rollover is a signed,
    muted column, so LEFT = Budget + Rollover − Spent;
  - Left to spend counts Flex only, and non-monthly money shows as Set aside;
  - the pace line is one dashed line across the Flex bars; bars aren't coloured by pace;
  - a missed payment stays in the forecast until it's matched or skipped.

  Its six data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision
  record: budgets and the forecast"):
  - Flex spending leaves accounts in proportion to the last 3 months;
  - a payment is missed only once imports cover its date;
  - rollover carries both ways and is derived from history;
  - pace is linear by day;
  - a non-monthly amount is seeded once from yearly schedules, then belongs to the user;
  - without scheduled income, the horizon is 30 days.
- ✅ D6.11 Safe report rendering (markdown subset, inert links, `ai_authored` marker) and
  the withheld-text indicator, designed with the D6.1 Reviews screen (M4.10, M2.13).
  Reviews · safe rendering (`C6D-0`, 1440×900, "Subscription check" with a table, an
  inert link and a blocked image) and a states board, Reviews · D6.11 states (`E43-0`:
  the subset as source and rendering, each case shown as text, the withheld-text
  markers). All token-bound, no new tokens. Spec in `DESIGN.md` → "Safe report
  rendering". Decided while drawing:
  - links render as their text followed by the address in mono `text-muted`, never
    clickable; a link whose text looks like an address shows both;
  - images become a dashed "Image not loaded" block that shows the address; HTML shows
    as written; anything outside the subset keeps its markdown characters;
  - "AI-written" agent badge in the byline, and a "Read back · `ai_authored`" row;
  - the inspector counts what was shown as text, and "What it read" counts withheld
    descriptions (`warning` shield), linking to a new "Withheld from AI" filter.

  Its four questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  safe report rendering and withheld text"):
  - emphasis renders as weight 500, with no italic face;
  - `withheld_count` on `mcp_access_log` (new migration), no transaction IDs logged;
  - the marker reads `[withheld: N characters that read like instructions]`;
  - `list_reports` strips hidden characters and sets `ai_authored`, without withholding.
- ✅ D6.12 Month start, designed for M4.11 (L.15): Settings · month start (`K57-0`),
  1440×900, from the D6.8 Settings shell: a CALENDAR section with Month start; Starts on
  (a select, the 1st to the 28th, then "Last day of the month" under a hairline);
  Weekends (Leave it / Use the Friday before, with the bank-holiday caption); a preview
  of last, this and next month with names, dates with weekdays and day counts; the
  helper; and an inspector listing what follows the rule and what keeps its own dates.
  Plus a states board, Month start · states (`KEQ-0`): the default, the open day menu,
  the last day with the Friday before, the Date chip's month presets with their dates,
  and the Overview's month switcher with the month's dates. No new tokens. Spec in
  `DESIGN.md` → "Month start". Decided by the user (2026-11-25), over the artboards:
  - the Date chip's label shows the dates too when the preset's range differs ("This
    month · 23 Oct – 24 Nov");
  - no "Pick a month…" in the Date chip: the Overview's switcher picks months;
  - the setting applies at once, with no Save or confirm;
  - the panel section is "Calendar", not "Reports".

**Exit criteria:** tokens live in Paper and in the GPUI theme with a drift test;
Transactions and Overview are designed; the GPUI build of Transactions matches the Paper
screen closely enough that the designer signs off.

**Signed off** by the user on 2026-11-10. Every D item is done and every design question
is decided in `ARCHITECTURE.md`. The choices marked "review" in this track and in
`DESIGN.md` (decided while drawing, or while the user was away) are accepted as drawn.

---

## M1 — Core + import (headless) ✅

Import real statements into a local database from the command line.

- ✅ M1.1 Convert to a Cargo workspace: `crates/core`, `crates/import`, `crates/mcp`, `crates/app`.
  Packages `orre-core`, `orre-import`, `orre-mcp` (stubs for now) and `orre-app`
  (lib `orre_app` plus the `orre` binary); `assets/` and `design/` stay at the root.
  Versions, edition and licence come from the root `[workspace.package]` and
  `[workspace.dependencies]`; each crate declares the dependencies of its role
- ✅ M1.2 Add `LICENSE-MIT`, `LICENSE-APACHE`; set `license = "MIT OR Apache-2.0"`
- ✅ M1.3 SQLite setup: platform data dir, WAL mode, 0600 permissions, versioned migrations.
  `orre_core::db::Database::open` creates the data dir (0700) and `orre.db` (0600,
  as are its `-wal`/`-shm`), sets WAL, `synchronous = FULL`, foreign keys,
  `trusted_schema = OFF`, a busy timeout and `BEGIN IMMEDIATE`, then applies the
  embedded `crates/core/migrations/` in one transaction. An existing file is checked
  before anything changes it (a SQLite database, not a newer schema, unedited migration
  history); a refused file is left byte- and mode-for-mode as it was. A newer schema or
  an edited, unknown or skipped migration is refused. `Database::open_existing` (for
  `orre mcp`) never creates or migrates: a missing database, a pending migration, a
  newer schema or a changed history is refused. `ORRE_DATA_DIR` overrides the data dir
- ✅ M1.4 Schema: accounts, import batches, transactions, balance snapshots, categories,
  rules, transfer groups, budgets, FX rates, suggestions, reports.
  `crates/core/migrations/V1__initial_schema.sql`: `STRICT` tables, checked dates and
  currency codes, enum `CHECK`s, no `ON DELETE` actions, indexed foreign keys, triggers
  for read-only imported source fields and two-level categories, partial unique indexes
  for dedupe, FX rates as `rate_e9` fixed point (`ARCHITECTURE.md` → "Data model").
  Constraints carry the invariants: dedupe keys are source ID first (fingerprint only
  for rows without one), inserts never replace a stored row, and triggers enforce
  account currency, pot structure, valuation-only `asset`/`liability` accounts and
  read-only import batches. Triggers also enforce category keys against their parent
  (keys and parents read-only), keep a pot that holds transactions or balances under its
  parent, fix an account's currency once a rule with amount bounds names it (and keep such a rule
  in that currency), and enforce
  undo (an `undone` batch has no rows, takes none and never returns to `committed`); a
  partial unique index allows one entered balance per account and day. Tables for later
  items (splits, tags, links, duplicates, change batches, schedules, MCP clients) come
  with those items
- ✅ M1.5 Money type: integer minor units + ISO 4217 currency (no floats anywhere).
  `orre_core::money`: `Currency` only builds from a known ISO 4217 code and carries its
  minor units (hand-written table, frozen by tests, withdrawn codes since 2014 kept);
  `Money { minor: i64, currency }` with checked arithmetic (mixing currencies or
  overflowing is an error); `Money::parse_decimal` reads bank text exactly (`.`/`,`
  decimal mark, grouped thousands, `-`/`−`/`+`/parentheses) and refuses anything it
  would have to round; `to_decimal_string` is the plain export form. `Currency` is
  `serde` and SQLite `TEXT`. Locale display formatting stays in the app; FX is M4.6
- ✅ M1.6 `Importer` trait (detect → parse → normalize).
  `orre_import`: `Detect` (name, format, `Detection` from `No` to `Source`), `Importer`
  (`parse` into its own `Parsed` type, `normalize` into a `Statement`), `AnyImporter` for
  `&dyn` lists with `import`, and `detect` to rank candidates (the caller chooses).
  `orre_core::statement`: `Statement` (format, preset, currency, rows in file order,
  stated balances), `StatementLine`, flagged rows as `RowProblem` at a `SourceLocation`
  (never dropped, no transaction text), `normalize_description`. Exercised with a toy
  importer in tests; CSV and OFX are M1.7 and M1.10 (`ARCHITECTURE.md` → "The
  `Importer` trait")
- ✅ M1.7 CSV importer: encoding detection, delimiter, header/skip rows, date formats
  (`DD/MM` vs `MM/DD`), separate time column, decimal style (`1,234.56` vs `1.234,56`),
  signed amount or debit/credit columns, merchant vs raw description, source ID,
  source category/type, original (local) amount + currency, optional balance column.
  `orre_import::csv`: `CsvImporter` over a validated `CsvMapping` (the shape M1.8's
  TOML will deserialize into). BOM or stated encoding, else UTF-8 only (never guessed);
  delimiter stated or detected (ties refused); `skip_rows` and optional header, columns by
  name or position; `strftime` dates that must round-trip, with `DateFormat::candidates`
  to surface `DD/MM`/`MM/DD` ambiguity; per-row currency column; signed or debit/credit
  amounts (`Unsigned`/`Signed`). Rows with the wrong column count are flagged
  (`RowProblem::ColumnCount`), as are unreadable fields. Synthetic fixtures in
  `fixtures/csv/` (`ARCHITECTURE.md` → "The CSV importer")
- ✅ M1.8 TOML preset format + loader (`presets/<region>/<bank>.toml`), including
  source-category → category mapping.
  `orre_import::preset`: `Preset::from_toml` reads a `CsvMapping` plus ID, name, region,
  `ignore` and `[category_map]`; unknown keys are refused and field values are checked
  where written (errors point at the line); TOML column positions count from 1.
  `crates/import/build.rs` compiles every `presets/**/*.toml` in (`Presets::bundled`);
  `Presets::from_files` checks `<region>/<bank>.toml` placement, region, an ID of
  `<region>-<bank>[-…]` and unique IDs. `Preset::category_for` maps exact source
  categories to `orre_core::category::CategoryKey`; `Preset::unread_columns` lists header
  columns neither read nor ignored. A test requires every bundled preset to import its
  fixture in `fixtures/presets/`. Contributor reference: `presets/README.md`
  (`ARCHITECTURE.md` → "Presets"). `[sub_account_transfer]` came with M1.17
- ✅ M1.9 First preset: `presets/uk/monzo.toml` with a redacted fixture (built from a real export).
  The preset (`uk-monzo`, signed `Amount`, `multiline_fields = true`, Monzo categories
  mapped to default keys) and a synthetic fixture with one row per known quirk, with
  tests in `crates/import/tests/preset_uk_monzo.rs`. Checked by structure against a real
  export (2026-11-13): it has no pot IDs, so pots are keyed by name, and pot transfers
  sit in "Transfers", so `[sub_account_transfer] category` gives them
  `transfer.savings`. The real file imports with nothing flagged and re-imports with
  nothing new (`ARCHITECTURE.md` → "Presets"; "Decision record: sub-accounts", points
  8–9)
- ✅ M1.10 OFX/QFX importer (SGML 1.x and XML 2.x), `FITID` IDs, `LEDGERBAL` balances.
  `orre_import::ofx`: `OfxImporter` over Orre's own tolerant SGML/XML reader (`ofx-kit`
  dropped: it fails whole files on one bad row and guesses encodings). Bank and credit
  card statements; several statements in one file are refused unless one is chosen
  (`OfxImporter::for_statement`); repeated `FITID`s and `CORRECTACTION` rows are flagged.
  Synthetic fixtures in `fixtures/ofx/` (`ARCHITECTURE.md` → "The OFX importer")
- ✅ M1.11 Deduplication: source ID when present (OFX `FITID`, CSV ID column);
  fingerprint with occurrence index otherwise.
  `orre_core::import`: a source ID is matched by ID only (a conflicting date or amount
  is flagged); otherwise a readable `v1|date|amount|currency|occurrence|description`
  fingerprint (`import::fingerprint`)
- ✅ M1.12 Balance reconciliation where the file has balances; failed imports flagged, not
  committed silently. Manual balance anchor for accounts whose exports have none.
  `orre_core::balance::check`; running balances checked row by row, file balances
  stored, mismatches need explicit confirmation; `account::add_balance` is the manual
  anchor
- ✅ M1.13 Import batches with undo. `import::preview` → `commit` (one DB transaction,
  re-planned and refused if stale) → `undo` / `undo_preview`; unreadable rows are kept as
  `import_flag` rows (`V3__import_flags.sql`) (`ARCHITECTURE.md` → "Dedupe,
  reconciliation and the commit")
- ✅ M1.14 Default two-level category set with stable keys. 15 groups, 50 subcategories,
  each `spending`, `income` or `transfer`, seeded once by `V2__default_categories.sql`;
  `orre_core::category::{DEFAULT_CATEGORIES, CategoryTree}`; bundled presets may map
  only to default keys (tested) (`ARCHITECTURE.md` → "Categories")
- ✅ M1.15 Rules engine (contains / regex / amount range / account), priority ordering.
  Validated types, store (`create`, `create_at`, `list`, `move_to`, `delete`) and
  `RuleSet` matching field by field in priority order; `set_category` only (the other
  actions are M1.24). Runs in `import::commit` after the preset's category map, which it
  overrides (`ARCHITECTURE.md` → "Decision record: rules engine, transfer detection and
  the CLI")
- ✅ M1.16 Transfer-pair detection (opposite amounts, own accounts, N-day window).
  `orre_core::transfer`: 3-day window, closest pairs first, `suggested`
  `transfer_group` rows, run by `import::commit`; confirm/reject come with the inbox and
  Transactions screen
- ✅ M1.17 Sub-accounts (Monzo Pots, Starling Spaces, Revolut Vaults): preset-driven detection,
  auto-created pot accounts, mirrored pot-side entries auto-paired as transfers.
  `[sub_account_transfer]` in presets (Monzo's pots in `presets/uk/monzo.toml` and its
  fixture); `orre_core::sub_account` plans the pots in the preview and mirrors the
  rows in `import::commit`, before rules and transfer detection, as `confirmed`
  transfer groups; pots are found by key (`account.sub_account_key`,
  `V4__sub_account_key.sql`) and stay when their import is undone. Starling Spaces and
  Revolut Vaults need their presets (`ARCHITECTURE.md` → "Decision record:
  sub-accounts")
- ✅ M1.18 CLI: `orre import <file> [--account] [--preset] [--dry-run]`,
  `orre accounts`, `orre undo-import <batch>`. clap, in `orre_app::cli`; also
  `--statement N` for multi-statement OFX, `--yes` to accept flags, a mismatch or an
  undo, and `orre accounts add`
- ✅ M1.19 Redacted fixtures + tests for every importer path. Synthetic fixtures for
  newest-first, headerless, tab-separated and US-dated CSVs and a follow-on OFX
  statement, test-only mappings in `fixtures/csv/mappings/`, and end-to-end tests in
  `crates/import/tests/{csv,ofx,commit}_paths.rs` and `crates/app/tests/cli_paths.rs`.
  The bug it found (a file balance inside a locked period was recorded, making the batch
  impossible to undo) is fixed (`ARCHITECTURE.md` → "Decision record: importer paths
  and locked balances")
- ✅ M1.20 Split transactions: `transaction_split` lines that sum exactly to the parent
  (checked in core); reports and budgets read the `ledger_line` view, so a split purchase
  counts in each of its categories. `orre_core::split`, `V5__transaction_split.sql`;
  a split parent has no category, amount or currency change while split, transfers
  aren't split, and undo and manual deletes remove the lines (`ARCHITECTURE.md` → "Decision record: splits, tags and notes, manual rows, merchant
  names")
- ✅ M1.21 Tags (several per transaction, private tags never leave the app) and a
  user-editable note beside the read-only imported one. `orre_core::tag` and
  `orre_core::note`, `V6__tags.sql`; names unique ignoring case, shared with rules'
  `add_tag`; undo and manual deletes remove the links (`ARCHITECTURE.md` → "Decision record: splits, tags and notes, manual rows, merchant
  names")
- ✅ M1.22 Manual transactions and cash accounts (`origin = manual`). Imported source fields
  stay read-only: fix a bad row by undoing its batch, or exclude it with a recorded reason.
  `orre_core::manual` (any account that holds transactions; locks and `closed_on`
  respected) and `orre_core::exclusion`; no migration (`ARCHITECTURE.md` → "Decision record: splits, tags and notes, manual rows, merchant
  names")
- ✅ M1.23 Merchant clean-up: deterministic cleaning of bank text (card references,
  "CONTACTLESS", processor prefixes) into `merchant_display`, plus a merchant alias table.
  Raw fields are never changed. `orre_core::merchant`, `V7__merchant_alias.sql` (with
  `merchant_display_source`); runs in `import::commit` before rules, aliases apply at
  once to derived names, and rules match the cleaned merchant (`ARCHITECTURE.md` → "Decision record: splits, tags and notes, manual rows, merchant
  names")
- ✅ M1.24 Rule actions beyond setting a category: set merchant, add tag, set note, exclude,
  split template, my share. Each transaction records the rule that categorised it. Applying
  a rule to past transactions shows a preview and commits as an undoable change batch.
  `orre_core::change` (V8, `V8__change_batch.sql`): a `Recorder` that writes and records
  field changes, `dry_run` previews, `undo` restoring only fields that still hold the
  batch's value. `rule::apply_to_batch` applies every action at import, field by field
  (tags included); shares and split lines from basis points exactly (half away from zero;
  largest remainder); `rule::preview_apply` → `commit_apply` as one `rule_apply` batch,
  skipping split rows, changing user-set values only on included rows, refusing a stale
  preview and exclusions under a lock (`ARCHITECTURE.md` → "Decision record: M1 wave 4")
- ✅ M1.25 Refunds, reimbursements and shared costs: `transaction_link`
  (`refund`/`reimbursement`/`settlement`). A refund nets against the purchase's category
  instead of counting as income; a reimbursable purchase stays outstanding until linked;
  `my_share_minor` on transactions and split lines feeds an owed-to-me balance.
  `orre_core::link`, `V9__transaction_link.sql` (which redefines `ledger_line` with link
  rows, `in_reports` and `personal_minor`); deterministic suggestions at import (90-day
  refunds, 60-day reimbursement sets); undo and manual deletes clear links
- ✅ M1.26 Possible-duplicates queue: same account and amount within ±N days but different
  text (typically overlapping CSV and OFX exports). Merging excludes the duplicate, never
  deletes it, and can be undone. `orre_core::duplicate`, `V10__duplicate_candidate.sql`;
  detected in the import commit and by `scan`; merges are `duplicate_merge` change
  batches that keep the best-sourced row and carry user-set values over
- ✅ M1.27 Dated balance assertions: every balance snapshot is checked against the computed
  balance, and a failure names the window between the last passing assertion and the
  failing one. Per-account lock date (`reconciled_through`): anything that would change
  transactions before it is flagged and needs an explicit unlock.
  `orre_core::reconciliation` (`account_checks`, `lock`, `unlock`); entered balances,
  imports, manual edits, merges, rule applies and undos inside the lock are refused; a
  rule's exclusion counts whatever its reason (user decision, 2026-11-12)
- ✅ M1.28 Local backups: rotating `VACUUM INTO` snapshots (0600) before every import commit
  and migration; `orre backup`, `orre restore <file>` (integrity and schema-version
  checks before swapping). `orre_core::backup`: `<data dir>/backups`, the last 10
  automatic backups plus one per earlier month, manual ones kept; `import::commit` and
  `Database::open` back up first; `orre backup [--list]`, `orre restore <file>
  [--yes]`; running MCP servers refuse calls while a restore swaps the files (M2)
  (`ARCHITECTURE.md` → "Decision record: local backups")
- ✅ M1.29 `orre export --format csv|json`: documented schema, filters by date, account,
  category and tag; split lines keep their parent ID. `orre_core::export` (schema
  version 1, columns in its module docs) reading `ledger_line`; `--output` files 0600,
  `--force` to replace; notes and private tags only with `--include-notes` /
  `--include-private-tags` (user decision, 2026-11-12), share labels never

`crates/import/tests/m1_exit.rs` walks the exit criteria below end to end on synthetic
data, and a real Monzo export imports and re-imports cleanly (M1.9).

**Exit criteria:** your own CSV/OFX exports import without duplicates on
re-import, reconcile where balances exist, and get categorized by rules. Splits, tags,
manual transactions and refund links work through the core API with tests, and a backup
exists from before every import.

---

## M2 — MCP server ✅

Analyse real data from Claude Code or Codex before any GUI exists. Everything below is
built and tested on synthetic data, and was checked on 2026-11-13 with Claude Code against
a real Monzo export: totals, pseudonymous and standard access, suggestions approved from
`orre inbox`, the access log, and recurring detection. Decisions: `ARCHITECTURE.md` → "Decision record: M2: MCP
server (2026-11-13)"; setup: [`mcp-setup.md`](mcp-setup.md).

- ✅ M2.1 `orre mcp` stdio server (`rmcp`), read-only DB connection for queries.
  `crates/mcp`: one wrapper per call (gate → call → log), the tool registry by level,
  `V11__mcp.sql`; the database is opened per call (never created or migrated), so a
  restore is never blocked and calls are refused while it swaps the files
- ✅ M2.2 Read tools: `list_accounts`, `monthly_summary`, `spending_by_category`,
  `search_transactions`, `merchant_history`, `recurring_payments`, `budget_status`,
  `net_worth_history`, `uncategorized_transactions`, `list_categories`,
  `list_rules`, `list_reports`. Queries in `orre_core::reporting` (a `Ledger` over
  `ledger_line`, per-currency `Totals`, `Limit` 50/200 with `truncated`), budgets in
  `orre_core::budget` with `orre budgets list|set|remove`; account names only from
  `pseudonymous` up, text search only at `standard` (user decisions, 2026-11-13)
- ✅ M2.3 Inbox-only writes: `propose_categorizations`, `propose_rule`,
  `propose_transfer_pair`, `save_report`. `orre_core::inbox`: typed, validated payloads,
  one pending copy per payload, 200 pending suggestions and 500 reports per client
  (the user frees room with `orre inbox delete-report`), `V13__inbox.sql` (decision columns and triggers); approve and reject with an impact
  preview
- ✅ M2.4 `monthly_review` prompt: its steps name only the tools at the client's level and
  end with `save_report` (`crates/mcp/src/prompts.rs`)
- ✅ M2.5 PII guard: no account numbers or personal details in any tool output (tested).
  `assert_no_pii` on every response, merchant names checked at `pseudonymous`, a row
  whose bank text is only a person's name hidden below `standard` (names at `standard`
  accepted, user decision 2026-11-13), account details withheld from merchants and
  descriptions at every level
- ✅ M2.6 Recurring-payment / subscription detection in core. `orre_core::recurring`:
  deterministic chains per cadence (weekly to yearly), fixed and variable amounts, noise
  rejection, "Not recurring" dismissals (`recurring_dismissal`, V12)
- ✅ M2.7 CLI inbox until the GUI exists: `orre inbox [approve|reject]`. Also `list`,
  `show` (with the impact preview) and the saved reports (`reports`, `report`,
  `delete-report` by ID or by client, all or nothing, `--yes` to delete); approving
  several IDs is all or nothing; AI-written text is shown stripped and labelled unverified
- ✅ M2.8 Setup docs for Claude Code, Claude Desktop, and Codex
  ([`mcp-setup.md`](mcp-setup.md): configuration, levels, the kill switch, the log, a
  profile with Orre as the only data source)
- ✅ M2.9 MCP tool tests against fixture databases. One test file per tool, plus
  `crates/mcp/tests/matrix.rs` (every tool × level: registration, `refused_level`, no
  PII, one log row per call, inbox-only writes; a new tool fails it until listed) and
  `crates/mcp/tests/m2_exit.rs`
- ✅ M2.10 Schedules: confirm a detected recurring series (M2.6) as a schedule with an
  expected date, amount and tolerance; imports fulfil occurrences; missed or late payments
  are flagged. `orre_core::schedule`, `V12__schedule.sql` (`start_date`, matches and
  skips; user decision 2026-11-13); matched in the import commit, cleared by undo;
  `orre schedules detect|list|show|confirm|skip|unskip|match|unmatch|pause|resume|end|
  delete|dismiss|undismiss`; lock dates apply to occurrence dates
- ✅ M2.11 MCP access control: MCP is off until a client is enabled (CLI, and the app's
  Settings since M3.15). Each
  client has an access level: `aggregate` (totals only), `pseudonymous` (stable keyed-hash
  merchant tokens) or `standard`. Tools above a client's level are never registered. This
  guards against over-sharing; it is not a security boundary against local processes.
  `orre ai-access list|enable|level|revoke|ignore|off|on|log`; the kill switch and level
  read on every call, `tools/list_changed` on a change; unknown names recorded as
  `requested`
- ✅ M2.12 AI access log: every tool call records the client, tool, redacted arguments,
  an optional `purpose`, row count, result hash and access level, never the returned data.
  Arguments redacted by each tool's schema, SHA-256 of canonical JSON, 12 months' retention.
  Calls refused while the database can't be opened (a restore, pending migrations) wait in
  a pending file next to it and are moved in at their own times (`access_log::pending`,
  `V14__access_log_pending.sql`)
- ✅ M2.13 Untrusted-text envelope: `merchant_display` by default; raw text only as a
  length-capped, labelled `description_untrusted`, with control, zero-width and bidi
  characters stripped and URLs made inert. Text matching injection patterns is withheld,
  and the app shows how much was withheld. Setup docs (M2.8) recommend a client profile
  without web or fetch tools. `orre_core::envelope` (`sanitize`, `strip_invisible`,
  `reads_like_instructions`, pseudonyms); descriptions hidden below `standard`
- ✅ M2.14 MCP over the new model: reads use ledger lines (splits), tags and refund links;
  new suggestion kinds `merchant_alias` and `refund_link`. The app files `rule`
  suggestions itself after repeated identical manual recategorisations (source `orre`).
  `orre_core::inbox::own`, fired by `orre categorize <id>... <category>` until the
  GUI's inline edit (M3)

**Exit criteria:** ask "where did my money go last month?" in Claude Code and get
an answer backed by visible tool calls; approve AI-suggested categories from the inbox.
Every tool call appears in the access log.

---

## M3 — GUI foundation ✅

- ✅ M3.1 App shell: sidebar navigation, window state, built on the D2 theme. Every screen in
  M3–M4 follows its Paper design from the design track; no stock component styling.
  `crates/app/src/shell.rs` (`ShellView`), the window `orre` opens. Navigation follows
  Paper, not the sidebar this item first named: the eight view tabs in the title bar
  (`gpui-base` tabs: role, selected state) and the Settings button (`Glyph::Settings`,
  active icon-button look; clicked again, it goes back to the previous view (user)),
  ⌘1–⌘8 and ⌘,; the left panel belongs to the view (the accounts
  panel on the five money views). Window state (bounds, maximised or full screen, the open
  view) is saved in the `setting` table (`setting::WindowState`, key `app.window`) after
  a 500ms pause and on close, and restored on launch, checked against the displays and
  the minimum size (user, 2026-11-14), so the GUI now opens the database. Views not built
  yet show the shell and the Empty state component (`ui::empty_state`, Paper `13L-0`)
  (user). Transactions keeps its selection across switches. Snapshots (`rules`,
  `settings`) checked against Paper `B8Y-0`, `B08-0` and `13L-0` at 2x: horizontal edges
  exact; the title bar's 28px controls sit 1 device pixel higher than Paper's, as before
  M3.1 (the half-pixel centring in "Fidelity findings (D5)"). Decisions:
  `ARCHITECTURE.md` → "Decision record: the app shell and window state"
- ✅ M3.2 Accounts management (create, edit, opening balance; pots shown under their parent).
  `crates/app/src/accounts/`: the accounts panel reads the database (pots nested under
  their parent, latest balance, `danger` when overdrawn, the flagged dot for a failed
  check, "No balance"), and is a Tab stop with the arrow keys; selecting an account shows
  the account inspector on Transactions (Name, Bank, Kind, Currency locked once it holds
  money, Preset; Balances with "Add balance…"; Pots; Imports); New account (Name, Bank,
  Kind, Currency, optional opening Balance with its date) and Add balance (drawn for M3.2
  on `8D8-0`, `I7C-0`, approved by the user). Core: `account::update`, `usage`,
  `create_with_balance`, `entered_balances`, `reporting::account_tree`; V15 adds
  `account.preset_id` (user); `orre accounts add --preset` and a PRESET column. Orre's
  dialog and select are built on the unstyled `gpui-base` roots (`ui::dialog`,
  `ui::menu`). Undrawn states composed from existing specs (user; `DESIGN.md` →
  "Accounts panel" → "Built in M3.2"). "Close account…" moved to M3.17 (user). Snapshots
  (`account`, `account-kind`, `new-account`, `add-balance`) checked by eye against
  `8D8-0` and `FHZ-0`. Decisions: `ARCHITECTURE.md` → "Decision record: accounts
  management"
- ✅ M3.3 Transactions view: virtualized `DataTable`, filter/sort/search, inline category edit.
  The table is the D5 virtualised GPUI `list` (not the stock `DataTable`, which can't
  interleave day headers), now over the database: core's `reporting::register` reads
  every transaction once (one row per transaction, splits and transfer partners
  inside), and `Register::select` filters, groups by day and sorts in memory, so search
  never reads the database. Filtered by the accounts panel (the Account column hides),
  the Account, Category (a group includes its subcategories) and Date chips,
  Uncategorised, and search over merchant and raw description; sorting reorders rows
  within each day (user). Category edits (Enter or a double-click in place, the
  inspector's select, the bulk-action bar) go through `inbox::own::recategorize`, one
  undoable change batch, with the undo bar after a change to several rows. Category
  colours ranked at read time over 12 months (user; stored in M4.1). Undrawn states
  composed from existing specs (user; `DESIGN.md` → "Transactions" → "Built in M3.3").
  Snapshots (`transactions`, `transactions-multi`, `-category-menu`, `-date-menu`,
  `-sorted`, `-no-match`) checked by eye against `18E-0` and `1NP-0`. Decisions:
  `ARCHITECTURE.md` → "Decision record: the Transactions view"
- ✅ M3.4 Import flow: pick file → detect/choose preset → column-mapping screen with live
  preview → dedupe/reconcile summary → commit.
  `crates/app/src/import/`: the Import dialog (⌘I, the Transactions toolbar's Import, or
  a file dropped on the window), File (drop zone, Choose file…, RECENT IMPORTS) → Map
  (Preset and Account, COLUMNS, a FILE section for a mapping of your own, the live
  preview of the first 10 rows with flagged rows and their notes) → Review (New, Already
  imported, Flagged, AFTER IMPORT, the reconciliation and locked-period blocks) → the
  commit on a background thread, then the notification with Undo; the accounts panel
  and Transactions read the database again. `import::session` chooses as `orre
  import` does (OFX; a preset by its header; the account's preset; else suggested
  columns, never a guessed date order) and `import::store` (`ImportStore`) opens the
  database per call. Import crate: `csv::MappingDraft` (suggest, build), `FileLayout`,
  `CsvFile::read`, `date_formats`, `CsvImporter::problem_text`. Core:
  `import::rehearse` (the commit's steps, rolled back, for AFTER IMPORT) and
  `CommitSummary::uncategorized`. No migration. Undrawn states composed from existing
  specs (user; `DESIGN.md` → "Import" → "Built in M3.4"); Unlock… inert until M3.12
  (user). Snapshots (`import-*`) checked by eye against `5ZQ-0` and `6J4-0`; UI
  integration tests drive ⌘I, a dropped file, Review, Import and Undo against a scratch
  database. Decisions: `ARCHITECTURE.md` → "Decision record: the import flow"
- ✅ M3.5 Save/export a mapping as a preset. "Save as preset…" on the Map step: name and
  region give the ID (`preset::slug`), `Preset::to_toml` writes TOML that
  `Preset::from_toml` reads back to the same preset (tested on every bundled preset),
  saved in `<data dir>/presets/<region>/<bank>.toml` (user; `preset::save_user_preset`,
  0600) and set as the account's preset; "Save a copy…" writes it where you choose
  (user). Saved presets load next to the bundled ones in the app, the account
  inspector and the CLI (`preset::load_user_presets`); a broken one is reported and
  skipped. `presets/README.md` → "Presets saved from the app"
- ✅ M3.6 Inbox: bulk approve/reject AI suggestions. `crates/app/src/inbox/`: the panel
  (INBOX Pending/Approved/Rejected, KIND, SOURCE as filters), the groups (one per kind and
  source, widest reach first) with Approve all (n) / Reject all, all or none
  (`inbox::approve_all`, new `inbox::reject_all`), row ✓/✕ on hover and selection, A / R /
  ↑↓, the agent card and Orre's neutral one, Approved and Rejected read-only; the tab's
  badge counts agents' pending suggestions. `InboxStore` opens the database per call. A
  rule approved here is applied to history, never over a category you set. No undo yet,
  no CHECKS, Show in Transactions or Edit in Rules (user). Snapshots (`inbox-*`) checked
  by eye against `564-0`, `ABK-0`, `DL8-0`, `EWZ-0`; the window itself only
  build-verified. Decisions: `ARCHITECTURE.md` → "Decision record: Inbox"
- ✅ M3.7 Rules and categories editors. The rules editor is built with M3.13 (the Rules
  view's inspector: When, Then with every action kind, Order, reordering, delete). The
  categories editor (2026-11-15) is the Rules view's fourth section, Categories,
  composed from existing specs, not drawn (user): every group and its subcategories
  with kind and usage, filtered by kind or Hidden; the inspector renames (the label
  only), hides or shows, and deletes a category nothing uses (with a confirmation);
  "New category" adds a group with a kind, or a subcategory taking its group's kind.
  Core: `category::{add, rename, set_hidden, delete, usage, overview, derive_key}`
  (`category/edit.rs`), `CategoryTree::visible`/`is_visible`. Migration
  `V18__category_hidden.sql`: a used category can be hidden instead of deleted (user);
  hidden categories leave the menus that set a category (rules, Transactions, the
  new-transaction dialog), stay in the Category filter and the export, and MCP's
  `list_categories` marks them `hidden`. Snapshots (`rules-categories`,
  `rules-category-new`, `rules-category-delete`) checked by eye; the window itself
  only build-verified. Decisions: `ARCHITECTURE.md` → "Decision record: the categories
  editor (built)"
- ✅ M3.8 Transfer-pair confirmation. The Transactions inspector's Transfer section,
  composed from existing specs, not drawn (user, 2026-11-15): a suggested pair is a card
  like Orre's refund suggestion ("Is this a transfer?", why, the other side, "Confirm
  transfer" / "Not a transfer"); a confirmed pair is one row like a link row, its ✕
  unpairing; each decision has the undo bar. Core: `transfer::{confirm, reject, unpair,
  undo, pair_of, availability}` (`transfer/decision.rs`): status changes on the
  detected group, re-checked in `BEGIN IMMEDIATE`, refused for excluded, split or
  linked rows, a pot's mirrored pair never unpaired; undo is the inverse change, no
  change batch, no migration. `reporting::pairing` (the section's read model over the
  register, `RegisterRow::pair`) and `inbox::decide_transfer`, which settles an agent's
  pending suggestion for the same pair in the same DB transaction. Snapshots
  (`transactions-transfer`, `transactions-transfer-confirmed`) checked by eye; the window
  itself only build-verified. Decisions: `ARCHITECTURE.md` → "Decision record:
  transfer-pair confirmation (built)"
- ✅ M3.9 Split editor (the remaining amount fills in automatically; lines are locked to the
  total), tags and notes in the inspector. Core's `split::draft::SplitDraft` is the
  editor's model (remainder, the sign never typed, "Add line" filling in the rest, saved
  only when balanced) and `edit` records each inspector edit (split, tags, note, merchant
  name) as one undoable change batch; the register carries tags, the bank's note, the
  user-set name and whether a row can be split. In the app (`transactions/editor.rs`,
  `ui::tag`): Details with Merchant, Tags, Bank note and Note; the Split section with the
  editor or its Split button; the tag menu creates public or private tags (user). "Split"
  starts one line with the whole amount (user); Merchant editing built now (user). No
  migration. Snapshots (`transactions-split`, `-split-short`, `-split-over`, `-details`,
  `-split-start`, `-tag-menu`) checked by eye against `2Q3-0`, `4XF-0` and `3WT-0`; the
  window itself only build-verified. Decisions: `ARCHITECTURE.md` → "Decision record:
  splits, tags and notes in the inspector"
- ✅ M3.10 Manual transactions and cash accounts; exclude a row with a reason, with an
  "excluded" filter so nothing disappears. Transactions: the Excluded chip and excluded
  rows (`3FE-0`), the exclusion card with Include again and Edit reason…, the Source
  section on every row (raw description, the batch or "Added by you", Exclude…, Undo
  import…, Delete transaction…), the Exclude, undo-import, delete and New-transaction
  dialogs (`51L-0`, `529-0`), ⌘N (from Transactions and the accounts panel) and a cash
  account's New transaction, a manual row's Date, Amount and Account in the inspector
  (`4D7-0`); the status bar counts excluded rows. Cash accounts (`EPF-0`): the inspector's
  Counts with "Count cash…" and the starting point, and the Count cash dialog with an
  optional "Cash count difference" entry, saved with the count in one DB transaction
  (`account::count`). Core: `reporting::register`'s `RowSource`, exclusion details and
  account currency; `manual::create_in`. No migration. Undrawn states composed (review).
  Snapshots (`transactions-excluded`, `-exclude-dialog`, `-manual`, `-new-transaction`,
  `-undo-import`, `accounts-cash`, `cash-count`, `cash-count-first`) checked by eye; the
  window itself only build-verified. Decisions: `ARCHITECTURE.md` → "Decision record:
  excluding, the Source section and manual transactions (built)" and "Decision record:
  cash counts (built)"
- ✅ M3.11 Link refunds, reimbursements and settlements; outstanding-claims and owed-to-me
  lists. As D6.5 draws them: the Transactions inspector's Share (Just me / Shared /
  Claim back, Yours and Owed by) and Links sections (rows with ✕ and the undo bar,
  Orre's suggestion card), the link dialog, the linked refund's read-only category, the
  table's "· refund", "· shared", "Repays …" cells, and Owed to you as a third CHECKS
  row in the Inbox (expense claims with the matched payment, people, a person's costs,
  payments and name). Every link, unlink, share and rename is one undoable change batch:
  migration `V17__change_item_links.sql` adds a `links` field to `change_item` (user).
  Core read models: `reporting::linking`, `link::owed`. Undrawn states composed (user);
  a person's costs are the outstanding ones (user). Snapshots (`links-*`, `review-owed*`)
  checked by eye against `9WS-0`, `BAT-0`, `CCF-0`, `EB0-0`; the window itself only
  build-verified. Decisions: `ARCHITECTURE.md` → "Decision record: refunds,
  reimbursements and settlements (built)"
- ✅ M3.12 Duplicates review queue (merge or dismiss, showing why each pair was flagged) and
  reconciliation: assertion results per account, lock and unlock. Under CHECKS in the
  Inbox's panel, as D6.4 draws them (user): `crates/app/src/checks/` (Possible
  duplicates with the pair open side by side, why it was flagged, "If you merge", M / N /
  ←→ / ↑↓, the undo bar for merges and dismissals; Balance checks per account with the
  failing window, the pair that explains it, lock and unlock dialogs). Core's read model
  is `orre_core::checks` (the merge's own plan rolled back). The status bar counts
  failing checks and the flagged dot opens them. No migration. Snapshots (`review-*`)
  checked by eye against `8SE-0`, `98K-0`, `9MQ-0`; the window itself only
  build-verified. Decisions: `ARCHITECTURE.md` → "Decision record: review queues (built)"
- ✅ M3.13 Rules: "Create a rule from this edit?", apply-to-history preview, merchant alias
  editor. `crates/app/src/rules/` (the eighth tab): Rules in priority order with what each
  decides today and "Never reached", the rule inspector (When, Then, Past transactions,
  the Inbox's sparkle line, Delete rule…), drag to reorder; Apply to past transactions
  and New rule on the canvas ("Will change", "Category you set" with "Change these
  too"), one undoable change batch with the undo bar; Merchant names (applied at once);
  Change history with Undo this change…. In Transactions (`transactions/rule_prompt.rs`):
  the D6.3 prompt after a category edit and the Rule section's "Create rule from this" /
  "Open rule". Core: `rule::update`, `create_from`, `preview_draft`/`save_draft`,
  `overview`, `preview_rows`, `propose`/`rule_from`/`dismiss`; `merchant::overview`;
  `change::history`/`history_items`; `inbox::preview::touching_rule`. Migration
  `V16__rules_editor.sql` (rule origin, the batch's rule, "Not now") (user). Undrawn states
  composed (user); "Show in Transactions" left out (user). Snapshots (`rules*`,
  `transactions-rule-prompt`) checked by eye against `B1J-0`, `EFT-0`, `G2O-0`, `GZT-0`,
  `HFH-0` and `3WT-0`; the window itself only build-verified. Decisions:
  `ARCHITECTURE.md` → "Decision record: the Rules view (built)"
- ✅ M3.14 Inbox impact preview: what approving each suggestion would change (e.g. "changes
  143 past transactions; 12 have a category you set"). `orre_core::inbox::preview`
  (`describe`, `preview`, `covered_by`): the rows a rule, name or categorisation would
  change with now and after and the ones it leaves alone, spending by category, what
  decides a rule's matches today, what it settles, a wide rule summed by merchant, a
  transfer's or refund's month; computed when shown, by the approval's own code rolled
  back. In the app: the selected suggestion opens in place, the wide-rule warning, and
  the inspector's "If you approve". Category colours ranked at read time
  (`reporting::palette`, user); untrusted text through `envelope::shown`
- ✅ M3.15 AI access settings: enabled clients, access levels, revoke, kill switch, and the
  access log. `crates/app/src/settings/` (`SettingsView`, `store::SettingsStore` opening
  the database per call): the kill switch, clients with level, last call, calls in 7
  days and an ON switch that keeps the level, requests (Ignore, Enable… at a chosen
  level, starting at Aggregate), what each level shares, the client inspector (Level,
  the command with copy, the last 7 days, Remove client…); the access log by day,
  filtered by client and period, with the call inspector (redacted arguments, the
  purpose as untrusted text, never the result hash). Core: `mcp_client::enable_at`,
  `turn_off`, `turn_on`, `remove`; `access_log::activity`; `clock`. The status bar
  follows the kill switch. No migration: no display names, no live connection state
  (user, 2026-11-14). Snapshots `settings`, `settings-off`, `settings-log`,
  `settings-enable` checked by eye against `AQ2-0`, `GR9-0`, `DC6-0`, `GYO-0`.
  Decisions: `ARCHITECTURE.md` → "Decision record: Settings, AI access and backups
  (built)"
- ✅ M3.16 Backups (list, create, restore with the M1.28 checks) and the export dialog.
  Backups: Recent and One a month with what each holds (read `immutable`), Back up now
  (with the notification), Restore from a file…, the backup inspector (what restoring
  undoes, Show in folder, Save a copy…), and the Restore dialog with each check shown
  and an explicit confirmation. The export dialog (CSV / JSON, dates, account,
  category, tag, excluded rows, notes and tags) through the system's save dialog.
  Core: `backup::detail` (`contents`, `details`, `monthly`, `since`, `assess`,
  `save_copy`), `export::Options::include_excluded`, `count`, `date_span`, and
  `to_file` refusing the database; `docs/EXPORT.md`. Snapshots `settings-backups`,
  `settings-backed-up`, `settings-restore`, `settings-restore-fails`, `settings-export`
  checked by eye against `F63-0` and `GQA-0`
- ✅ M3.17 Manual asset and liability accounts (house, car, pension, mortgage, loan) with
  dated valuations; closing an account ("Close account…", every kind, with its lock-date
  guard; moved here from M3.2, user 2026-11-14). Core: valuations through
  `account::add_balance` (`EnteredSource::Valuation`; a liability at or below zero; none
  after closing); `account::close` (`closing`, `close` with its pots and, for a valuation
  account, a £0 value; never before what it holds or its lock date; `reopen`,
  `list_closed`, `deletion`/`delete` for an account nothing refers to). In the app: the
  ASSETS AND LIABILITIES `+`, New account and Add balance for assets (Value) and
  liabilities (Owed, stored negative), the valuations canvas (step-line chart and table,
  ⌘N), the inspector's footer (Close account…, Delete account… where allowed), the Close
  and Delete dialogs and the undo bar that reopens (user). CLI `accounts close`,
  `reopen`, a CLOSED column. No migration. Snapshots (`accounts-asset`,
  `accounts-liability`, `new-account-asset`, `add-valuation`, `add-balance-liability`,
  `close-account`, `close-account-bank`, `delete-account`, `account-closed`) checked by eye
  against `BSI-0` and `EPF-0`; the window itself only build-verified. Decisions:
  `ARCHITECTURE.md` → "Decision record: asset and liability accounts, closing and
  deleting (built)"

**Punch list from the user's exit check** (found 2026-11-17; to do later, user):

- ✅ M3.P1 Import: say why Review is disabled. `ImportDialog::review_blocker`
  (`crates/app/src/import/dialog.rs`) already words the reason ("Choose the account to
  import into.", a currency mismatch, no rows, a mapping problem) but only greys out the
  button; show it in the footer in place of "Nothing is saved until you import…".
  Built: the Map footer's caption is the blocker while there is one (with no accounts:
  "There are no accounts yet. Add one in the Accounts panel to import into."; with
  accounts but none that can take an import, all closed, pots, assets or liabilities:
  "No account can take an import. Add a current or card account in the Accounts panel.")
- ✅ M3.P2 Import: the Account menu with no accounts says "No matches", the filter's wording;
  it should say there are no accounts yet. Built: `MenuList`/`Select::empty_text` for a
  menu with no options ("No accounts yet", or "No accounts to import into" when there are
  accounts but none can take an import; one row; the footer says what to add);
  "No matches" stays for a filter that left none
- ✅ M3.P3 Import: "New account…" in the Account menu, so a fresh database isn't a dead end
  (today: close the dialog, add the account in the panel, import again). New UI: decide
  whether to draw it in Paper first or reuse the New account dialog, with the file's
  currency filled in and the new account chosen on return. Built (user: reuse the
  dialog): the menu always ends with a hairline and "New account…"
  (`MenuGroup::footer`); it opens New account over Import, hosted by the Import dialog,
  with the file's currency and the preset's bank; Add account chooses it and Import
  carries on. Drawn on `IS2-0`; decision record "New account from Import" (`import.md`)
- ✅ M3.P4 Import: dropping several files on the window imports only the first and drops the
  rest without a word (`shell.rs` and `import/dialog.rs` take `paths().first()`); import
  them in turn, or say that only one was taken. Built: the second option, on the window
  and the drop zone ("Only the first of 3 files was opened. Drop the others one at a
  time."); a queue would need a design for what Cancel does to the rest
- ✅ M3.P5 Date fields hide the first digit of a two-digit day: `ui::field::date_input` is 96px
  (`--container-column-amount`) in Geist Mono, too narrow for `17 Nov 2026`, so the text
  scrolls and reads `7 Nov 2026` while 17 Nov is what gets saved. Used by New account, Add
  balance and valuations, Close, New transaction and the manual row's Date in the
  inspector. Design the width (or size) in Paper first, then check every caller. Built:
  a new token `--container-field-date` (112px, `size::FIELD_DATE`; tokens re-exported,
  hash `f0be798c`); the manual row's Date still fills its row
- ✅ M3.P6 Add balance: the date starts on today, so on an account created today with an opening
  balance the default is always refused (one entered balance per account and day); start
  on a free day, or say so before the first click. Built: the second option (the date
  stays today; an earlier one would sit before the opening balance): "This account already
  has a balance on 17 Nov 2026. Choose another day." live under the caption, the Date
  field in a `danger` border and Add balance disabled until the day is free (drawn:
  `IRD-0`, snapshot `add-balance-day-taken`)
- ✅ M3.P7 Add balance: the error stays after the fields change; clear it on edit as the other
  dialogs do (`crates/app/src/accounts/add_balance.rs`)
- ✅ M3.P8 Add balance: `account::Error::BalanceExists` shows its date as `2026-11-17`, not the
  field's `17 Nov 2026`. Built: the dialog writes every ISO date in core's refusal as the
  field does (also `AfterClosing` and `Locked`), leaving dates inside paths and
  timestamps alone
- ✅ M3.P9 Entered balances can't be edited or removed: core has `account::add_balance` only, so
  a wrong opening balance can't be corrected, yet `BalanceExists` says "edit that one
  instead". Needs core functions (with the lock-date guard) and inspector UI, designed in
  Paper first. Built: `account::update_balance` (the old and new day both after the
  lock) and `remove_balance`; a balance row in the inspector opens Edit (Add balance with
  its values, "Remove balance"); removing shows an undo bar. Drawn on `IS2-0`; decision
  record "editing and removing entered balances (built)" (`accounts.md`)
- ✅ M3.P10 Accounts panel: right-click menus on its rows (user). The app has no context menu yet
  and DESIGN.md has none, so draw it in Paper first (likely `ui::menu` opened at the
  pointer). Proposed actions, each one the app already has elsewhere:
  - Account: Show transactions, Add balance…, Import into this account…, New
    transaction…, Edit in the inspector, Close…, Delete… (only when empty)
  - Pot: Show transactions, Add balance…
  - Asset or liability: Add value…, Edit in the inspector, Close…, Delete… (only when
    empty)
  - Category: Show transactions, Rename…, New subcategory…, Hide, Delete… (only when
    unused), Open in Rules
  - Uncategorised: Show transactions

  Built: `MenuList::context` opened at the pointer (`ui::menu::at_pointer`), items from
  `accounts::context_menu`, each an existing action. A category's Rename…, New
  subcategory…, Hide and Delete… open Rules → Categories on it (the editor can't be
  driven from outside yet); a cash account has no Import item. Drawn on `IS2-0`; decision
  record "right-click menus on the accounts panel" (`app.md`)
- ✅ M3.P11 Transfer or refund (user: "poor UX"): money in that matches a purchase in another
  account (same amount, days apart) is always suggested as a transfer, and while a row is
  in a suggested transfer the Links section is hidden (`RegisterRow::takes_links`), so the
  only way to link it as a refund is "Not a transfer" first, which nothing hints at. Found
  with a manual +£12.40 in a cash account against an imported −£12.40 purchase. Offer the
  refund in the suggestion itself (e.g. "Is this a transfer?" with Confirm transfer, Link
  as refund, Neither), or let detection prefer a refund when the money out is a purchase
  rather than a transfer out; decide the wording and layout in Paper first. Built (user:
  in the card): "Transfer or refund?" with Transfer / Refund / Neither when the money out
  is spending; Refund is `inbox::link_as_refund` (reject the pair and link, one DB
  transaction, one undo). Drawn on `IS2-0`; decision record "transfer or refund in the
  suggestion card" (`transactions.md`)
- ✅ M3.P12 Transactions: "This week" and "Last week" in the Date chip's menu (user). Today
  `reporting::register::DatePreset` has This month, Last month, Last 3 months, This year and
  All time. Decide which day a week starts on (Monday, or a setting). Built:
  `DatePreset::ThisWeek`/`LastWeek`, first in the menu; weeks run Monday to Sunday (ISO
  8601, not a setting), and This week runs to Sunday as This month runs to its end
- ✅ M3.P13 Transactions: sorting by a column looks like it does nothing (user, test 5.2). By
  design it sorts only within each day (`docs/ARCHITECTURE.md`, Transactions decision 5;
  `Register::select`), and with one or two rows a day, as in the fixtures and much real
  data, nothing visibly moves. Revisit the decision: e.g. a column sort that drops the day
  grouping (one flat list, date as a column) until it's cleared, keeping today's within-day
  sort only if it still earns its place. Changing it is a new decision record and a Paper
  pass on the flat table. Built (user: flat): any column sort is one flat list with a
  Date column first and no day headers or nets; DATE or a third click goes back to days.
  Drawn on `IS2-0`; decision record "a column sort is one flat list" (`transactions.md`)

**Exit criteria:** day-to-day use (import, categorize, split, link refunds, review the inbox
and duplicates) without the CLI.

Every task and punch-list item is done; only the user's sign-off on this exit check, on
real data, remains.

---

## Architecture review — findings ✅

From an adversarial review of `ARCHITECTURE.md` and `docs/architecture/` on 2026-11-17.
Not a milestone: the items cut across M1–M3's code and are ordered by severity. R.1–R.5
should land before the open-source release (M5). "Docs only" means the finding was read
from the decision records and not yet confirmed in the code, so confirm it first. An item
that changes a decision needs a new decision record in its topic file.

**High**

- ✅ R.1 Approval is reachable by the agents it contains. `orre inbox approve <id>…` takes
  no confirmation and has no terminal check (`crates/app/src/cli/inbox.rs`, `ApproveArgs`),
  and Claude Code and Codex have a shell by default, so an injected agent can file a
  suggestion and approve it. Require an interactive terminal for `inbox approve`,
  `categorize` and `ai-access enable` (and whatever else changes data or access as the
  user), or say in the invariant that containment depends on the client having no shell.
  Decision record in `mcp.md`; update `mcp-setup.md`. Done: commands that act as the user
  need an interactive stdin (`cli/terminal.rs`), with no bypass; the record says it's a
  speed bump and containment needs a client with no shell (`AGENTS.md` invariant too);
  previews (`import --dry-run`; `undo-import`, `schedules delete` and `inbox delete-report`
  without `--yes`) run without one
- ✅ R.2 `list_reports` leaks across access levels. It is an `aggregate` tool and returns
  every client's report bodies (`crates/mcp/src/tools/catalog.rs`), so a `standard`
  client's review, with merchant names and bank text, reaches an `aggregate` client, and
  one client can plant text for another. Return only the caller's own reports, or record
  the author's level on the report (a migration) and filter on it. Add the case to the
  tool × level matrix. Done: both: own reports only, and only those saved at the client's
  current level or below (`report.access_level`, V19)
- ✅ R.3 A source ID is trusted with no content check. A row with an ID never matches by
  fingerprint, and near-duplicate review skips imported pairs with identical text
  (`crates/core/src/duplicate.rs`), so a bank that regenerates `FITID`s on each download,
  or a file imported again through a mapping without its ID column, duplicates every
  overlapping row without a flag; only a balance check would notice, and Monzo-style
  exports have none. When a new row's ID is unknown, look for a stored row on the account
  with the same date, amount and normalised text from another batch, and queue the pair as
  a duplicate candidate (a new `reason`) or flag it in the preview. Fixture rows for both
  cases. Done: candidate reason `unmatched_source_id` (V20); rows the import matched by
  source ID are no counterpart
- ✅ R.4 A bank's correction has no way in. The same source ID with a new amount or date
  (tips, fuel holds, settled FX) is flagged `SourceIdConflict` and left out
  (`crates/core/src/import.rs`); source fields are immutable, so the stored row stays
  wrong, and the only remedy is undoing the whole batch with every user edit on it.
  Decide a "superseded" resolution: exclude the old row with its own reason and import the
  new one, moving the user's fields across as a duplicate merge does, as one undoable
  batch. Overlaps L.21 (pending → posted): decide them together. Done: the row is a
  correction the user accepts; the stored row is excluded as `superseded` (V21, the first
  migration with foreign keys off); undoing the import restores it. Only a row within 31
  days moving money the same way is a correction; a scheduled payment's match moves to it.
  L.21 stays open for a new ID with a new amount
- ✅ R.5 Editing a preset changes dedupe identity without a word (docs only). The
  fingerprint depends on which columns a preset joins into the description, its date
  format and whether it maps an ID column, but presets have no version and only the
  fingerprint's format is frozen by a test. Add a golden test per bundled preset fixture
  that freezes each row's external ID and fingerprint, and write the rule into
  `presets/README.md`: a change that moves either needs a new preset ID. Done:
  `crates/import/tests/preset_golden.rs` and `tests/golden/`

**Medium**

- ✅ R.6 Merchant names are untrusted text outside the envelope. At `standard`,
  `Call::merchant` (`crates/mcp/src/registry.rs`) strips invisible characters and runs
  the instruction check, but the field is unlabelled, has no length cap and its domains
  aren't made inert; the merchant falls back to the raw description, and a payer chooses
  an incoming transfer's name. Run it through the same sanitising as
  `description_untrusted` and mark it untrusted in the output, or reword the invariant
  ("raw text only in `description_untrusted`"). Done: a merchant at `standard` goes
  through `envelope::sanitize` (capped, domains inert, withheld and counted) and is labelled
  untrusted in its schema, the server instructions and the review prompt; the invariant is
  reworded
- ✅ R.7 `pseudonymous` promises more than it gives (docs only). Stable tokens beside exact
  amounts, dates and categories re-identify most merchants (a monthly £10.99 under
  subscriptions), and account names show at this level although they are hidden at
  `aggregate` because a name can name a person. Either say plainly in `mcp.md`, the
  Settings copy and `mcp-setup.md` what the level does and doesn't protect, or tighten it
  (account labels instead of names, coarser amounts). Done (user): say it, and below
  `standard` accounts are labels (`current 1`, `pot 2`) with `name: null`; `mcp.md`,
  `mcp-setup.md`, Settings and the CLI help say amounts and dates stay exact, so it isn't
  anonymous. Amounts stay exact
- ✅ R.8 The lock has no single definition (docs only). It "guards balances only", yet any
  exclusion in a change batch is refused for changing "reported totals", category edits
  (which change totals too) are allowed, a single-row non-duplicate exclusion is allowed,
  and a manual row's merchant edit is refused though it moves no balance. Choose one
  meaning, derive each guard from it in one decision record in `links-and-checks.md`,
  and change the guards that don't follow. Done (user): balances only; one
  predicate, `exclusion::moves_balance`, for single-row exclusions and change batches
  (rule applies, merges, undo); a manual row's time, merchant and description are editable
  in a lock; the 2026-11-12 amendment is reversed. Schedules' lock guards are gone too,
  since a schedule moves no balance (record, point 8)
- ✅ R.9 Two net worths. The accounts panel leaves out an account with no stated balance
  (`reporting::account_tree`); `net_worth_history` sums it from zero and marks it
  `unanchored` (`reporting::net_worth`). Pick one rule for both before M4.5. Done (user):
  the panel's; `net_worth::counts` decides for both, and the history lists such an
  account, flagged, adding zero
- ✅ R.10 "Transfer" has two sources of truth (docs only): a category of kind `transfer` and
  a confirmed pair. A suggested pair counts as income and as spending until confirmed, and
  the table doesn't mark it ("transfer-pair confirmation (built)", point 10), so reports
  are overstated by default. Mark suggested pairs in the table, decide whether rules skip
  confirmed legs ("what a transaction can take", point 4), and say how the two mechanisms
  relate. Done: the table marks a suggested pair's legs with a `warning` ⇄
  (composed, user); rules skip the category of rows that take none of their own
  (`ApplyPreview::skipped_categories`); the record says how the two relate
- ✅ R.11 Category keys are "forever" but can be reused (docs only). A deleted category's
  key is free again and user keys share the key space with future defaults, so a preset's
  key or a later default can land on an unrelated user category of another kind. Keep
  deleted keys taken (a tombstone) or give user keys their own prefix; a migration either
  way. Done (user): both. A trigger keeps every deleted key in `category_key_retired`
  (V22) and `derive_key` never hands one out again; new user keys get a `u_` part
  (`u_pets`, `food.u_snacks`) that defaults never use. Keys made before V22 stay as they
  are
- ✅ R.12 Backups keep deleted data with no end (docs only). One automatic backup a month is
  kept forever and manual ones are never deleted, all unencrypted, so a deleted report or
  an undone import stays on disk; `mcp.md` F.5's "until they rotate out" isn't true of the
  monthly ones. Decide a cap or an age limit and show the total size in Settings. User
  presets live outside the database, so a restore or a move to another computer can leave
  `account.preset_id` naming nothing: include them in backups or say so. Done (user):
  automatic backups keep the 10 most recent and one a month for 12 months; manual ones
  stay until deleted. Settings shows the total and has "Delete backup…" (composed, not
  drawn in Paper); `orre backup --delete <file> [--yes]` needs a terminal with `--yes`.
  Presets aren't in backups and the restore dialog and CLI say so; an account whose preset
  isn't on this computer says "not on this computer"
- ✅ R.13 A file imported into the wrong account isn't noticed. The OFX account ID is parsed
  and thrown away, so nothing warns when account A's file goes into account B. Store a
  keyed hash of it per account (never the number) and warn in the preview when it differs.
  Done (user): warn, and learn on commit. HMAC-SHA256 under `setting`
  `import.account_key`, one row per batch (`account_source_identity`, V23), forgotten on
  undo. The preview warns when the account knows other numbers, or another account has
  had this one (named); importing anyway learns it. The Review block is composed, not
  drawn in Paper
- ✅ R.14 Performance is unmeasured (docs only). Accounts and Inbox read on the main thread,
  the Inbox runs a rolled-back approval for each pending suggestion when it opens (up to
  200 per client), `rule::overview` evaluates every rule over every transaction on each
  reload, and every MCP read is also a synced write to the access log. Measure on a large
  synthetic ledger first (L.30), then move what is slow off the main thread. Done:
  measured at 100,606 transactions, 100 rules and 400 pending suggestions
  (`crates/core/examples/ledger_bench.rs`, release; `app.md`, "Decision record: performance
  at 100,000 transactions"). The Inbox reads on a background thread, and its previews
  share one ledger read: all 400 take 22 s instead of 179 s on the UI thread, so at
  this size it opens about 22 s after the window (not worked on here). Core fixes:
  `covered_by` 19.8 s → 0.4 ms, `rule::overview` 735 → 412 ms, the ledger's ID query 20
  → 1 ms. An access-log write costs 2.8 ms with `synchronous = FULL`, kept. Accounts and
  Rules still read on the UI thread: R.19

**The documents**

- ✅ R.15 A current-state reference section for the eight topics that are decision records
  only (accounts, transactions, rules, links and review queues, inbox, reporting,
  settings and backups, the app), so the rule in force can be read without following
  amendments across files; the records stay as the history. Done (docs only): each of the
  eight files opens with a section that states the rule in force, checked against the
  code and citing the record that set it; `ARCHITECTURE.md` → Contents lists them
- ✅ R.16 Stale reference text: `data-model.md` lacks `category.hidden` (V18) and the
  `links` change field (V17), and its migration list stops at V16; `ARCHITECTURE.md`'s
  command list leaves out `accounts`, `undo-import`, `inbox`, `categorize`, `ai-access`,
  `budgets` and `schedules`; the Platforms row says "Windows compiles in CI" and there is
  no CI yet (M5.5); `reporting.md` cites "Inbox impact preview, point 1" for applying a
  rule to history, which is "Inbox", point 2. Done (docs only): all four, plus
  `data-model.md`'s section is now "Data model" and lists V17–V23, `AGENTS.md`'s command
  list, the Backups row's retention, a missing Contents entry, and `superseded` rows
  leaving a balance check's sum in `import.md` and the `reconciliation` docs
- ✅ R.17 `AGENTS.md`'s invariant says no names in MCP output, while `mcp.md` A.1 accepts a
  person's name at `standard`. Reword the invariant to match the decision. Done (docs
  only): no PII below `standard`; account numbers, notes, share labels, private tags,
  merchant addresses and attachments at no level
- ✅ R.18 A rule's preview doesn't say why it skipped a row's category. Since R.10, rules
  leave the category of a confirmed transfer leg or a confirmed refund's money in alone
  and list them in `ApplyPreview::skipped_categories`, but the Inbox's "If you approve"
  shows nothing for them. Design the line in Paper, then show it. Done: drawn on the
  Inbox · D6.6 states board (`EWZ-0`). "If you approve" has a Skips row ("1 transfer ·
  2 linked refunds") and a caption sentence, and the open preview's AFTER lane says
  "Stays, a transfer" / "Stays, a linked refund" (`After::NoOwnCategory`, also for a
  categorisation's skipped rows). A linked refund no longer makes a rule wide; an
  incoming transfer leg still does, as the rule would categorise the next one at import
  (`inbox.md`, "the preview says why a rule skips a row's category", point 2). The Rules view's apply preview
  still doesn't show `skipped_categories` or `skipped_splits`, and the Inbox doesn't show
  or discount a split template's `skipped_splits`

- ✅ R.19 Accounts and Rules read on the UI thread (from R.14). At 100,606 transactions the
  Accounts load takes 117 ms (`reporting::account_tree` 104 ms, up to ~220 ms in one run)
  and the Rules load 521 ms (`rule::overview` 412 ms, of which reading the transactions
  the rules see is ~100 ms), against a 50 ms budget. Move both to a background read as
  the Inbox does (`Inbox::unloaded`, `reload` on `background_spawn`, keeping the last data
  until the new read lands), and decide what each view does between an edit and its
  reload. Done (composed): both read in the background (`Accounts::unloaded`,
  `Rules::unloaded`), saying nothing until the first read; writes stay on the UI thread.
  A new account is selected at once. Rules does at once what the next input needs: a
  move reorders, a delete removes the row, a create closes its preview and selects the
  item when the read lands, a save isn't repeated, and a read doesn't overwrite a form
  edited since it was asked for (`app.md`, "Accounts and Rules read in the background").
  The Rules view's apply preview and other selection reads stay on the UI thread

**Exit criteria:** every item is fixed, or closed by a decision record that says why it
stays as it is.

---

## UI review — agent-readable views and keyboard paths ✅

From reviewing Vercel's UI SDK against Orre on 2026-11-17 (UI SDK itself is web-only
and isn't used). Three ideas apply: views that state their contents and actions as data
instead of pixels, behaviours kept apart from their looks, and a keyboard path for every
drag. Not a milestone. Order: U.1 with U.4, then U.6. New UI is drawn in Paper first.

**Views that describe themselves**

- ✅ U.1 A test-only `describe()` on the Inbox: its rows (label, category, amount), the
  selection, the open dialog and the actions available (Approve, Reject, ↑↓), as plain
  data, compiled for tests only. Untrusted text goes through `envelope::shown` as the view
  shows it. Move a few `crates/app/src/inbox/tests.rs` tests onto it, so they assert on
  what the view shows instead of its internals. The pilot: it decides whether U.2–U.3
  follow. Built (`inbox/describe.rs`, shared types in `ui::describe`); verdict: the tests
  read as what the user sees and caught unchecked states, so U.2–U.3 followed. Cost: the
  view renders from `…Text` structs describe shares, which roughly doubles its diff
- ✅ U.2 Transactions' `describe()`, replacing the hand-written read-backs
  (`transactions/write_tests.rs`: `said()`, `Shown`) and their copies in other tests
- ✅ U.3 `describe()` on Checks, Rules, Settings and Accounts, in that order, each with its
  tests moved onto it
- ✅ U.4 Decision record in `docs/architecture/app.md` (listed in `ARCHITECTURE.md`) and a
  line under `AGENTS.md` → Invariants, before U.1 lands: `describe()` and any path that
  runs a view's actions exist only in tests or a dev feature, never in the release binary
  and never reachable from `orre mcp`. A tool that could press Approve would get round
  "MCP writes go to the inbox only" and approval in Orre's own UI. L.7's in-GUI MCP
  falls under the same record
- ✅ U.5 A dev-feature-only local socket so an agent can drive a dev build through
  `describe()` and its actions, against a scratch `ORRE_DATA_DIR`, to check a UI change in
  the running app instead of reporting it as build-verified. Never against the real
  database. Deferred on 2026-11-18, built on 2026-11-21 at the user's request: the
  `dev-driver` feature (debug builds only), `orre dev` (`describe`, `show`, `press`,
  `type`, `click`, `run <label>`), with dialogs and focus described at the shell
  (`shell::describe`). An action is named by its accessible label, and a test per view
  checks that every button it describes can be clicked. Hover isn't described.
  `docs/architecture/app.md` → "Decision record: the dev driver"

**Behaviours apart from looks**

- ✅ U.6 One shared list behaviour in `ui::`. ↑↓ is bound separately in nine places
  (`transactions.rs`, `accounts.rs`, `inbox.rs`, `checks.rs`, `rules.rs`, `settings.rs`
  twice, `ui/menu.rs`, `showcase.rs`), each with its own selection code. The behaviour
  owns the bindings, the selection and scrolling it into view (Home/End and page keys if
  wanted); each view keeps its look. No visual change. It's also where `describe()` reads
  the selection and actions from
- ✅ U.7 Decision keys as data: A / R in the Inbox and M / N / ←→ in Checks declared as
  (key, action, label) once, so `describe()` lists them and the `ui::kbd` hints come from
  the same place and can't drift. Paper first if the hints change

**A keyboard path for every drag**

- ✅ U.8 Reorder rules from the keyboard: the Rules table is drag-only
  (`rules/table.rs`, `on_drag`/`on_drop`). ⌥↑ / ⌥↓ on the selected rule through the same
  `Rules::write` pipeline (undoable), and a line saying where it went ("Moved 'Tesco →
  Groceries' to position 3 of 12"). Keys, wording and placement in Paper first
- ✅ U.9 Audit for other pointer-only actions (drag-only or hover-only) across the views:
  a list first, then a keyboard path for each, designed in Paper where it shows. Dropping
  a file already has ⌘I

**Exit criteria:** U.4 is recorded; the Inbox tests run on `describe()` and U.2–U.3 are
done or closed by a note saying why not; every drag in the app has a keyboard path.

---

## Live updates ✅

What SurrealDB's live queries give, built on SQLite. Before this work a view reloaded
only after the app's own changes and when it was shown (`ShellView::show`), so a change
made by another process was seen only when the user moved around: an MCP client filing
suggestions, `orre import`, `undo-import` or `categorize` in a terminal. The Inbox badge
and Transactions stayed stale meanwhile. SQLite can't push a change to another process,
so the app now notices it itself and reloads without a click (LU.3). Not a milestone.
Order: LU.1, LU.2, LU.4, then LU.3 and LU.5; LU.6 alongside.

- ✅ LU.1 Decision record in `docs/architecture/app.md` (listed in `ARCHITECTURE.md`)
  before code. To decide:
  - **Detection by the files.** Once a second, read the size and modified time of the
    database and its `-wal` file, plus the database file's identity. Every commit from
    any process touches `-wal`. This opens nothing, so "no connection is held" stands.
  - **Rejected: a held connection polling `PRAGMA data_version`.** Restore renames a new
    file over the database (`backup::restore`), so a held connection keeps reading the
    old one. It would also block that rename on Windows (L.10).
  - **Rejected: a change-journal table filled by triggers.** A trigger on `suggestion`
    would write another table on every MCP insert, against "MCP writes go to the inbox
    only". It would also cost a write per imported row.
  - **The poll interval, and slowing it when the window isn't focused.**
- ✅ LU.2 A watcher in core (`orre_core::watch`, no GPUI), with two steps:
  - **Stamp:** the file metadata above. An unchanged stamp ends the check.
  - **Fingerprints:** on a changed stamp, one open through `backup::gate::open` runs a
    cheap indexed query per topic, e.g. the Inbox's newest suggestion id and pending
    count, or the newest transaction and `change_batch` ids. It returns the topics that
    differ from last time.

  Tests:
  - A write from a second connection reports its topic.
  - A checkpoint with no data change reports none.
  - A restore (a new file identity) reports every topic.
  - A held restore lock skips the check, and the next check catches up.
- ✅ LU.3 Shell wiring:
  - A background timer, like `ShellView::keep_time`, and only in the app: tests and
    snapshots don't run it.
  - It maps each topic to the views that reload: Inbox and its badge; Transactions,
    Accounts and Checks; Rules; Settings (AI clients). The status bar follows the
    reloads, as now. A Rules change reloads Transactions too (merchant names); a
    Ledger change reloads Rules only while Rules is on show (decided 2026-11-22).
  - Settings: when the log's chase for the selected call fails, truncate the log back
    to the length it had before the chase, so it doesn't keep one page more per
    failed chase.
  - Everything off the UI thread, within the 50 ms budget.

  Built in `shell::live`: the app's own writes go through the shell, which rebases and
  reloads in one place (`docs/architecture/app.md` → "Decision record: live updates in
  the shell (2026-11-22)", confirmed 2026-11-23).
- ✅ LU.4 Reloading keeps your place. A reload that arrives unasked must not lose what the
  user is doing. Per view:
  - **Transactions:** selection, scroll, filters, search, an inline edit or open dialog.
  - **Accounts:** selection and the inspector form.
  - **Inbox:** the selected and open suggestion. A suggestion decided elsewhere, or by
    an approval in flight, goes without moving the selection.
  - **Checks:** the open queue.
  - **Rules:** already keeps forms (`Rules::form_edits`); check that it holds for
    reloads nobody asked for.
  - **Settings.**

  Each view gets a test on its `describe()`: an outside change lands mid-task, and the
  view's state is unchanged apart from the new data.
- ✅ LU.5 Showing what arrived, designed in Paper first: for example the badge counting
  up, new rows marked briefly, a quiet "3 suggestions from Claude Code" in the status
  bar. Never a toast that takes focus.

  Designed on `JJY-0` and `JYE-0` (`docs/DESIGN.md` → "Live updates: showing what
  arrived"); built from `.scratch/lu5/spec.md`: `shell::arrivals` (the line, the badge's
  unseen fill, row marks and their timings), Transactions' and the Inbox's diffs of a
  read, and the removed-row note. Only reads the shell started for a check count; a read
  for the app's own write clears that, so an overlap is left unshown, never shown as
  another process's.
- ✅ LU.6 Cost: measure an idle tick and a fingerprint check at 100,000 transactions
  (`ledger_bench` or a new example) and record it in LU.1's record. Fingerprints use
  existing indexes, or a new migration adds them. Also the Settings clients read on the
  UI thread (`reload_clients`: an access-log flush and aggregates over `mcp_access_log`).

  Measured (`docs/architecture/app.md` → "Decision record: live updates (2026-11-21)",
  point 9, confirmed 2026-11-23): an idle tick is 3 µs, a fingerprint check
  5 ms, and no fingerprint needs an index, so no migration. The clients read is 129 ms
  with a year of heavy MCP use, so `reload_clients` reads in the background now.

**Exit criteria:** with Orre open, a suggestion filed by an MCP client and a
`orre import` run in a terminal both show within about two seconds, without clicking.
Nothing typed, selected or scrolled is lost. The idle cost is measured and recorded.
Checked by tests, and in the real window by the user.

Every task is done and the tests pass; only the user's check in the real window remains.

---

## Full-text search (FTS5) 🚧

Ranked, fuzzy matching over bank text, with SQLite's FTS5 in place of a vector index:
deterministic, no model and no new dependency. The bundled SQLite is built with
`SQLITE_ENABLE_FTS5`. Text is searched in two places today:

- **The Transactions search box** filters the register already in memory
  (`Register::select`) and never reads the database (M3.3). It stays as it is.
- **The database text filter** (`reporting::ledger`, behind MCP's `search_transactions`)
  is `LIKE '%…%'` over the display merchant, bank merchant and raw description, a scan of
  every row.

FTS5's `trigram` tokenizer answers the same substring question from an index, and ranks
by `bm25`, which "similar transactions" builds on. Not a milestone. Order: FT.1, FT.2,
FT.3, then FT.4 and FT.5; FT.6 alongside.

- ✅ FT.1 Decision record in `docs/architecture/reporting.md` (listed in
  `ARCHITECTURE.md`) before code ("Decision record: full-text search over bank text
  (2026-11-23)", confirmed 2026-11-24). To decide:
  - **What's indexed:** the display merchant, the bank merchant and the raw description.
    Notes stay out, since they never reach MCP at any level.
  - **The table:** an external-content FTS5 table (no second copy of the text) with the
    `trigram` tokenizer, case-insensitive.
  - **How it stays in sync:** triggers on `"transaction"` and on wherever the display
    merchant is stored. MCP never writes either, so "MCP writes go to the inbox only"
    is untouched.
  - **Untrusted text:** the index holds bank text, which is untrusted, but it's used only
    for matching, as now.
  - **Access levels unchanged:** text search stays at `standard` only (`mcp.md`, "Text
    search").
  - **Queries under three characters,** which trigrams can't index, fall back to `LIKE`.
- ✅ FT.2 The migration and a core search module:
  - The migration creates the table and its triggers, and builds the index for existing
    rows.
  - The module turns user or MCP text into an FTS5 query that is always a literal
    phrase, never FTS5 syntax, so `OR`, `NEAR`, `*` or quotes in a query can't change
    what it means.

  Tests: the index follows an import, an undo, an edit to the display merchant, a
  supersede and a restore of an older backup (its migration rebuilds the index).
- ✅ FT.3 `reporting::ledger`'s text filter, and so `search_transactions`, read through
  the index. A test runs both the `LIKE` path and the index on the fixtures and checks
  they return the same rows; the MCP access-level tests are unchanged.
  (Built over a synthetic test ledger with every kind of ledger line, not `fixtures/`,
  which `orre-core` can't parse.)
- 🚧 FT.4 Similar transactions: a core function returning a transaction's nearest
  neighbours, ranked by `bm25` over its cleaned merchant stem (as `rule::proposal` cuts
  it), the same sign and a similar amount. Uses, each drawn in Paper first where it
  shows:
  - **A category suggestion for an uncategorised row,** from its nearest categorised
    neighbours, filed as Orre's own suggestion (source `orre`), not an AI one. It's
    made after an import commits, never inside it.
  - **"Similar" in the transaction inspector.**
  - **A read tool at `standard`** (later, with L.32), with its access-level test.

  Built (2026-11-24): `search::similar` and the category suggestion after an import
  (`inbox::similar`), as decided in `reporting.md` → "Decision record: similar
  transactions". Waiting: "Similar" in the inspector (Paper first) and the read tool
  (L.32).
- ⬜ FT.5 L.29's structured search syntax uses this as its text engine (noted in L.29);
  no work here until L.29 starts.
- ✅ FT.6 Cost: measure at 100,000 transactions and record in FT.1's record:
  - the index's size on disk, and so the size of backups
  - the time an import adds
  - a text query through the index against `LIKE`

  If it doesn't pay at that size, FT.3 goes back to `LIKE`, and the index serves only
  FT.4. Until then FT.3 reads through the index (user, 2026-11-24). A first look in
  FT.3's review is in FT.1's record, point 7: much faster for a rare text, slower for a
  text in every row, and the index nearly doubles the file.

  Measured with `ledger_bench --only search` (FT.1's record, point 9): a backup grows
  from 34.8 to 55.0 MiB, a 1,000-row import commits 67 to 105 ms later, and a search
  for a merchant in 0.4% of rows takes 10 ms instead of 180 ms; a text in nearly
  every row is 1.5 times slower. It pays, so FT.3 keeps the index (composed, for the
  user to confirm).

**Exit criteria:** `search_transactions` with a `query` returns the same rows as before
and is measured faster at 100,000 transactions. An uncategorised row with categorised
look-alikes gets Orre's own category suggestion. Query text can't change a search's
meaning.

---

## M4 — Analytics 🚧

- ✅ M4.1 Monthly overview: income vs spending vs net (bar/line), category breakdown, MoM change.
  Built before M4.11 on calendar months (user, 2026-11-24): core's
  `reporting::overview` and the Overview screen (`crates/app/src/overview`), with the
  chart elements in `ui::income_chart` and `ui::ranked_bars`. Decision record "the
  Overview" in `docs/architecture/reporting.md`. Deferred: storing category colours
  (waits for the user's decision), the recurring price-rise flag and scheduled series
  (M4.3, M4.8), the chart's growth animation and round line joins.
- ⬜ M4.2 Trends: category and merchant over time, top merchants
- ⬜ M4.3 Recurring payments / subscriptions view
- ⬜ M4.4 Budgets: per-category monthly targets with progress
- ⬜ M4.5 Net worth: balances over time across accounts
- ⬜ M4.6 Multi-currency reporting in home currency; opt-in ECB rate fetch + manual rates
- ⬜ M4.7 Reviews: browse and delete saved MCP reports
- ⬜ M4.8 Upcoming payments and a daily balance forecast per account from schedules,
  showing the lowest balance before the next income; `cash_flow_forecast` read tool
  (cached FX only)
- ⬜ M4.9 Budgets: per-category rollover; fixed, non-monthly and flex groups, with yearly
  schedules seeding the non-monthly amounts; a spending-pace line
- ⬜ M4.10 Safe report rendering: a markdown subset with no HTML or images, and links shown
  as plain text; reports are marked `ai_authored` when read back
- ✅ M4.11 Month start (L.15, decision record "the month start" in
  `docs/architecture/reporting.md`). Planned first in M4, but M4.1 was built before it,
  on calendar months (user, 2026-11-24). Core and MCP built (2026-11-25), and the
  Overview, Transactions' month presets and Inbox labels follow the rule; the app side
  built from D6.12 (2026-11-25), so the rule is set in Settings:
  - **Core:** the rule as a typed `setting` value (day 1–28 or the last day; weekends off
    or the Friday before; default the 1st). `Period` cuts months under it and keeps its
    `YYYY-MM` name. Tests: no gaps or overlaps for every start and weekend choice across
    leap years, the naming at day 15/16, and a budget keyed `2026-12` staying December's
    when the rule changes.
  - **Reports and MCP:** `monthly_summary`, `spending_by_category`, `budget_status`,
    `net_worth_history` and `monthly_review` follow it; month results gain `from` and
    `to`, and the tool descriptions say so. Access-level tests unchanged.
  - **App** (D6.12, Paper `K57-0`, `KEQ-0`): Settings → CALENDAR → Month start, the
    start day and weekend choice applied at once, a preview of last, this and next
    month, and what follows the rule (`settings/month_start.rs`). The Date chip's month
    presets show their dates when they differ from the calendar month, in the menu and
    on the chip; the Overview's switcher shows the month's dates when they differ. The
    Settings topic now reloads the Overview, Transactions and the Inbox (`live::views`).
    Tests: the setting from the select and the Weekends control; the shell re-cutting
    the Overview's month and the Date chip after a change (the 25th and the last day,
    both with the Friday before); dates shown only when they differ.

**Exit criteria:** the dashboard answers "how am I doing?" at a glance, including what's
due before payday.

---

## M5 — Open-source release (v0.1) ⬜

- ⬜ M5.1 README (screenshots, install, MCP setup), CONTRIBUTING, CODE_OF_CONDUCT, SECURITY
- ✅ M5.2 "Add your bank" guide: preset TOML + redacted fixture, no Rust required
  (`presets/README.md` → "Add a bank in four steps")
- ⬜ M5.3 Bank support for v0.1: the bundled CSV preset is Monzo (`uk-monzo`). Other banks
  import through the built-in OFX/QFX importer, which needs no preset, or the generic CSV
  mapping. Presets for Starling, Nationwide, Chase, Bank of America, Capital One, Revolut
  and N26 move to v0.2
- ✅ M5.4 Issue templates (bank preset request, bug, importer request)
  (`.github/ISSUE_TEMPLATE/`, #25)
- 🚧 M5.5 CI: fmt, clippy, tests; build on Linux, macOS, Windows (best-effort).
  `.github/workflows/ci.yml` mirrors every `lefthook.yml` pre-push job on Linux and
  macOS, adds an MSRV `cargo check` and a non-blocking Windows `cargo check`; done once
  the first runs on GitHub are green and `main` requires the checks
- 🚧 M5.6 Release artifacts: Linux tarball + Nix flake, and an unsigned macOS `.dmg`.
  No AppImage in v0.1. The Linux tarball (`packaging/linux/build-tarball.sh`,
  smoke-tested under Xvfb), the Nix flake (`flake.nix`, the release binary with the
  GPUI libraries on its rpath) and the unsigned macOS `.dmg`
  (`packaging/macos/build-dmg.sh`, ad-hoc signed, not notarised: no Apple Developer
  account) are built and checked by the `Linux tarball`, `Nix flake` and
  `macOS dmg (unsigned)` jobs of `.github/workflows/release.yml` (`workflow_dispatch`
  and `v*` tags), with install and Gatekeeper steps in `docs/install.md`; done once a
  dispatch run of all three jobs is green
- ✅ M5.7 Data location, backup, and privacy documentation: `docs/data-and-privacy.md`,
  linked from `README.md` and `SECURITY.md`
- ⬜ M5.8 Deferred to v0.2. Beancount and hledger journal export: deterministic and
  stably sorted; fixture exports are checked with `bean-check` / `hledger check` when
  those tools are installed (external tools, not dependencies)

**Exit criteria:** a stranger in the UK, US, or euro area can install it, import
their bank's export, and connect an AI client by following the README. Outside Monzo,
the import path is OFX/QFX (or the generic CSV mapping).

---

## Later

Good candidates for contributors are marked 🙋.

- 🙋 L.1 More bank presets
- 🙋 L.2 QIF importer
- 🙋 L.3 CAMT.053 (ISO 20022) importer
- 🙋 L.4 MT940 importer
- L.5 PDF import: local text extraction + per-layout templates; model-assisted
  extraction strictly opt-in
- L.6 Encrypted database (SQLCipher)
- L.7 Streamable-HTTP MCP inside the running GUI (e.g. "jump to transaction")
- L.8 Sankey view of income → categories
- 🙋 L.9 UI translations
- L.10 Windows as an officially supported platform
- L.11 Attachments and receipts: local files stored by content hash, tax-bundle export;
  never exposed over MCP
- L.12 Savings and payoff goals: targets on pots and accounts, on-track status, avalanche vs
  snowball for debt
- L.13 Accounting reports (balance sheet, income statement), saved report definitions,
  global report exclusions, CSV export of any view
- L.14 Investments and holdings with manual or imported prices (an online price fetch would
  need its own decision, like the ECB one)
- L.15 Configurable month start (payday-aligned periods). Decided (user, 2026-11-24):
  `docs/architecture/reporting.md` → "Decision record: the month start"; to be built as M4.11.
  Its point 10, a per-month override for holidays and early pay, stays here for later
- L.16 Anomaly and spike insights (median/MAD), with a read tool
- L.17 Runway, Age of Money and an in-app weekly recap
- L.18 Personal inflation index from your own recurring prices
- L.19 Calendar heatmap of daily spend
- L.20 What-if scenarios (seeded, inflation-adjusted projections)
- L.21 Pending → posted handling (needs fixtures showing which exports include pending rows)
- L.22 Match imported rows to manual entries
- 🙋 L.23 Starter merchant rule packs per region (TOML, like presets)
- 🙋 L.24 Migration presets for YNAB, Mint, Actual, Monarch and Firefly exports, including
  multi-account files
- 🙋 L.25 CSV hardening: UTF-16 via BOM, flag ambiguous `DD/MM`/`MM/DD` dates, footer rows,
  statement total shown before commit
- L.26 Statement archive: source files kept by hash and linked to their batch (pairs with L.6)
- L.27 Transfers: a queue for one-sided transfers; cross-currency pairs with the FX difference
- L.28 FX: bank-applied rate and fee per transaction; an FX fees report
- L.29 Structured search syntax shared by the GUI, exports and MCP; saved queries and
  formula cards. Its text terms use the FTS5 index (FT.5)
- 🙋 L.30 100k-transaction performance benchmark
- L.31 Account groups with subtotals and report include/exclude
- L.32 More MCP: `find_duplicates`, `preview_rule`, `reconcile_preview`, `list_suggestions`,
  `spending_pace` read tools; `duplicate_flag`, `budget_adjustment`, `recurring_review`
  suggestion kinds; tool annotations, output schemas and resources; prompts
  `categorize_inbox`, `rule_tuning`, `subscription_audit`, `year_end_review`, `weekly_review`
- 🙋 L.33 Export and import rules as TOML
