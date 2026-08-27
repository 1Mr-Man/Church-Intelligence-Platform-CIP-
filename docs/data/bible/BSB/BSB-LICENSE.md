# Berean Standard Bible (BSB) — License & Provenance Record

This document is CIP's evidence record for importing the Berean Standard
Bible as the first complete, production Bible dataset. It exists so a
human reviewer can independently verify the same claim later — see
"What CIP is allowed to do" for the operative conclusion, and "Evidence
chain" for exactly what was and wasn't directly checked.

## Source

**Translation:** Berean Standard Bible (BSB), a complete 66-book Protestant
canon translation created and published by the Berean Bible project
(bereanbible.com / berean.bible).

**Acquisition path used by CIP:** CIP did **not** fetch text from
bereanbible.com, berean.bible, or Bible Hub directly — this development
environment's network egress policy blocks all three domains. Instead,
the dataset was acquired via `git clone` of
[`github.com/lyteword/bsb`](https://github.com/lyteword/bsb) (commit
`808caa61129a5fcd72623cd4b097aab972f5341b`, dated 2026-03-14), a
publicly-cloneable Markdown transcription of the complete BSB text,
organized one file per chapter across all 66 canonical books. This
repository's own `LICENSE` file dedicates its contents to **CC0 1.0
Universal** (a formal public-domain dedication).

## Rights statement

The `lyteword/bsb` repository's `README.md` states, quoting the primary
source directly:

> "As per specification on their [site](https://berean.bible/licensing.htm):
> The Berean Bible and Majority Bible texts are officially placed into the
> public domain as of April 30, 2023. See terms and conditions. Licensing
> is not required for any use."

This was independently corroborated by a second, separate open-source
project explicitly built around redistributing BSB —
[`github.com/BSB-publishing/bsb2usfm`](https://github.com/BSB-publishing/bsb2usfm)
— whose own `README.md` and formal `LICENSE`/`UNLICENSE` files state:

> "The Berean Standard Bible text is free of copyright restrictions and
> dedicated to the public domain... No attribution is required (though
> appreciated). No permission is needed to use the BSB text in any
> project, publication, software, or derivative work."

and whose `README.md` further states the text is public domain "for the
full Bible (66 books)," directly matching CIP's canonical book catalog
(`core/bible::book_alias::BOOKS`) book-for-book, code-for-code.

## Evidence chain (honest accounting)

| Claim | Directly verified by CIP? |
|---|---|
| `lyteword/bsb`'s own content (this Markdown transcription) is CC0 | **Yes** — its `LICENSE` file was read directly. |
| BSB-publishing/bsb2usfm asserts BSB text is public domain, quoting `bereanbible.com` | **Yes** — its `README.md`/`LICENSE`/`UNLICENSE` were read directly. |
| `berean.bible/licensing.htm` itself states BSB was placed in the public domain as of April 30, 2023 | **Not directly** — `berean.bible`, `bereanbible.com`, `biblehub.com`, and `en.wikipedia.org` were all blocked by this environment's network egress policy (only `raw.githubusercontent.com` and the `git` protocol to `github.com` were reachable). This statement rests on two independent, credible GitHub-hosted sources (above), one of which quotes it directly with a citation and effective date, not on CIP's own visit to the primary page. |

**Conclusion CIP drew:** given two independent, formally-licensed
open-source projects — one of them literally named for BSB publishing —
both asserting the same specific, dated public-domain claim with no
contradicting evidence found anywhere, CIP classified this dataset as
`VerifiedPublicDomain`. This is a considered judgment under real
environmental constraints, not a first-party confirmation. **A human
maintainer should independently visit `berean.bible/licensing.htm`
before this dataset is relied upon in a context where that first-party
confirmation matters (e.g. wide public redistribution).**

## What CIP is allowed to do

Per the rights statement above: copy, distribute, modify, and use the BSB
text for any purpose, including commercial use, without attribution or
permission. CIP's own use — storing the text locally, displaying it under
operator control, never modifying its wording — is well within this
grant even under the most conservative reading.

## Is the full text bundled?

**Yes.** `database/datasets/bsb/bsb.json` (checked into this repository)
contains the complete parsed BSB text — 66 books, 1189 chapters, 31,086
verses — normalized into CIP's `BibleDatasetInput` import shape.
Bundling was chosen (per the "distribution model" decision) because the
text is verified public domain: nothing prevents CIP from redistributing
it as part of the application source.

## Verified vs. not verified — summary

- **Verified (directly):** the specific transcription repository's own
  CC0 license; a second, independent project's formal public-domain
  license for the same text; internal structural completeness (66 books,
  1189 chapters, no duplicates, no empty text) of the imported dataset.
- **Not verified (blocked by network policy):** the primary
  `berean.bible/licensing.htm` page itself, `bereanbible.com`,
  `biblehub.com`, and a general web search for corroborating sources.

## Date verified

2026-08-27 (this development session). Verification source: GitHub
repositories `lyteword/bsb` and `BSB-publishing/bsb2usfm`, fetched via
`raw.githubusercontent.com` and `git clone`.

## Bible Hub's role

Bible Hub (biblehub.com) was **not** used as the acquisition source and
was **not reachable** in this environment. It is referenced here only
because the Berean Standard Bible project is historically associated
with the Bible Hub team; no content was scraped from biblehub.com. See
`docs/bible-production-dataset.md`'s "Bible Hub usage policy" section for
CIP's standing policy on Bible Hub.
