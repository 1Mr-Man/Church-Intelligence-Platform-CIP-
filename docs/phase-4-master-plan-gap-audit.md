# Phase 4 Gap Audit — CIP Master Architecture v1.0 vs. Current Codebase

## Purpose

The user asked, on closing out the multi-screen pillar (Phase 3.10-
3.10.4), to cross-reference everything built so far against the full
CIP Master Architecture v1.0 document (the original vision/architecture
document this project has built against since Phase 0), identify
anything the master plan calls for that remains undone, and use that to
define "Phase 4" going forward - a fresh top-level phase, distinct from
this session's own `3.x` numbering, which grew organically and no longer
maps cleanly onto the master plan's original `01-08` module structure or
its own `Phase 0-6` roadmap.

This document is an honest audit, not an implementation. Nothing in this
document was built this phase.

## How to read this

Each of the master plan's eight major platform modules (its own section
5) is graded:

- **DONE** — built, tested, documented, matches the master plan's intent.
- **PARTIAL** — a real, working subset exists; specific named capabilities
  from the master plan are missing.
- **NOT STARTED** — no code exists for this capability.

Every "not started" or "partial" gap is cited against the actual
Rust/TS code or `docs/` file, not just the master plan's own wording -
consistent with this project's standing rule against inflating a
software-only reading into more than what was actually verified.

---

## 01 — Bible Intelligence

**Status: PARTIAL.**

| Master plan capability | Status | Evidence |
|---|---|---|
| Bible translation registry | PARTIAL | `bible_translations` table + `BibleProvider` trait exist (`core/bible`); only **KJV** is actually seeded/available. No licensing-tier metadata (public-domain/licensed/church-owned) on translation rows. |
| Direct/abbreviated/incomplete reference detection | DONE | `core/bible`'s reference detector, live since Phase 1.1. |
| Quotation detection | DONE | `DetectionType::Quotation`, tested. |
| **Paraphrase detection** | **NOT STARTED** | No embedding/semantic-similarity engine anywhere in the codebase. `DetectionType::Paraphrase` exists as an enum variant but nothing ever produces it - confirmed by grep, zero call sites construct that variant outside its own type definition. This is one of the five pillars named explicitly in every phase's `knownLimitations` since Phase 3.10. |
| Conceptual/semantic Scripture matching | **NOT STARTED** | Same root cause - no vector/embedding search exists in this codebase at all. |
| Sequential verse detection ("and verse 29...") | **NOT STARTED** | No "smart verse continuation" logic exists. |
| Bible name/pronunciation normalization | PARTIAL | Book-name alias normalization exists (`core/bible`'s alias module); no pronunciation-variant handling for spoken input. |
| Semantic Bible search ("verses about...") | **NOT STARTED** | `search_bible` (Phase 1.5) is FTS/keyword-based only - confirmed by reading `cip_core_bible::search`. No embedding-backed natural-language search. |
| Cross-reference intelligence | **NOT STARTED** | No `bible_cross_references` data or engine exists. |
| Bible comparison mode (side-by-side translations) | **NOT STARTED** | Impossible today regardless - only one translation (KJV) is installed. |
| AI Preloading (predict verse before spoken) | **NOT STARTED** | No predictive/partial-utterance logic exists. |

**Root blocker for most of this module:** no local embedding model or
vector search has ever been added to this codebase. Every "semantic"
capability in the master plan depends on it.

---

## 02 — Music Intelligence

**Status: PARTIAL.**

| Master plan capability | Status | Evidence |
|---|---|---|
| Song/hymn database, title/lyric/theme/Scripture/artist search | DONE | `core/music`, Phase 2.1. |
| Lyric recognition (STT-based) | DONE | Phase 2.1's lyric-matching layer. |
| Semantic song matching | PARTIAL | Deterministic distinctiveness/matching scoring exists (Phase 2.1); not embedding-based semantic search ("song about surrender"). |
| **Real audio fingerprinting** | **NOT STARTED** | `integrations/music-acoustic` has `Null`/`Scripted`/`Local` recognizer *scaffolding* (Phase 2.2) but no actual fingerprinting algorithm (e.g. Shazam-style spectral hashing) - confirmed by reading the crate: `LocalAcousticRecognizer` is a structural placeholder, not a working fingerprint matcher. Named explicitly as not-started in every phase's `knownLimitations` since Phase 3.10. |
| Church song library (church-uploaded songs) | PARTIAL | `saved_content_candidates`/Music Library UI exist; per Phase 2.7.1's own audit, "Music Library is legitimately empty in a production build - no licensed production song dataset exists." No per-church upload-your-own-song workflow with recognition-profile generation. |
| Hymn database/hymn books | **NOT STARTED** | No `hymns`/`hymn_books` tables exist separately from `songs` - the master plan's schema (section 33) calls these out as distinct entities. |
| CCLI/SongSelect licensed integration | **NOT STARTED** | No external song-provider adapter exists. |

---

## 03 — Live AI Listener

**Status: DONE for the core pipeline; PARTIAL for stated stretch goals.**

| Master plan capability | Status | Evidence |
|---|---|---|
| Speech-to-text, streaming, timestamps | DONE | Whisper integration, Phase 1.2-3.8.7.x. |
| Service-mode detection | DONE | Service Intelligence engine (`core/intelligence`'s `ServiceIntelligenceEngine`) detects phase transitions - this **is** the master plan's "Live Service Mode Engine" (its own section 24), delivered under different internal naming. |
| Multilingual recognition / code-switching | **NOT STARTED** | Whisper is invoked with no language parameter beyond its default; no Yoruba/Igbo/Hausa/Pidgin support, no code-switching logic. Named explicitly as not-started ("multi-language support") since Phase 3.10. |

---

## 04 — Sermon Intelligence

**Status: DONE.**

Points, sub-points, quotes, illustrations, questions, prayer points,
action points, food-for-thought, provenance tagging (direct
quote/AI summary/AI reflection) - all delivered (`core/sermon`, Phase
2.3-2.6, Sermon Foundation, Sermon Harvest Phase 3.9). This module most
closely matches the master plan's own intent of any of the eight.

---

## 05 — Presentation Engine

**Status: PARTIAL.**

| Master plan capability | Status | Evidence |
|---|---|---|
| Bible/lyrics/sermon/media slides, templates | DONE | `presentation/renderer`, Phase 1.4. |
| Multi-screen output | DONE | Phase 3.10-3.10.4 (Stage/Confidence/Lobby, Display Registry, Presentation Router, multi-window lifecycle) - just closed out. |
| LED wall / stage display as distinct destinations | PARTIAL | The three `DisplayScreen` roles cover Stage/Confidence/Lobby; the master plan names more destinations (LED wall, children's room, online service) with independent content rules - not modeled. |
| **OBS/vMix/livestream integration** | **NOT STARTED** | No `integrations/obs` or `integrations/vmix` crate exists despite being scaffolded in the master plan's own recommended repository layout (its section 74). No lower-third/overlay generation for livestream. |

---

## 06 — AI Operator

**Status: DONE.**

The suggestion → confidence → approve/edit/ignore/project lifecycle
(`ai_suggestions`, the Live Church Brain UI, `AttentionQueue`,
`PresentationCard`) matches the master plan's own section 6 closely -
this is the "killer feature" the master plan names explicitly, and it
is real and tested throughout this codebase.

---

## 07 — Production & Integration

**Status: PARTIAL**, subsuming the same OBS/vMix/multi-destination gaps
already listed under module 05 above. The master plan's own section 7
also lists a "children's room" destination and "online service"
destination as independent targets - neither is modeled as a distinct
`DisplayScreen` variant today.

---

## 08 — Post-Service Intelligence (Sermon Harvest)

**Status: DONE for the core harvest; PARTIAL for downstream content
generation.**

| Master plan capability | Status | Evidence |
|---|---|---|
| Transcript, structure, Scriptures, points, quotes, illustrations, prayer/action points, food for thought, timeline | DONE | `harvest.rs`, Phase 3.9. |
| Daily/weekly reflection generation | **NOT STARTED** | No reflection-generation logic exists; the harvest bundle returns raw findings, not AI-generated daily/weekly reflections. |
| Sermon-to-content (social posts, clips, captions, YouTube descriptions) | **NOT STARTED** | Explicitly out of scope per Phase 3.9's own doc ("a real, separate future feature"). |
| Searchable service recording with jump-to-timestamp | PARTIAL | Timeline entries exist and are timestamped; no dedicated "jump to the moment Pastor said X" search UI over past services - `service.rs`'s "What did the pastor just say?" search only works on the *current* live transcript, not historical services. |

---

## Cross-cutting architecture items (outside the eight modules)

| Master plan requirement | Status | Evidence |
|---|---|---|
| Internet/hybrid intelligence (local-first, online-enhanced) | **NOT STARTED** | This entire codebase is offline-only by construction - no online search gateway, no web-search adapter, no "not in local database → search online" fallback exists anywhere. This is a major, explicitly-designed pillar of the master plan (its sections 31-32, "Internet Intelligence Engine") that has never been started. |
| Church/user roles & permissions | **NOT STARTED** | No multi-user model exists at all - the app has no login, no user table, no role enforcement. Single-operator, single-machine only. |
| Cloud synchronization | **NOT STARTED** | No sync engine; `services`/`songs`/etc. live in one local SQLite file with no replication story. |
| Mobile/remote control | **NOT STARTED** | Desktop-only; no companion mobile app or remote-control protocol. |
| Analytics | **NOT STARTED** | No usage/accuracy-metrics dashboard exists (the master plan's own section 65 "AI Accuracy Philosophy" calls for measuring detection accuracy/false-positive rate/correction rate empirically - none of that is instrumented). |
| Backup architecture | PARTIAL | `backup_database` command exists (Phase 3.2); no external-drive/network-storage/cloud backup target, no scheduled automatic backup. |
| Security (auth, encrypted credentials, audit logging) | PARTIAL | Audit trail exists (every AI suggestion's lifecycle is recorded via `audit_events`/timeline); no authentication, no encrypted credential storage (moot today since there are no external API keys in the app), no role-based access control. |

---

## What this audit does NOT flag as a gap

To be precise about scope, these master-plan items are **fully or
substantially delivered** and are not part of any "Phase 4" backlog:

- Bible/Music/Sermon/Service/Cross-Domain/Content Intelligence engines
  (the `core/intelligence` registry architecture itself, matching the
  master plan's own "AI must never become a single point of failure for
  the presentation system" boundary - suggestions never bypass operator
  approval anywhere in this codebase).
- Offline-first operation (the master plan's other core requirement -
  "the service must never depend on the internet to continue" - is
  fully honored; every domain works with zero network access).
- SQLite-based local database, event architecture, confidence engine,
  AI content provenance tagging (DIRECT_QUOTE/AI_SUMMARY/AI_REFLECTION
  equivalents exist via `SermonElementKind` and finding-status handling).
- The "Live Church Brain" signature interface (the master plan's own
  stated differentiator) - built and iterated on since Phase 1.3.
- Multi-screen presentation output, Display Registry, Presentation
  Router, multi-window lifecycle (Phase 3.10-3.10.4, just closed).

## Honest sizing

Each **NOT STARTED** item above is not a small addition - most are their
own multi-phase subsystems comparable in scope to what Bible/Sermon/
Music Intelligence each took to build (multiple phases, real domain
models, persistence, commands, events, frontend surfaces, and full
regression + Windows verification each). Attempting all of them in one
undifferentiated pass would abandon this project's own established
discipline (one phase at a time, audit before major work, full
regression and Windows verification every phase) purely to move fast -
exactly the outcome that discipline exists to prevent.

## Proposed "Phase 4" candidates

In the master plan's own rough size/value order, the largest remaining
gaps are:

1. **Semantic/paraphrase Bible detection** — requires a local embedding
   model + vector search; unlocks paraphrase detection, semantic Bible
   search, and cross-reference intelligence together (they share the
   same underlying capability).
2. **Real audio fingerprinting** — a genuine spectral-hashing recognizer
   to replace the current `LocalAcousticRecognizer` scaffold.
3. **Internet/hybrid intelligence** — the online-search-when-local-fails
   fallback the master plan treats as a first-class architectural pillar,
   currently entirely absent.
4. **Multi-language support** — Whisper language configuration +
   Yoruba/Igbo/Hausa/Pidgin handling.
5. **Church/user roles & permissions** — the prerequisite for any real
   multi-user or multi-church deployment.
6. **OBS/vMix/livestream integration** — production-system connectivity.

Items 1-4 are the same four pillars the user named at the start of the
current `3.x` arc; item 1 subsumes several individually-listed master-
plan capabilities (paraphrase detection, semantic search, cross-
references) that all depend on the same missing embedding infrastructure,
so they should be sequenced together rather than as separate phases.

This document does not choose the order. See the accompanying question.
