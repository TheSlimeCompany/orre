# Coffer — Architecture: Categories

The category tree, the categories editor, and which rows take a category.

Part of [`ARCHITECTURE.md`](../ARCHITECTURE.md), which lists every topic and
decision record. A section or record named here that isn't in this file is in
that list.

## Categories

`coffer_core::category` (M1.14; decisions in "Decision record: categories"). Every new
database starts with this two-level set (`V2__default_categories.sql`,
`DEFAULT_CATEGORIES`). Groups are in display order; a transaction can be filed under a
group itself (`transfer`, `other`) as well as under a subcategory.

| Group (kind) | Subcategories |
|---|---|
| `income` Income (income) | `salary` Salary · `self_employment` Self-employment · `benefits` Benefits · `pension` Pension · `interest` Interest · `investments` Investment income · `rental` Rental income · `gifts` Gifts received |
| `home` Home | `rent` Rent · `mortgage` Mortgage · `property_tax` Council tax · `maintenance` Repairs and maintenance · `household` Household |
| `bills` Bills | `energy` Energy · `water` Water · `phone` Phone · `internet` Internet · `tv` TV · `insurance` Insurance · `subscriptions` Subscriptions |
| `food` Food | `groceries` Groceries · `eating_out` Eating out · `takeaway` Takeaways |
| `transport` Transport | `public` Public transport · `taxi` Taxis · `fuel` Fuel · `parking` Parking and tolls · `vehicle` Vehicle costs |
| `shopping` Shopping | `clothing` Clothing · `electronics` Electronics |
| `entertainment` Entertainment | `events` Events and tickets · `hobbies` Hobbies |
| `health` Health | `medical` Medical · `fitness` Fitness · `personal_care` Personal care |
| `travel` Travel | `flights` Flights · `accommodation` Accommodation |
| `family` Family | `childcare` Childcare · `children` Children · `education` Education · `pets` Pets |
| `giving` Giving | `gifts` Gifts · `charity` Charity |
| `finance` Finance | `fees` Fees and charges · `interest` Interest paid · `loans` Loan repayments · `tax` Tax |
| `cash` Cash | — (cash withdrawals; with a cash account, the withdrawal is a transfer instead) |
| `other` Other | — |
| `transfer` Transfers (transfer) | `savings` Savings · `investments` Investments · `credit_card` Credit card payments |

A subcategory's key is `<group>.<key>` (`food.groceries`); groups not marked are
`spending`. `transfer` is for money between the user's own accounts, or to one Coffer
doesn't see: like a confirmed transfer pair, it is neither income nor spending.

- `CategoryKind` is the stored kind; a test keeps its keys equal to the schema's CHECK.
- `Database::categories` returns a `CategoryTree`: groups with their subcategories in
  the order they were created (the defaults first, in the order above), looked up by
  key (`get`, `group_of`). `CategoryTree::load` takes a `&Connection`, so code that
  writes in a transaction can map keys to IDs inside it.
- `CategoryKey` and `CategoryKind` are SQLite `TEXT`; a stored value that doesn't parse
  is an error, not a guess.
- Editing categories (add, rename, delete, hide) is the Rules view's Categories section
  (M3.7; "Decision record: the categories editor (built)"). A hidden category
  (`category.hidden`, V18) keeps its rows and history but isn't offered for new ones.

## Decision records

Oldest first.

### Decision record: categories (2026-11-11)

M1.14, the default category set. The set itself is under "Categories" above.

1. **The defaults are seeded by a migration** (`V2__default_categories.sql`), not by Rust
   at every open. A migration runs once, in the same transaction as the schema, and is
   checksummed like any other, so a default the user deleted never comes back and
   `coffer mcp`, which never writes the schema, needs nothing extra.
   `coffer_core::category::DEFAULT_CATEGORIES` is the same list in Rust (a test keeps
   them equal) for code that has no database, such as the bundled presets' tests.
2. **A category has a kind: `spending`, `income` or `transfer`** (`category.kind`).
   Reports need to know which side a category is on, and so does the "moves income into
   a spending category" check on wide rules (Inbox impact preview, point 2). A
   subcategory has its group's kind, and a kind never changes, since changing it would
   move a category's whole history between income and spending. Changing it means a new
   category and moving the rows (an undoable change batch). A new category without a
   kind is `spending`.
3. **Keys are forever; labels belong to the user.** Renaming changes the label only (the
   V1 trigger already makes `key` and `parent_id` read-only), so presets, rules, budgets
   and MCP clients keep working. Default labels are English (en-GB) and keys name the
   concept, not a country's word for it: `home.property_tax` is labelled "Council tax",
   and an en-US label would say "Property tax". Translating default labels (until the
   user renames one) can key off `DEFAULT_CATEGORIES`.
4. **User categories share the key space.** A category the user adds (M3.7) gets its key
   from its label when it's created (lowercase ASCII letters and digits, other runs as
   `_`, a numeric suffix if taken, a fallback when nothing is left), and the key doesn't
   follow later renames. It can be a new group or go under any group, taking the group's
   kind. A later Coffer that adds defaults inserts each key only where it's free, so a
   user's category with the same key is kept as it is.
5. **Defaults are ordinary rows.** The user can rename them, and can delete one that
   nothing uses. Deleting is refused while a transaction, split line, budget or
   schedule points at it (foreign keys) or a rule action or preset names its key (core
   checks rules; an unmatched preset key just leaves rows uncategorised). Hiding a used
   category instead of deleting it is for M3.7 to design.
6. **No category for "uncategorised", refunds or reimbursements.** Uncategorised and
   split rows have no `category_id`. A linked refund takes its purchase's category and
   a reimbursement nets against its expense (`transaction_link`); money in under a
   spending category lowers that category's spending and isn't income.
7. **Bundled presets map only to default keys**, checked by a test in
   `coffer_import::preset`. Loading a preset doesn't check keys against the defaults,
   since a preset the user saves (M3) may name their own categories; the import maps a
   key that isn't in the database to nothing and leaves the row for rules or the user.

### Decision record: the categories editor (built) (2026-11-15)

M3.7's categories half. Follows "Decision record: categories" above (kinds never change,
keys are forever, labels belong to the user, deleting is refused while used). Marked
**(user)**: decided by the user on 2026-11-15; **(composed)**: decided while building.

1. **It's the Rules view's fourth section, Categories**, composed from the Rules view's
   specs, not drawn (user).
2. **A used category can be hidden** (`category.hidden`, migration
   `V18__category_hidden.sql`): it leaves the menus that set a category (the rule editor,
   the Transactions row menu, inspector, split lines and bulk bar, the new-transaction
   dialog) but keeps its transactions, history and reports, and stays in the Category
   filter and the export (user). `CategoryTree::visible`/`is_visible` serve pickers;
   `iter` and `groups` still return everything. MCP's `list_categories` lists hidden
   categories with `hidden: true` and asks agents not to propose them (composed, lead).
3. **Hiding a group** hides its subcategories in pickers, each keeping its own flag, so
   showing the group again restores them as they were (composed).
4. **Keys** come from the label once, when the category is added: lowercase ASCII letters
   and digits, other runs as one `_`, `custom` when nothing is left, `_2`, `_3`, … while
   taken. "Taken" means a category has that key now, so a deleted category's key is free
   again (composed).
5. **Names** are trimmed, can't be blank, and are unique among the categories beside them,
   ignoring case (composed).
6. **Deleting** is refused while transactions, split lines, budgets, schedules, rule
   actions or pending Inbox suggestions (a categorisation's items, a suggested rule's
   `set_category` or `split_template` lines) use the category (`Error::InUse`, with the
   counts), and for a group with subcategories (`Error::HasSubcategories`) (composed).
   *Changed 2026-11-15 (review follow-up):* pending suggestions didn't count at first, so
   deleting left one that could only fail as an unknown key. Counting them, rather than
   rejecting them as part of the delete, keeps the decision with the user: approving one
   puts the category to use, and a categorisation's other items may still be wanted.
   Settling, as `inbox::decide_transfer` does, fits only when the user's action answers
   the whole suggestion. When suggestions are all that use it, the refusal says to
   approve or reject them in the Inbox instead of "hide it".
7. **Add, rename, hide and delete apply at once**, with no change batch: no transaction
   changes (composed).
8. **The table's TRANSACTIONS column** counts transactions plus split lines in the
   category; the inspector lists the six counts separately (composed).
9. **The delete dialog** is opened by the Rules view itself, not the shell (composed).
10. **Supersedes** "Decision record: the Rules view (built)", point 10, and the "for M3.7
    to design" in "Decision record: categories", point 5.

### Decision record: what a transaction can take, decided in core (2026-11-16)

From the same architecture review. **(user)** marks the user's choices, made when they
were asked.

1. **A transaction takes a category of its own unless it is split, a leg of a confirmed
   transfer pair, or the money in of a confirmed refund** (whose category follows its
   purchase). This was the table's rule (decision record "the Transactions view", point
   3, and "refunds, reimbursements and settlements (built)", point 5). It is now core's
   `category::assign`: one function over three facts. The database and the
   Transactions register each read the facts, and a test runs the same rows through
   both. A suggested pair or a suggested refund doesn't count.
2. **Core refuses (user).** `inbox::own::recategorize` (the table, the inspector, the
   bulk-action bar and `coffer categorize`) refuses such a row with
   `RecategorizeError::NoOwnCategory`, naming the transaction ID and why, never its
   text, and writes nothing. It refuses even when the row already has that category.
   The rejected option was core only reporting the rule, which would have left the CLI
   and approved AI suggestions able to categorise a transfer leg or a linked refund.
3. **Approving a categorisation skips such rows.** The other targets are still set. The
   impact counts them separately (`CategorizationImpact::skipped`), not as missing,
   because missing blocks approval. The preview lists each one as "No change". A
   suggestion whose targets are all skipped is approved with no change batch. Split
   targets are still refused whole by the payload check. The Inbox's "If you approve"
   panel doesn't say why a row was skipped yet; that needs a design in Paper first.
4. **Other writers don't ask.** Rules at import and applied to history, duplicate
   merges, removing a split and undo write what is theirs to decide; `change::unwritable`
   still refuses only split rows. A rule applied to history can still set a category on
   a confirmed transfer leg or a linked refund's money in. Transfers don't count in the
   ledger, and a linked refund counts in its purchase's category, so what is reported
   doesn't change. Whether rules should skip those rows, as they skip split rows, is
   left open.
   *Amended 2026-11-19: rules now skip the category of those rows, as they skip split
   rows, and their other actions still apply. See "two kinds of transfer, and marking a
   suggested pair" in `transactions.md`, and `rules.md`, "Decision record: rules skip
   rows that take no category of their own".*
5. **Which inspector sections a row gets is core's too** (`RegisterRow::takes_share`:
   money out outside a transfer; `takes_links`: any non-zero amount outside a transfer).
   The meaning is unchanged. Neither is unified with `reporting::linking`'s
   `can_be_purchase` or `kinds_from`, which are narrower: a duplicate-excluded or
   over-the-cap row shows the Links section, but the dialog offers no kinds (user,
   scope).
6. **Left in the app:** an excluded row isn't categorised in place. The share sign and
   "half each" default, which side of a link is the money in, and why a transfer was
   suggested move later (user, scope).
