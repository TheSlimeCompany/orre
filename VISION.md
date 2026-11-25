# Vision

## Charter

Orre is the place you keep the financial truth on your own machine, and a supervised way to let an AI you already trust help you understand it. Industry-leading personal finance is not the app that holds the most bank logins; it is the one that gives you insight without surrendering the ledger.

## The problem

Money tools ask people to choose between convenience and ownership. Cloud apps import automatically, and increasingly offer an AI that has already seen the feed, but the ledger lives on someone else's computer. Spreadsheets and plain-text ledgers keep the books close, but leave insight as homework. People should not have to give the feed away to ask a serious question of their money.

## Who it is for

Orre is for privacy-minded individuals in the United Kingdom, the United States, and the euro area — including one person keeping a household's books — who will export from their bank and may already use an AI client such as Claude or Codex. Generic CSV and OFX/QFX import is meant to work with any bank; presets make a known bank's export one step. It is also for developers who want an inspectable, scriptable ledger.

It is not for businesses needing an accounting suite, for people who will only accept automatic bank sync, or for anyone who wants an AI to move money or write the ledger unsupervised.

## What Orre is

Orre is a local-first personal finance desktop application. You import bank exports (CSV, OFX, QFX) into a SQLite database on your machine. Imports are deterministic: preview, deduplication, reconciliation, and undo are part of the product, not afterthoughts.

A Model Context Protocol (MCP) server can give a client you choose supervised access to that ledger. The server is off until you enable a client. Access levels limit what a client is handed. Tools are read-mostly: an assistant may analyse, save reports, and file suggestions. Reports are saved for you to read and never change the ledger. Suggestions change it only once you approve them. Access levels are a guardrail, not a hard boundary: anything that can read the database file can read your finances. SECURITY.md says plainly what is and is not protected, and we do not dress it up.

## Principles

These are ranked. When two of them pull in different directions, the earlier one wins unless we deliberately change this document.

1. **The ledger is sacred.** Correct, inspectable numbers come before clever features. Imports are deterministic; writes are attributable; undo exists. No model runs in the import path. If model-assisted extraction (of PDF statements, say) is ever offered, it is opt-in, runs before import, and its output is previewed and accepted like any other file before the deterministic import reads it. Work that can mutate or lose financial data is high criticality, and is treated as such.

2. **Local is the source of truth.** The copy on your machine is authoritative. No account is required, and there is no telemetry. The opt-in ECB exchange-rate fetch is the only network call Orre may make; any other needs a recorded architecture decision first.

3. **AI may study your books only as a guest, and only under your supervision.** You remain the author of your financial history. An assistant may read within the access level you set, save reports, and propose changes; anything that would change the ledger needs your approval. Supervision is staged: insight, then suggestion, then approved write. Reading has a cost too: a client on a hosted model sends what it reads to its provider, and at `standard` that includes names and bank text. `aggregate` sends totals and your category labels, which can be personal if you wrote them yourself; `pseudonymous` sends rows with names as tokens, though amounts and dates can still identify a merchant. Only a local model keeps what it reads on your machine.

4. **Data should outlast the app.** The working store is SQLite, not a lock-in format. The schema is documented. Export and backup are first-class. If the project stopped, a determined person should still be able to read and move their history.

5. **Honesty over reassurance.** We describe what the software does and what it does not protect. We do not imply a hard security boundary where we have only a guardrail, and we do not claim protections we have not shipped.

6. **A calm tool, not a coach with an agenda.** The job is to help people see clearly and decide for themselves. We do not gamify, upsell, or moralise. The numbers are the interface: boring and correct beats clever.

7. **Useful without AI, better with it.** Import, reconciliation, and analytics have to stand on their own. The MCP layer is a multiplier, not a crutch, and not an excuse for a weak monthly workflow.

For example, a model reading PDF statements would be convenient, and principle 7 welcomes AI. Principle 1 outranks it, so the model may only produce a file you preview before a deterministic import; principle 2 outranks it too, so a hosted model would need its own architecture decision first.

## Non-goals

Unless we change this vision on purpose, Orre will not:

- Connect to banks: no bank aggregation, bank sync, bank credentials, aggregation provider, cloud service, or Orre account.
- Sync the ledger through a cloud service, or keep a copy of it on anyone else's server. Any sync between your own devices would have to be local or peer-to-peer, with the file on your machine as the source of truth; none is promised.
- Become a payments, trading, or bill-pay application.
- Replace a financial adviser, tax accountant, or solicitor.
- Become a business ERP, payroll, or tax-filing suite.
- Let an AI act on a bank account or move money, with or without approval. An AI may change the ledger only through suggestions you approve.
- Become a social product, a marketplace, or a vehicle for financial advertising.

## What success looks like in three to five years

In a few years, a privacy-minded person in the UK, the US, or the euro area should be able to answer a serious question about their money in a calm local app they own, with an optional assistant that can look, and save reports, but cannot touch the ledger until they say so. Orre exists to make that ordinary.

The questions we mean are ones of understanding and planning: where the cash went this quarter, whether a purchase fits the next three months, what a short runway looks like. We do not mean unsupervised advice, tax filing, or moving money.

This section is the measurable end state: what the product looks like, and what it does, when we have succeeded. We compare current work against it. The following are targets, not a description of the product today. Each target is written so we can say, plainly, whether we have met it or not. If we cannot pass or fail a target, it is not yet measurable enough, and we should tighten the wording rather than leave it as a mood. A "to be set" placeholder must be filled before its target counts.

- **First overview, fast.** A new user goes from installing a release to a reconciled first month and its overview in under [N minutes, to be set], with no AI client and no network connection.
- **Import that covers the audience.** Orre ships tested presets for at least [N, to be set] banks in each of the UK, the US, and the euro area, named in ROADMAP.md, and each imports through preview, deduplication, and reconciliation without writing code.
- **History that opens elsewhere.** A full export loads into Beancount and hledger and passes their own checks.
- **Strangers use it.** At least [N, to be set] people outside the project have installed a release and imported their own bank's export by following the README alone, and said so in an issue or discussion.
- **A small, serious commons.** At least [N, to be set] bank presets, importers, or MCP tools from people outside the project have been merged. Each passed the project's own review: an MCP tool passes the per-tool access-level and PII tests, and one-way doors get a hostile review.
- **Honesty that has kept pace.** Encryption at rest has shipped. SECURITY.md still states, without softening, that access levels are a guardrail, not a hard boundary against a local process.

Some things must stay true on the way; breaking one is a regression, not progress. The monthly cycle of import, reconciliation, and overview needs no AI and no network. Asking an AI about your books needs no Orre account and no bank aggregator. Reports never change the ledger; suggestions wait for approval. The schema is documented, exports are open formats, and backups are plain SQLite files, so the history outlives the project.

## How we use this file

This file is the destination and the decision filter. ROADMAP.md holds the sequence and the status. ARCHITECTURE.md and SECURITY.md hold the current design and the threat model. README.md points here.

When planning, or when an agent is doing the work, start from this prompt: look at this vision, look at where we are now, and what is the next step that gets us closer?

Use that prompt whenever we are deciding what to do next: a planning session, a new piece of work, or a moment when we are stuck. The answer should be a short, written list of the next steps that would close the gap. A human accepts that list before the work starts. The vision is the end state; the list is the next move toward it.

A proposal should name the principle it serves, and should not violate a non-goal. Prefer work that makes the ledger more trustworthy, the import more complete, the monthly workflow calmer, or the supervised AI path clearer. Reject work that weakens those things, even if it is impressive.

When the work is finished, check the result against this file in the same way: does it move us toward the success picture, and does it still fit the charter and principles? If it does not, it is not done.

Revisit this document when a principle would have to change — for example, if we ever decided to take on bank synchronisation. Do not update it to track milestone status.
