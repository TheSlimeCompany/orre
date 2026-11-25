# Vision

## Charter

Orre is the place you keep the financial truth on your own machine, and a supervised way to let an AI you already trust help you understand it. Industry-leading personal finance is not the app that holds the most bank logins; it is the one that gives you insight without surrendering the ledger.

## The problem

Money tools have long asked people to choose between convenience and ownership. Cloud apps import transactions automatically and, increasingly, offer an AI that has already seen the feed. In return, the ledger lives on someone else's computer. That company can change the product, close it, or invite you into something else. A careful person is right to hesitate before connecting a bank account to a chatbot, or before treating a hosted finance app as the long-term home of their history.

The other path — spreadsheets, plain-text ledgers, self-hosted servers — keeps the books close, but often leaves insight as homework. Many people already want to ask a serious question of their money. They should not have to give the feed away to do it.

## Who it is for

Orre is for privacy-minded individuals and households in the United Kingdom, the United States, and the European Union who will export from their bank, who want a calm local desktop app, and who may already use an AI client such as Claude or Codex.

It is also for open-source users and developers who want an inspectable, scriptable ledger and a clear place to add bank import presets.

It is not for businesses or bookkeepers who need a full accounting suite; for people whose only acceptable workflow is automatic bank synchronisation; or for anyone who wants an AI to move money, or to write the ledger unsupervised.

## What Orre is

Orre is a local-first personal finance desktop application. You import bank exports (CSV, OFX, QFX) into a SQLite database on your machine. Imports are deterministic: preview, deduplication, reconciliation, and undo are part of the product, not afterthoughts. There is no Orre account, no bank credentials, and no requirement to talk to a network in order to use your books.

A Model Context Protocol (MCP) server can give a client you choose supervised access to that ledger. The server is off until you enable a client. Access levels limit what a client is handed. Tools are read-mostly: an assistant may analyse, and it may file suggestions, but those suggestions wait in an approval inbox. Access levels are a guardrail, not a hard boundary. Anything that can read the database file can read your finances. In v1 the database is not encrypted at rest. We say this plainly here and in SECURITY.md, and we do not dress it up.

## Principles

These are ranked. When two of them pull in different directions, the earlier one wins unless we deliberately change this document.

1. **The ledger is sacred.** Correct, inspectable numbers come before clever features. Imports are deterministic; writes are attributable; undo exists. Work that can mutate or lose financial data is high criticality, and is treated as such.

2. **Local is the source of truth.** The copy on your machine is authoritative. The network is optional. No account is required. Any future network use (for example an opt-in rate fetch) stays explicit, narrow, and off by default.

3. **AI may study your books only as a guest, and only under your supervision.** You remain the author of your financial history. An assistant may read within the access level you set, and it may propose changes. It does not silently alter the books. Supervision is staged: insight, then suggestion, then approved write.

4. **Data should outlast the app.** The working store is SQLite, not a lock-in format. The schema is documented. Export and backup are first-class. If Orre the application disappeared, a determined person should still be able to read and move their history.

5. **Honesty over reassurance.** We describe what the software does and what it does not protect. We do not imply a hard security boundary where we have only a guardrail, and we do not claim protections we have not shipped.

6. **A calm tool, not a coach with an agenda.** The job is to help people see clearly and decide for themselves. We do not gamify, upsell, or moralise. The numbers are the interface: boring and correct beats clever.

7. **Useful without AI, better with it.** Import, reconciliation, and analytics have to stand on their own. The MCP layer is a multiplier, not a crutch, and not an excuse for a weak weekly workflow.

## Non-goals

Unless we change this vision on purpose, Orre will not:

- Require a cloud service, an Orre account, bank credentials, or an aggregation provider.
- Become a payments, trading, or bill-pay application.
- Replace a financial adviser, tax accountant, or solicitor.
- Become a business ERP, payroll, or tax-filing suite.
- Let an AI write the ledger, or act on a bank account, without approval.
- Become a social product, a marketplace, or a vehicle for financial advertising.

Multi-device synchronisation and encryption at rest may appear on the roadmap. Until they exist, they are not described as capabilities.

## What success looks like in three to five years

In a few years, a privacy-minded person in the UK, the US, or the EU should be able to answer a serious question about their money in minutes — in a calm local app they own, with an optional assistant that can look but cannot touch until they say so. Orre exists to make that ordinary.

The questions we mean are ones of understanding and planning: where the cash went this quarter, whether a purchase fits the next three months, what a short runway looks like. We do not mean unsupervised advice, tax filing, or moving money.

This is the end state we compare our current work against. The following are targets, not a description of the product today.

- **A weekly ritual that needs no AI.** Someone can import last month's export, finish reconciliation, and read a clear monthly overview without enabling any AI client.
- **A supervised question that needs no cloud ledger.** That same person can ask the AI they already use a planning question, grounded in their local books, without creating an Orre account and without granting a bank aggregator access. Suggestions appear in the approval inbox and change nothing until they are accepted.
- **Import that covers the audience.** File import is genuinely useful across UK, US, and EU banks — not a single preset — with deterministic preview, deduplication, reconciliation, and undo.
- **A ledger that survives us.** The schema is documented, exports and backups are ordinary, and if the Orre organisation and every connected model vanished, the history would still be readable and movable.
- **Honesty that has kept pace.** Encryption at rest has shipped. SECURITY.md still states, without softening, that access levels are a guardrail, not a hard boundary against a local process.
- **A small, serious commons.** Other people can add bank presets and tools without forking the ledger, and Orre is a straightforward recommendation when the question is: insight on my money, without surrendering the ledger.

## How we use this file

This file is the destination and the decision filter. ROADMAP.md holds the sequence. ARCHITECTURE.md and SECURITY.md hold the current design and the threat model. README.md points here.

When planning, or when an agent is doing the work, start from this prompt: look at this vision, look at where we are now, and what is the next step that gets us closer?

A proposal should name the principle it serves, and should not violate a non-goal. Prefer work that makes the ledger more trustworthy, the import more complete, the weekly workflow calmer, or the supervised AI path clearer. Reject work that weakens those things, even if it is impressive.

Revisit this document when a principle would have to change — for example, if we ever decided to take on bank synchronisation. Do not update it to track milestone status.
