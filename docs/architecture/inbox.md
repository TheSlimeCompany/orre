# Coffer — Architecture: Inbox

Suggestions waiting for the user: the Inbox and its impact preview.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

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
