# Coffer — Architecture: Inbox

Suggestions waiting for the user: the Inbox and its impact preview.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Inbox

`coffer_core::inbox` files, previews and decides suggestions; `crates/app/src/inbox/` is
the screen. The rule in force is below; the records under "Decision records" are the
history. What MCP may file is `mcp.md` → "MCP surface" and "Decision record: M2: MCP
server", block E.

### Suggestions

- **Kinds** (`SuggestionKind`): `categorization` (1–100 items, `payload::MAX_ITEMS`),
  `rule`, `transfer_pair`, `merchant_alias`, `refund_link`. Stored in `suggestion`
  (V13) with a typed payload (`inbox::payload`, `Payload`) in canonical form, checked
  against the database when filed and again when approved.
- **Sources.** An MCP client at `standard` (the `propose_*` tools; the server sets the
  source to the client's name), or Coffer itself (`COFFER_SOURCE`, stored as `coffer`).
  `insert_suggestion` and `insert_report` are the only writes an MCP tool can make
  (`AGENTS.md` → Invariants). An agent's rule may use only `set_category`,
  `split_template`, `set_merchant` and `add_tag` ("M2: MCP server", E.4).
- **Once.** One pending suggestion per kind and canonical payload, from any source:
  filing it again returns the existing ID (`Filed::duplicate`). A source has at most
  `MAX_PENDING_PER_SOURCE` (200) pending.
- **Status.** `pending`, then `approved` or `rejected`; a decided suggestion stays
  decided (V13's trigger). "Approved" means "was applied": undoing its batch or deleting
  its rule leaves it approved (E.9). An approval can't be undone from the Inbox
  (decision record "Inbox", point 4).
- **Untrusted text.** Rationales, patterns, names, client names and bank text are shown
  as plain text through `envelope::shown` and acted on nowhere (decision record "Inbox",
  point 7). Nothing logs them.

### Approving and rejecting

`inbox::approve` and `approve_all`, `reject` and `reject_all`. Each call is one `BEGIN
IMMEDIATE` transaction: approving checks the payload again (`Payload::check`), applies it
through core's own APIs and marks it approved; on any error nothing is written. A list is
all or nothing, a repeated ID is refused, and a suggestion an earlier rule in the list
settled is skipped.

- **categorization**: one `bulk_edit` change batch, source `ai_approved`. It overwrites a
  category the user set (the preview counts those) and skips rows that take no category
  of their own (`category::assign`; "what a transaction can take, decided in core",
  point 3).
- **rule**: stored first in the list (`RuleOrigin::Inbox`), then applied to history only
  with `ApproveOptions::apply_to_history`, never over a category the user set (`user`
  or `ai_approved`): uncategorised rows and rows whose category came from a rule or a
  preset map change (decision record "Inbox impact preview", point 1). Like any rule it
  skips the category of rows that take none of their own ("rules skip rows that take no
  category of their own"). Its apply settles every pending categorisation whose every
  item names a transaction the apply gave that category; split-line items and partial
  overlaps stay pending ("Inbox impact preview", point 4; "M2: MCP server", E.7).
- **transfer_pair**: confirms detection's suggested group for exactly that pair, or
  creates a confirmed one; refused when either row is in a confirmed link.
- **merchant_alias**: stores the alias and recomputes names. Rules match the cleaned bank
  text, so a name changes no rule's matches ("Inbox impact preview", point 5).
- **refund_link**: a confirmed link of the payload's amount, or as much as both sides
  can still take.

No suggestion moves a balance, so none meets a lock; `ApproveError::Locked` is a
safeguard ("M2: MCP server", E.6, as amended). A transfer decided in Transactions settles
a pending `transfer_pair` suggestion for the same pair in the same DB transaction
(`decide_transfer`, `mark_transfer`): approved on confirm, rejected as
`DECIDED_IN_TRANSACTIONS` on reject ("transfer-pair confirmation (built)", point 8).

**Where.** Only in the app's Inbox and `coffer inbox approve|reject`, never over MCP.
The Inbox applies a rule to history (decision record "Inbox", point 2); the CLI does only
with `--apply-to-history`. `coffer inbox approve` needs an interactive terminal, and
`reject` doesn't ("approval needs the user's terminal", points 3 and 4).

### The impact preview

- **Counts and lists.** `inbox::impact` counts (`Impact`, `Detail`: IDs, counts and
  amounts, never text); `inbox::preview::preview` lists the rows approving would change,
  what each has now and after, the rows left alone because the user set them, the rule
  that decides a rule's matches today, and the categorisations a rule settles (decision
  record "Inbox", point 1). `coffer inbox show` prints the impact; neither is for MCP.
- **Computed when shown, by the approval itself.** Both run the approval as the Inbox
  makes it (a rule applied to history) in a rolled-back DB transaction
  (`change::dry_run`) against today's data, so they can't drift from what approving does
  ("Inbox impact preview", point 6). `Impact::blocked` and
  `RuleImpact::history_blocked` carry the approval's own refusal, and then no change is
  shown ("one spending rule over the ledger", point 4).
- **Wide rules.** A rule is wide (`RuleImpact::wide`) when its matches span more than
  `WIDE_MERCHANTS` (5) distinct `merchant_display` values, or it sets a spending category
  and a match is money in or in an income category (`moves_income`), not counting a
  linked refund's money in. A default, not an invariant
  ("Inbox impact preview", point 2; "the preview says why a rule skips a row's
  category", point 2). A wide rule is also summed by merchant
  (`ByMerchant`: the first `MERCHANTS_SHOWN` (3), the rest, the ones you set); its
  `total` is a gross listing sum.
- **List length.** Up to `LIST_ALL` (10) matches in full, else the newest `LIST_NEWEST`
  (6); `Preview::matches` says how many there are.
- **Spending changes** are the reports' own: the touched rows' ledger lines tallied
  before and after (`reporting::Tally`), the user's share, sided by `reporting::Side`
  (`reporting.md` → "Reporting"). A transfer's or refund link's effect is on the month
  of the money.
- **Skipped rows.** A row that takes no category of its own (a confirmed transfer leg,
  a linked refund's money in) is listed with why: `After::NoOwnCategory`, "Stays, a
  transfer" or "Stays, a linked refund", for a categorisation's target (counted in
  `CategorizationImpact::skipped`) and for a rule's match (counted by kind in
  `RuleImpact::skipped_transfers` and `skipped_refunds`; the inspector's Skips row).
  The reason wins over "Name or tags change" on that row ("the preview says why a rule
  skips a row's category").
- **Other reads.** `preview::describe` resolves what any suggestion names, decided ones
  too; `covered_by` finds the pending rule that would settle a categorisation;
  `touching_rule` lists pending agent rules over a stored rule (the Rules view).

### The Inbox screen

- **Reading.** `inbox::store::InboxStore` holds no connection: each call opens the
  database through `backup::gate::open`, which checks the restore lock. `Inbox::reload`
  reads on a background thread, all pending previews with one ledger read
  (`preview::preview_all`); the shell builds it with `Inbox::unloaded`. Approving,
  rejecting and `covered_by` still run on the UI thread, then reload (`app.md` →
  "performance at 100,000 transactions", points 1 and 2).
- **Groups.** One kind from one source (`GroupKey`), kinds in `KIND_ORDER`. A
  categorisation with several items is one row (decision record "Inbox", point 5).
  Approve all and Reject all act on a group, all or nothing; Approve all leaves out what
  `Entry::needs_own_approval` names: a wide rule, a suggestion approving would refuse, a
  rule whose apply to history would be refused, and one with no preview ("Inbox impact
  preview", point 3). Rejecting here records no reason.
- **Left panel**: INBOX (status), CHECKS (the review queues, `links-and-checks.md` →
  "review queues (built)"), KIND, SOURCE. A client is shown by its `--client` name;
  Coffer's own as "Coffer" "from your edits" (`text::source_name`).
- **Badge.** The tab counts pending suggestions from AI clients, not Coffer's own
  (`Inbox::badge`).
- **Colours.** `reporting::palette::category_palette` ranks category colours when read,
  with the Transactions register's ranking (decision record "Inbox", point 6).
- **After a decision** the bar says what changed, without Undo. "Show in Transactions"
  and "Edit in Rules…" aren't built (decision record "Inbox", point 9).

### Coffer's own suggestions

`inbox::own` ("M2: MCP server", E.8). `own::recategorize` is how the app and `coffer
categorize` set categories as the user: one `bulk_edit` batch, refusing a row that takes
no category of its own (`RecategorizeError::NoOwnCategory`). In the same DB transaction,
for each display merchant and category just set, it counts the unsplit rows that hold
that category with `category_source = user` now. At `RULE_SUGGESTION_THRESHOLD` (3), and
only when every counted row's bank merchant cleans to one text, it files `merchant is
<cleaned merchant>` → `set_category` from `coffer`, with a fixed rationale and no
merchant text. It files nothing when an identical suggestion is pending or was ever
decided, a stored rule has that definition, or the source is at its cap; the user's edit
never fails for it.

## Decision records

Oldest first.

### Decision record: Inbox impact preview (2026-11-10)

The open questions from the D6.6 designs (`DESIGN.md` → "Inbox impact preview"), decided
by the user on the designer's recommendation:

1. **Approving an agent's rule never changes a category the user set.** From the Inbox
   it applies only to uncategorised rows and rows with `category_source = rule`. Including
   the user's own rows is a choice made only in D6.7's apply-to-past preview ("Change
   these too"). The Inbox has no such toggle.
2. **A rule is wide when it spans more than 5 merchants or moves income.** That means its
   matches span more than 5 distinct `merchant_display` values, or any match would move
   income into a spending category. This is a default, not an invariant.
3. **Approve all always skips wide rules.** A wide rule needs its own approval with the
   preview open.
4. **A rule's approval settles the categorisations it covers.** Pending `categorization`
   suggestions for rows the rule changes are marked approved under the rule's
   `change_batch`, so the Inbox doesn't ask twice.
5. **Merchant names don't change which rules match.** Rules match the cleaned bank text
   before names apply (D6.7), so a name suggestion is cosmetic, and its preview says so.
6. **The impact is computed when shown.** It runs against today's data each time a
   suggestion is opened, so a later import or edit can't make a stale preview look
   smaller than the real change.

### Decision record: Inbox (2026-11-14)

M3.6 and M3.14. Marked **(user)**: decided by the user on 2026-11-14 when asked before
the build. Everything else follows `DESIGN.md` → "Inbox", "Inbox impact preview" and the
"Inbox impact preview" record above. No migration.

1. **The preview lists, core computes (`inbox::preview`).** `impact` counts; `preview`
   lists the rows approving would change (a rule's or a name's matches, a
   categorisation's items), what each has now and after, the rows left alone because the
   user set them, the spending each category gains or loses, the rule that decides a
   rule's matches today, the categorisations a rule settles, a wide rule summed by
   merchant (the first 3, the rest, the ones you set), and a transfer's or refund's
   effect on its month. It plans with the approval's own code in a rolled-back
   transaction, so the list can't drift from what approving does. Up to 10 matches are
   listed, else the newest 6. `describe` resolves what any suggestion names (decided ones
   too); `covered_by` finds the pending rule that would settle a categorisation.
2. **Approving a rule from the Inbox applies it to history.** As the D6.6 preview says
   ("Changes 5 past transactions"): `ApproveOptions { apply_to_history: true }`, never
   over a category the user set (point 1 above). The CLI keeps `--apply-to-history`
   opt-in.
3. **Approve all and Reject all are all or nothing**, per group (one kind from one
   source): `approve_all` and a new `reject_all` in one DB transaction. Approve all leaves
   out wide rules and any suggestion that can't be approved now. A suggestion a rule
   earlier in the list settled is skipped, as `approve_all` already does.
4. **No undo yet (user).** Undoing an approval would have to remove the rule, alias,
   link or transfer it made as well as revert its batch, and V13's trigger keeps a
   decided suggestion decided, so it needs a migration and its own item. After an
   approval the D6.3 bar says what changed, without Undo; the D6.6 copy that offers an
   undo is left out.
5. **A categorisation with several items is one row (user)**: "N transactions", opening
   in place like a rule with each transaction listed. Approving it approves every item,
   as core always has.
6. **Category colours are ranked when read (user).** `reporting::palette`: the six
   top-level spending groups with the most spent in the 12 months to today get
   `chart-1` … `chart-6`, their subcategories share the group's colour, the rest the
   "Other" marker. Not stored yet, so they can change as spending does; storing them on
   the category ("Data visualisation") is still to come. M3.3 and M3.6 were built in
   parallel with different rankings (per group in one currency for Transactions, per
   category across currencies here); at the merge (2026-11-14) the palette was moved
   onto the register's ranking (`register::ranked_groups`) so a category has one colour
   on both screens, and a test checks they agree.
7. **Untrusted text is plain text.** Rationales, patterns, names, client names and bank
   text go through `envelope::shown` (the CLI's stripping, moved to core): control,
   invisible and bidi characters removed, whitespace collapsed, cut to a length. GPUI
   draws text without markup or links, and nothing in it is acted on.
8. **The app holds no connection (as M3.2).** `inbox::store::InboxStore` opens the
   database per call through `backup::gate`, which checks the restore lock. The pending list is read with every
   preview when the Inbox opens and after each decision, on the main thread (decision
   record "Inbox impact preview", point 6). The tab's badge counts pending suggestions
   from AI clients; Coffer's own aren't counted.
9. **Left for later (user).** D6.4's CHECKS section (M3.12), "Show in Transactions"
   (needs M3.3's filters) and "Edit in Rules…" (M3.7, M3.13) aren't drawn. Client display
   names come with the AI access settings (M3.15); until then the Inbox shows the
   `--client` name.

### Decision record: the preview says why a rule skips a row's category (2026-11-19)

Review finding R.18, decided by the lead from the R.10 records ("rules skip rows that
take no category of their own", `rules.md`); point 2 confirmed by the user. Drawn in Paper on the states board
(`DESIGN.md` → "Inbox impact preview" → "Rows a rule leaves alone"). No migration.

1. **The preview says why.** A rule's impact counts the matches whose category it skips,
   by kind (`RuleImpact::skipped_transfers`, `skipped_refunds`, from
   `ApplyPreview::skipped_categories`); "If you approve" shows them as a Skips row after
   Keeps, and the caption says a confirmed transfer or a linked refund takes no category
   of its own. In the open preview such a row is `After::NoOwnCategory` ("Stays, a
   transfer", "Stays, a linked refund"), for a rule's matches and a categorisation's
   targets alike; a categorisation's skipped target was `After::Unchanged` before. A
   split row is still `After::SplitSkipped`, counted in `split_rows`.
2. **A linked refund doesn't make a rule wide; a transfer leg still does.**
   `moves_income` leaves out a linked refund's money in that a `set_category` rule
   skips: a rule over a merchant is expected to match that merchant's refunds, and a
   refund not linked yet that the rule categorises at import lowers that spending, as a
   refund should. Before, such a rule was wide, left out of Approve all, for a row it
   never changes. A confirmed transfer's incoming leg still counts although the rule
   skips it: rules run before transfer detection at import (`rules.md` → "At import"),
   so the next such leg gets the rule's spending category until its pair is confirmed,
   and a pattern that matches a transfer is broader than it looks. The Skips row says
   which kind it is. Merchants are still counted over every match. A `split_template`
   rule's skips (`ApplyPreview::skipped_splits`: also suggested pairs and other links)
   are neither shown nor discounted yet.
