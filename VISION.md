# Vision

## Charter

Orre is the place you keep the financial truth on your own machine, and a supervised way to let an AI you already trust help you understand it. Industry-leading personal finance is not the app that holds the most bank logins; it is the one that gives you insight without surrendering the ledger.

## The problem

Money tools have long asked people to choose between convenience and ownership. Cloud apps import transactions automatically and, increasingly, offer an AI that has already seen the feed. In return, the ledger lives on someone else's computer. That company can change the product, close it, or invite you into something else. A careful person is right to hesitate before connecting a bank account to a chatbot, or before treating a hosted finance app as the long-term home of their history.

The other path — spreadsheets, plain-text ledgers, self-hosted servers — keeps the books close, but often leaves insight as homework. Many people already want to ask a serious question of their money. They should not have to give the feed away to do it.

## Who it is for

Orre is for privacy-minded individuals and households in the United Kingdom, the United States, and the European Union who will export from their bank, who want a calm local desktop app, and who may already use an AI client such as Claude or Codex. Generic CSV and OFX/QFX import works with any bank today; broad UK, US, and EU preset coverage is a target, not the current state.

It is also for open-source users and developers who want an inspectable, scriptable ledger and a clear place to add bank import presets.

It is not for businesses or bookkeepers who need a full accounting suite; for people whose only acceptable workflow is automatic bank synchronisation; or for anyone who wants an AI to move money, or to write the ledger unsupervised.

## What Orre is

Orre is a local-first personal finance desktop application. You import bank exports (CSV, OFX, QFX) into a SQLite database on your machine. Imports are deterministic: preview, deduplication, reconciliation, and undo are part of the product, not afterthoughts. There is no Orre account, no bank credentials, and no requirement to talk to a network in order to use your books.

A Model Context Protocol (MCP) server can give a client you choose supervised access to that ledger. The server is off until you enable a client. Access levels limit what a client is handed. Tools are read-mostly: an assistant may analyse, save reports, and file suggestions for an approval inbox. Reports never change the ledger, and nothing that would happens without your approval. Access levels are a guardrail, not a hard boundary. Anything that can read the database file can read your finances. In v1 the database is not encrypted at rest. We say this plainly here and in SECURITY.md, and we do not dress it up.

## Principles

These are ranked. When two of them pull in different directions, the earlier one wins unless we deliberately change this document.

1. **The ledger is sacred.** Correct, inspectable numbers come before clever features. Imports are deterministic; writes are attributable; undo exists. Work that can mutate or lose financial data is high criticality, and is treated as such.

2. **Local is the source of truth.** The copy on your machine is authoritative. The network is optional. No account is required. Any future network use (for example an opt-in rate fetch) stays explicit, narrow, and off by default.

3. **AI may study your books only as a guest, and only under your supervision.** You remain the author of your financial history. An assistant may read within the access level you set, save reports, and propose changes; anything that would change the ledger needs your approval. It does not silently alter the books. Supervision is staged: insight, then suggestion, then approved write.

4. **Data should outlast the app.** The working store is SQLite, not a lock-in format. The schema is documented. Export and backup are first-class. If Orre the application disappeared, a determined person should still be able to read and move their history.

5. **Honesty over reassurance.** We describe what the software does and what it does not protect. We do not imply a hard security boundary where we have only a guardrail, and we do not claim protections we have not shipped.

6. **A calm tool, not a coach with an agenda.** The job is to help people see clearly and decide for themselves. We do not gamify, upsell, or moralise. The numbers are the interface: boring and correct beats clever.

7. **Useful without AI, better with it.** Import, reconciliation, and analytics have to stand on their own. The MCP layer is a multiplier, not a crutch, and not an excuse for a weak weekly workflow.

## Non-goals

Unless we change this vision on purpose, Orre will not:

- Require a cloud service, an Orre account, bank credentials, or an aggregation provider.
- Sync the ledger through a cloud service, or keep a copy of it on anyone else's server.
- Become a payments, trading, or bill-pay application.
- Replace a financial adviser, tax accountant, or solicitor.
- Become a business ERP, payroll, or tax-filing suite.
- Let an AI write the ledger, or act on a bank account, without approval.
- Become a social product, a marketplace, or a vehicle for financial advertising.

Encryption at rest is on the roadmap, but until it ships it is not described as a capability. Multi-device synchronisation is not planned; any future sync would have to be local or peer-to-peer, with the file on your machine as the source of truth.

## What success looks like in three to five years

In a few years, a privacy-minded person in the UK, the US, or the EU should be able to answer a serious question about their money in minutes — in a calm local app they own, with an optional assistant that can look, and save reports, but cannot touch the ledger until they say so. Orre exists to make that ordinary.

The questions we mean are ones of understanding and planning: where the cash went this quarter, whether a purchase fits the next three months, what a short runway looks like. We do not mean unsupervised advice, tax filing, or moving money.

This section is the measurable end state: what the product looks like, and what it does, when we have succeeded. We compare current work against it. The following are targets, not a description of the product today. Each target is written so we can say, plainly, whether we have met it or not. If we cannot pass or fail a target, it is not yet measurable enough, and we should tighten the wording rather than leave it as a mood.

- **A weekly ritual that needs no AI.** Someone can import last month's export, finish reconciliation, and read a monthly overview, with no AI client enabled and no network connection.
- **A supervised question that needs no cloud ledger.** That same person can ask the AI they already use a planning question, grounded in their local books, without creating an Orre account and without granting a bank aggregator access. Reports it saves change nothing in the ledger; its suggestions wait in the approval inbox until they are accepted.
- **Import that covers the audience.** A person in the UK, the US, or the EU can import a typical bank export for their country, using a preset or a documented CSV mapping, and finish preview, deduplication, and reconciliation without writing code.
- **A ledger that survives us.** The schema is documented, exports are in open formats that other tools read, and backups are plain SQLite files. If the Orre organisation and every connected model vanished, the history could still be read and moved with only that documentation and standard tools.
- **Honesty that has kept pace.** Encryption at rest has shipped. SECURITY.md still states, without softening, that access levels are a guardrail, not a hard boundary against a local process.
- **A small, serious commons.** Someone other than the original author can add a bank preset or a tool without forking the ledger, following documented extension points. When a careful person asks how to get insight on their money without surrendering the ledger, Orre is a recommendation we would stand behind.

## How we use this file

This file is the destination and the decision filter. ROADMAP.md holds the sequence. ARCHITECTURE.md and SECURITY.md hold the current design and the threat model. README.md points here.

When planning, or when an agent is doing the work, start from this prompt: look at this vision, look at where we are now, and what is the next step that gets us closer?

Use that prompt whenever we are deciding what to do next: a planning session, a new piece of work, or a moment when we are stuck. The answer should be a short, written list of the next steps that would close the gap. A human accepts that list before the work starts. The vision is the end state; the list is the next move toward it.

A proposal should name the principle it serves, and should not violate a non-goal. Prefer work that makes the ledger more trustworthy, the import more complete, the weekly workflow calmer, or the supervised AI path clearer. Reject work that weakens those things, even if it is impressive.

When the work is finished, check the result against this file in the same way: does it move us toward the success picture, and does it still fit the charter and principles? If it does not, it is not done.

Revisit this document when a principle would have to change — for example, if we ever decided to take on bank synchronisation. Do not update it to track milestone status.
