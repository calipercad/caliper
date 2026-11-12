# Licensing & the open-core model

Caliper is **open core**: a fully functional, permissively licensed core, with
the option to offer premium and hosted features on top under separate terms.

## The core — Apache-2.0

Everything in this repository is licensed under the **Apache License 2.0**
([`LICENSE`](LICENSE)). You can use, modify, self-host, and redistribute it,
including commercially, at no cost. Apache-2.0 was chosen over MIT because it
adds two things that matter once money and trademarks are involved:

- an **explicit patent grant** from every contributor (and a patent-retaliation
  clause), reducing patent risk for users and the project; and
- explicit **trademark** handling — the license does not hand out rights to the
  "Caliper" name or logo (see `NOTICE`).

## Premium / commercial — separate terms

Features and services that cost real money to run or that target commercial
users may be offered **outside** this repository under separate commercial
licenses. Current direction (subject to change):

- **Caliper Cloud** — a hosted, metered service: headless/CI rendering, managed
  text-to-CAD, shared BOM/assemblies, and an SLA. This is the primary paid
  surface, because it has a real server cost behind it.
- **Commercial / Pro licenses** — optional paid licenses for organizations
  (e.g. per-seat commercial use, or a one-time Pro tier), while personal use of
  the open-source core stays free.

Local rendering in the open-source CLI is **not** metered or gated — there is no
server cost to justify charging for it, and gating it would just invite forks.
The AI copilot uses *your own* Anthropic API key, so those tokens are yours.

## Why the CLA

Because the core is Apache-2.0 but premium editions may be proprietary, the
Project needs the right to distribute contributed code under **both** licenses.
The Contributor License Agreement ([`CLA.md`](CLA.md)) grants exactly that: you
keep ownership of your contribution and grant the Project a license (including
the right to relicense) so it can ship the code in the open-source core and in
commercial editions. This is the same mechanism used by many open-core projects.

## Trademark

"Caliper" and the Caliper logo are project trademarks. Apache-2.0 does not grant
trademark rights (section 6). You may truthfully say your product "works with
Caliper" or "is based on Caliper," but please don't name a fork or a competing
product "Caliper" in a way that implies endorsement.

---

> **Not legal advice.** This document and the accompanying `LICENSE`, `NOTICE`,
> `CLA.md`, and `CONTRIBUTING.md` are standard open-source scaffolding provided
> for convenience. Before launching a commercial or dual-licensing program —
> especially the Corporate CLA, trademark registration, and the commercial
> license text — have everything reviewed by a qualified attorney in your
> jurisdiction. "Legally foolproof" is a goal a lawyer signs off on, not a
> template.
