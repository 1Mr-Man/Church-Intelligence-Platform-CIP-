# Church Intelligence Platform (CIP)

CIP is a desktop application that assists a live church service: as
scripture is spoken, it detects the reference, brings up the verse text,
and queues it for presentation - reviewed and approved by a human operator,
never auto-applied. It is being built in phases; this repository currently
contains **Phase 1 - Foundation**, **Phase 1.1 - Bible Intelligence Core**,
**Phase 1.2 - Live Speech Foundation**, **Phase 1.2.1 - Runtime
Compatibility**, **Phase 1.3 - Live Service Intelligence & Operator
Workflow**, **Phase 1.4 - Presentation Foundation & Real-Service
Validation**, **Phase 1.5 - Content/Dataset Foundation &
Full-Service Validation**, **Phase 2.0 - Intelligence Architecture &
Unified Intelligence Context**, **Phase 2.1 - Music Intelligence
Foundation & Song Recognition Architecture**, **Phase 2.2 - Acoustic
Music Recognition & Live Song Detection**, **Phase 2.3 - Sermon
Intelligence & Live Message Understanding**, **Phase 2.4 -
Cross-Domain Intelligence & Correlation**, **Phase 2.4 - Service
Intelligence**, **Phase 2.5 - Sermon Intelligence Foundation**,
**Phase 2.6 - Sermon Intelligence**, **Phase 2.7 - Content
Intelligence**, **Phase 2.8 - Cross-Domain Intelligence**, and **Phase
2.9 - Unified Operator Workspace**, **Phase 2.10 - Full Phase 2
Validation & First-Use Readiness**, and **Phase 3.0 - First-Use
Hardening**. A separate release-readiness milestone (not a new
intelligence phase) subsequently replaced the tiny development Bible
fixture with a real, complete, legally-documented 66-book production
translation - see [`docs/bible-production-dataset.md`](docs/bible-production-dataset.md) -
and a local presentation display window (a second Tauri window under
explicit operator control) - see [`docs/presentation.md`](docs/presentation.md).

**Phase 2.10** validated the entire Phase 2 stack end to end against the
real running codebase - not just its documentation - and closed the one
real gap it found (Bible detection/context/suggestion had never been
tested against the real BSB dataset, only the dev fixture). Verdict:
**first-use ready under documented conditions**. See
[`docs/phase-2-validation.md`](docs/phase-2-validation.md) for the full
readiness matrix, evidence, and conditions.

**Phase 3.0** hardened the four genuine first-use gaps that validation
found: the speech model path is now configurable
(`CIP_WHISPER_MODEL_PATH`) and the "speech unavailable" notice now names
the exact fix; accepting a Content Candidate is no longer a dead end
(a "Saved Content" view); and Bible/BSB readiness is now visible in the
always-visible header, with a dataset-import failure degrading
gracefully instead of crashing the app. **If you are a church operator
setting CIP up for a real service, start with
[`docs/first-use.md`](docs/first-use.md)** - a Quick Start, configuration
guide, and troubleshooting table written for a non-developer. See
[`docs/phase-3-first-use.md`](docs/phase-3-first-use.md) for the
engineering record of what changed and why.

**Phase 3.1** asked a narrower question than any prior phase: not "does
the code pass its tests," but "can a real church install CIP, connect
real hardware, and run a real service without a developer in the room?"
It closed four genuine failure-injection test gaps, added the one
end-to-end proof still missing since Phase 2 (all nine domains chained
through a single transcript against real SQLite), and built and launched
a real installable `.deb` package for the first time. Live microphone
capture, real Whisper transcription, and physical projector/monitor
output remain **NOT AVAILABLE** in every development environment used so
far - honestly reported, never fabricated. Verdict: **PILOT READY —
CONDITIONAL**. See [`docs/phase-3-1-pilot.md`](docs/phase-3-1-pilot.md)
for the full readiness matrix and hardware results.

**Phase 3.2** attempted to remove those conditions through real hardware
validation and prepared a production release candidate. The hardware
picture is unchanged - still **NOT AVAILABLE**, not upgraded to VERIFIED,
in every environment this project has been built in - but a genuine
defect was found and fixed (a disconnected microphone mid-service was
previously invisible to the operator), a real backup mechanism was built
and proven with an actual restore round-trip, a hardware-diagnostics
command was added, and a real forced-process-termination (`kill -9`)
crash-recovery test was run against the actual release binary. Software
gate: **RELEASE CANDIDATE**. Church hardware pilot gate: **HOLD**, pending
real microphone/Whisper/projector verification on a church's own
hardware. See
[`docs/phase-3-2-hardware-pilot.md`](docs/phase-3-2-hardware-pilot.md)
and [`docs/release-manifest-3.2.json`](docs/release-manifest-3.2.json).

**Phase 3.3** asked whether CIP can now be qualified for a real physical
church pilot, and built the machinery to answer that question honestly
going forward: a code-level Hardware Pilot Qualification Model
(`pilot_evidence.rs`) that makes "an automated pass is never a hardware
pass" a structural guarantee rather than a promise, a substantially
expanded `get_pilot_diagnostics` operator tool (machine identity, build
commit, database health, Bible integrity, alongside the existing audio/
Whisper/display detail), a deterministic hardware qualification
checklist, and a portable, git-tracked `pilot-evidence/` package. A
direct OS-level hardware re-probe found the same result as every prior
phase - still **NOT AVAILABLE**, not upgraded to VERIFIED. Software gate:
**RELEASE CANDIDATE**. Hardware qualification, pilot qualification, and
final release gates: **HOLD**, honestly and explicitly, pending the same
real microphone/Whisper/projector/operator verification on a church's
own hardware named in Phase 3.2. See
[`docs/phase-3-3-pilot-qualification.md`](docs/phase-3-3-pilot-qualification.md)
and the [`pilot-evidence/`](pilot-evidence/) directory.

**Phase 3.4** attempted, for the first time, a real Windows release
candidate: cross-compiling from this Linux build environment using
`rustup target add x86_64-pc-windows-gnu`, `mingw-w64`, and `nsis`. It
worked - a genuine Windows PE executable and a real NSIS installer
(`Church Intelligence Platform_0.1.0_x64-setup.exe`, **unsigned**) were
produced and verified with `file`/SHA-256, something Phase 3.2's own
release manifest had recorded as not possible in this environment.
`get_pilot_diagnostics` (Phase 3.3) finally got a frontend: a "System
Diagnostics" panel showing machine, database, Bible, microphone, Whisper
model, and display status in plain language. `AudioEngineStatus` gained
`selectedDevice`/`channels`, and `DisplayDiagnostic` gained
`positionX`/`positionY`, closing two real gaps in the hardware-diagnostic
surface. None of this puts a real Windows machine, a real microphone, or
a real projector in this container - hardware qualification remains
**HOLD**, honestly, pending the same real-hardware pilot Phase 3.2/3.3
already called for, now on the actual target Windows laptop. See
[`docs/phase-3-4-windows-pilot.md`](docs/phase-3-4-windows-pilot.md) and
[`pilot-evidence/3.4/`](pilot-evidence/3.4/).

> **Roadmap note.** This repository's authoritative Phase 2 roadmap is:
> 2.0 Intelligence Architecture -> 2.1 Unified Intelligence Event/Context
> Layer -> 2.2 Music Content Foundation -> 2.3 Music Intelligence -> **2.4
> Service Intelligence** -> **2.5 Sermon Intelligence Foundation** ->
> **2.6 Sermon Intelligence** -> **2.7 Content Intelligence** -> **2.8
> Cross-Domain Intelligence** -> 2.9 Unified Operator Intelligence
> Workspace -> 2.10 Full Phase 2 Validation. The "Phase 2.4 - Cross-Domain
> Intelligence & Correlation" work above, and the "Phase 2.3 - Sermon
> Intelligence & Live Message Understanding" work (deterministic semantic
> detection - themes, main points, illustrations - in `core/sermon`/
> `sermon_adapter.rs`/`sermon.rs`), were both built and committed under
> earlier, internal phase labels before this roadmap was adopted; those
> labels are historical artifacts and are not rewritten. The cross-domain
> work was subsequently extended in place as the roadmap's actual Phase
> 2.8 (see [`docs/cross-domain-intelligence.md`](docs/cross-domain-intelligence.md)),
> and the roadmap's actual Phase 2.9, Unified Operator Workspace, is built
> on top of it - see [`docs/operator-workspace.md`](docs/operator-workspace.md).
> The semantic sermon work, understood as Phase 2.6-equivalent, was
> subsequently extended *in place* under the real Phase 2.6 label - adding
> Takeaway/FoodForThought detection, a logistics-question false-positive
> fix, and Phase 2.5 Sermon Foundation awareness (`sermonId`, section
> evidence, speaker attribution, a read-only candidate-section suggestion)
> - rather than being duplicated into a second engine; see
> [`docs/sermon-intelligence.md`](docs/sermon-intelligence.md). **Phase
> 2.5** under this roadmap is the separate, prerequisite entity/lifecycle
> foundation described in
> [`docs/sermon-foundation.md`](docs/sermon-foundation.md). See also
> [`docs/service-intelligence.md`](docs/service-intelligence.md) for the
> Service Intelligence work that is Phase 2.4 under this roadmap.

## Approved architecture

- **Local-first & offline-capable.** No required cloud database, no
  Supabase, no external database server. Every install owns a single local
  SQLite file.
- **Internet-enhanced, never internet-required.** Optional integrations
  may use the network to enhance the experience; nothing in `core` may
  assume one is present.
- **AI-native, human-controlled.** AI produces suggestions; only a human
  action approves, edits, or rejects them. Nothing auto-applies based on
  confidence alone.
- **Desktop-first.** Tauri + React + TypeScript frontend, Rust backend.
- **Domain-oriented.** Business logic is organized by domain (`core/*`),
  not by technical layer, and depends on external systems only through
  provider/adaptor traits (`integrations/*`).

See [`docs/architecture.md`](docs/architecture.md) for the full picture,
[`docs/bible-intelligence.md`](docs/bible-intelligence.md) for the
transcript-to-suggestion pipeline, [`docs/live-speech.md`](docs/live-speech.md)
for real audio capture, the speech-to-text boundary, and the Live Church
Brain UI, [`docs/live-service.md`](docs/live-service.md) for the service
lifecycle and operator workflow built around that pipeline,
[`docs/presentation.md`](docs/presentation.md) for the presentation
preparation path from an approved suggestion to persisted, prepared
output, [`docs/content-registry.md`](docs/content-registry.md) for what
local content exists and its provenance/licensing,
[`docs/bible-datasets.md`](docs/bible-datasets.md) for the Bible dataset
importer/integrity checker and the licensing policy governing them,
[`docs/full-service-validation.md`](docs/full-service-validation.md) for
the realistic full-service validation results,
[`docs/intelligence-architecture.md`](docs/intelligence-architecture.md)
for the shared intelligence contracts Bible/Music/Sermon/Content engines
are built on,
[`docs/music-intelligence.md`](docs/music-intelligence.md) for the Music
Intelligence engine (deterministic title/alias/number/lyric recognition
- explicitly not audio fingerprinting),
[`docs/music-datasets.md`](docs/music-datasets.md) for the music dataset
importer and its licensing policy,
[`docs/acoustic-music.md`](docs/acoustic-music.md) for acoustic
(audio-fingerprint) song recognition,
[`docs/sermon-intelligence.md`](docs/sermon-intelligence.md) for
deterministic sermon structure/theme/meaning detection,
[`docs/cross-domain-intelligence.md`](docs/cross-domain-intelligence.md)
for the deterministic rule engine that correlates Bible/Music/Sermon
findings with each other,
[`docs/content-intelligence.md`](docs/content-intelligence.md) for how
accepted findings are structured into future content opportunities
(never final copy, never published or scheduled),
[`docs/development.md`](docs/development.md) to get running locally, and
[`docs/database.md`](docs/database.md) for the SQLite/migration story.

## What's implemented (and what isn't)

**Phase 1 (Foundation)** established the application skeleton: the desktop
shell, the domain-oriented crate layout, the local SQLite schema and
migration system, typed domain contracts (`BibleProvider`, `AudioEngine`,
`SpeechEngine`, `SearchEngine`, `Suggestion`, `PresentationItem`,
`ServiceSession`, `ConfidenceResult`), the event architecture,
configuration, logging, and the Scripture Context Manager's interface
boundary.

**Phase 1.1 (Bible Intelligence Core)** implemented that interface
boundary for real: transcript text normalization, deterministic scripture
reference detection, the `ScriptureContextManager` (so "verse 28" resolves
against whatever chapter the pastor named, even across unrelated
intervening speech), Bible-validated reference resolution, confidence
scoring, and `Suggestion` creation - all driven by a deterministic
transcript-input test harness, with no real audio or speech model
involved. See [`docs/bible-intelligence.md`](docs/bible-intelligence.md).

**Phase 1.2 (Live Speech Foundation)** connects that pipeline to a real
live-service input path: a real cross-platform `AudioEngine`
(`integrations/audio::CpalAudioEngine`, over `cpal`), a replaceable
`SpeechEngine` boundary with three implementations (`NullSpeechEngine`,
`ScriptedSpeechEngine` for deterministic testing, and a real local
`WhisperSpeechEngine` behind a `whisper` Cargo feature), transcript/
detection/suggestion persistence, Tauri IPC and event wiring, a manual
text-entry fallback, online/offline and AI-availability status reporting,
and a v0.1 "Live Church Brain" operator UI. See
[`docs/live-speech.md`](docs/live-speech.md), including the documented
model-download blocker in this development environment and how to verify
real transcription with network access to a model host.

**Phase 1.2.1 (Runtime Compatibility & Web Fallback)** made the frontend
runtime-aware: this same build can be deployed as a static web app (e.g.
Vercel) with no Tauri backend behind it. Every Tauri IPC call and event
subscription now checks `isTauriRuntime()` first, so opening the web
deployment in an ordinary browser shows a clear "Web Runtime" notice
instead of the raw `TypeError` a bare `invoke()` call previously threw
outside Tauri. See [`docs/live-speech.md`](docs/live-speech.md#cip-web-vs-cip-desktop-phase-121).

**Phase 1.3 (Live Service Intelligence & Operator Workflow)** turned that
pipeline into a reliable, operator-controlled live-service tool: a full
service lifecycle (start/pause/resume/end, with duplicate-start and
invalid-transition protection), a service timeline reusing the existing
`audit_events` table, session-scoped suggestion deduplication, operator
ambiguity resolution and manual context correction (both validated and
audited), edit validation against the `BibleProvider`, audio/speech/
database failure recovery that keeps the service live, a service history
archive, and a refit "Live Church Brain" operator workspace (confidence-
grouped suggestion queue, current/recent/history views that never
interfere with each other, guarded keyboard shortcuts). See
[`docs/live-service.md`](docs/live-service.md), including its documented
scope decisions and the reasoning behind the deduplication/ambiguity/
failure-recovery policies.

**Phase 1.4 (Presentation Foundation & Real-Service Validation)** connected
that operator-approved pipeline to a real presentation preparation path:
`PresentationItem` now traces back to the suggestion (if any) it came
from and the template that rendered it, a deterministic
`SCRIPTURE_DEFAULT` renderer turns real, local-Bible-sourced content into
a structured slide, and separate Preview (non-mutating, available before
approval) and Prepare (approval-gated, persists) actions replace a
pre-1.4 UI bug where "Preview" silently called the approval-gated prepare
command. A manual creation path keeps presentation preparation working
with no suggestion, no speech engine, and no network. Preparation is
still never projection - nothing in this codebase can display prepared
content yet, and the phase proves as much: a detected Scripture cannot
automatically become a prepared item, and no code path ever sets a
presentation item to "active". See
[`docs/presentation.md`](docs/presentation.md).

**Phase 1.5 (Content/Dataset Foundation & Full-Service Validation)** built
the scalable local content layer underneath that pipeline and validated
the whole thing end to end against realistic transcripts. A new Content
Registry (`core/content`/`integrations/content`) answers "what local
content exists, and under what license?" for any content category
(Bible today; music/sermon/media/reference are the reserved shape for
later phases); a reusable, idempotent Bible dataset importer validates
and loads a structured local dataset (never bulk-downloading or scraping
a copyrighted translation); a dataset integrity checker distinguishes a
development fixture from a complete canonical dataset without hard-coding
Bible facts it can't verify; `core/bible` gained verse-range retrieval
and a translation-aware local search dispatcher (reference/chapter/range/
free-text, entirely offline); and a canonical full-service acceptance
test proves context retention, context replacement, false-positive
protection, operator overrides, and dataset-validation authority (an
out-of-range verse never produces a suggestion) all hold together in one
realistic scripted service. See
[`docs/content-registry.md`](docs/content-registry.md),
[`docs/bible-datasets.md`](docs/bible-datasets.md), and
[`docs/full-service-validation.md`](docs/full-service-validation.md).

**Phase 2.0 (Intelligence Architecture & Unified Intelligence Context)**
built the shared architecture future intelligence engines will sit
behind, without implementing any of them. A new `core/intelligence` crate
defines `IntelligenceDomain`/`FindingKind`/`FindingStatus`, the mandatory
observed/inferred/suggested/generated distinction, an evidence/provenance
model tied to the Phase 1.5 Content Registry, a deterministic (confidence
is not urgency) priority model, a bounded `IntelligenceContext` (proven to
stay bounded even fed 10,000 synthetic transcript segments), the
`IntelligenceEngine` contract, an in-process engine registry with
panic-safe failure isolation, a correlation-model foundation, and an
in-memory finding queue. The one real engine is a thin compatibility
adapter over the unchanged Bible Intelligence Core - `core/bible` and
`core/service` were not modified by this phase, and every existing Bible
regression test still passes unmodified. Music, Sermon, and Content
intelligence remain **not implemented** - `IntelligenceDomain` only
reserves the shape they will occupy. See
[`docs/intelligence-architecture.md`](docs/intelligence-architecture.md).

**Phase 2.1 (Music Intelligence Foundation & Song Recognition
Architecture)** added the first real *second* intelligence domain:
`MusicIntelligenceEngine`, a second `IntelligenceEngine` registered
alongside Bible, proving the Phase 2.0 architecture generalizes. A new
`core/music` crate implements deterministic, offline title/alias/hymn-
number/lyric recognition (never audio fingerprinting - that stays
honestly reported unavailable), scored by a documented confidence
hierarchy and distinctiveness formula, with first-class ambiguity
handling and song continuity. `integrations/music` provides a real
`SqliteMusicProvider` and an idempotent dataset importer, dataset-
isolated the same way two Bible translations are (two datasets can both
have song number "120" and never collide). Music and Bible never call
each other directly; the only channel is the shared
`IntelligenceContext`, proven with both real engines registered
simultaneously. Music recognition never automatically creates a
presentation item - accepting a finding is a review decision, nothing
more. `core/bible`, `core/service`, and `bible_adapter.rs` were not
modified by this phase. See
[`docs/music-intelligence.md`](docs/music-intelligence.md) and
[`docs/music-datasets.md`](docs/music-datasets.md).

**Phase 2.2 (Acoustic Music Recognition & Live Song Detection)** added a
second, real recognition path for Music Intelligence: acoustic
(audio-fingerprint/embedding) recognition, fused with the existing
lyric/title path rather than replacing it. A new `cip_core_music::AcousticMusicRecognizer`
trait (mirroring `SpeechEngine`'s pattern exactly) is implemented by a
new `integrations/music-acoustic` crate
(`NullAcousticMusicRecognizer`/`ScriptedAcousticMusicRecognizer`/
`LocalAcousticMusicRecognizer`) - the last honestly reports
`Unavailable` in this build, since no acoustic inference backend has
been chosen or implemented, matching Phase 2.2's explicit "never fake
recognition" requirement. Evidence fusion (`core/music::fusion`, a
noisy-or combination, never a simple average) lets acoustic and lyric
evidence for the same song corroborate each other without creating a
second finding/confidence system; song continuity, transitions, and
ambiguity all reuse Phase 2.1's existing policy unchanged. A new,
deliberately minimal `CurrentSong` concept is set only by an explicit
operator accept and cleared only by an explicit operator clear - never
automatically, regardless of confidence. `pipeline.rs`, `core/bible`,
and `bible_adapter.rs` were not modified by this phase. See
[`docs/acoustic-music.md`](docs/acoustic-music.md).

**Phase 2.3 (Sermon Intelligence & Live Message Understanding)** added a
new `core/sermon` domain crate - deterministic, phrase-anchored detection
of sermon theme, main/sub-points, definitions, key statements,
declarations, questions, illustrations/stories/examples, applications,
prayer points, reflections, transitions, and conclusion signals - and a
`core/intelligence::sermon_adapter::SermonIntelligenceEngine` translating
those detections into `IntelligenceFinding`s with strict Observed/Inferred
epistemic labeling (never Suggested/Generated). A theme candidate requires
both repeated mention *and* at least one structural mention before it
qualifies - repetition alone is never enough. Scripture references are
never re-detected: the engine only cross-links a freshly recorded main
point to whatever Scripture context the unchanged Bible engine already
established. `pipeline.rs` was not modified; Sermon Intelligence is
manual-command-only, mirroring Music's Phase 2.1 lyric path. See
[`docs/sermon-intelligence.md`](docs/sermon-intelligence.md).

**Phase 2.4 (Cross-Domain Intelligence & Correlation)** added the first
layer that reasons about relationships *between* findings from the Bible,
Music, and Sermon engines - a deterministic
`core/intelligence::cross_domain::CrossDomainCorrelationEngine` that reads
`IntelligenceContext.recent_findings` (never calling another engine
directly) and derives `IntelligenceCorrelation`s such as "this sermon
point references the same verse as this Bible finding." Every correlation
carries an explicit rule id/version and evidence; none is ever fabricated
from mere transcript proximity alone - "Amazing Grace" recognized
elsewhere in a service that also mentions Romans 8 does not automatically
correlate. Correlations are their own type, reviewed/dismissed through the
same human-in-the-loop discipline as every other finding, and never
auto-converted into a presentation item. `core/bible`, `core/service`, and
every existing engine's own logic were not modified; the one new bridge
(`analyze_bible_transcript`) exposes the already-registered Bible engine
to make its findings reachable for correlation, mirroring Music's and
Sermon's existing manual-command pattern. See
[`docs/cross-domain-intelligence.md`](docs/cross-domain-intelligence.md).

**Phase 2.7 (Content Intelligence)** added the bridge between intelligence
and future content production - never a leap into it. A new
`core/intelligence::content_intelligence::ContentIntelligenceEngine`
reads already-produced, already-reviewable findings and structures them
into `ContentCandidate`s: a typed record that a piece of already-proven
information *appears suitable* as a future content opportunity, never
final copy, never a social post, never published or scheduled. Like the
Phase 2.4/2.8 correlation engine, it is not registered into
`IntelligenceEngineRegistry` (a candidate is not an `IntelligenceFinding`,
so it does not implement `IntelligenceEngine`). Confidence is reused
unchanged from the source finding; a separate, independently-varying
`content_potential` score answers a different question ("how suitable
does this look as content"), proven by a dedicated test that the two
dimensions can diverge or invert. Only Sermon-domain findings are mapped
in this initial phase, via an explicit, documented summary-prefix table -
nothing is guessed from free text. See
[`docs/content-intelligence.md`](docs/content-intelligence.md).

**Phase 2.8 (Cross-Domain Intelligence)** extended the existing
correlation engine above - rather than building a second one - once
Service Intelligence, Sermon Foundation, and Content Intelligence existed
to correlate against. An audit found the engine already satisfied nearly
every formal requirement; the two genuine gaps closed were
`IntelligenceContext` gaining a `ContentCandidate`-aware builder (mirroring
`with_sermon_context`'s own additive discipline) and the `Service` domain
being included in the weakest temporal-proximity fallback rule for the
first time. Two new `CorrelationKind` variants (`SermonContent`,
`MultiDomainConvergence`) and two new rules were added; every Phase 2.4
rule and confidence value is unchanged. See
[`docs/cross-domain-intelligence.md`](docs/cross-domain-intelligence.md).

**Phase 2.9 (Unified Operator Workspace)** turned the eight independently
real capabilities above into one coherent operator screen, without adding
a ninth. A new glance-able header, a bounded/prioritized "needs attention"
queue, and a bounded/filterable cross-domain feed are all pure, frontend-
only projections (`lib/unifiedFeed.ts`/`lib/attentionQueue.ts`) over state
every existing panel already fetches - zero new Tauri commands, events, or
database migrations. Every existing panel remains fully present and
functional; only two purely diagnostic ones (Content Registry,
Intelligence Status) were collapsed by default. See
[`docs/operator-workspace.md`](docs/operator-workspace.md).

Still deliberately **not** implemented: a chosen/trained acoustic model
(so real-world acoustic recognition accuracy remains unverified in this
environment - see [`docs/acoustic-music.md`](docs/acoustic-music.md)'s
"PROVEN vs NOT AVAILABLE" section), any semantic/LLM-based sermon
understanding beyond deterministic phrase-anchored detection (see
[`docs/sermon-intelligence.md`](docs/sermon-intelligence.md)'s "NOT
AVAILABLE" section), semantic/paraphrase Bible search, automatic bullet
extraction, a web research engine, online Bible fallback, content
generation, cloud sync, OBS/vMix integration, remote operator accounts, a
mobile app, real display/projection output, and the full presentation
designer (visual/typographic design beyond one deterministic template).
Those are later phases.

## Repository layout

```
apps/desktop/          Tauri + React + TypeScript desktop application
  src/                 React/TypeScript frontend
  src-tauri/            Rust backend (Tauri commands, app shell)

core/                  Domain logic and contracts, one crate per domain
  bible/               BibleProvider, text normalization, reference detection, verse-range/search, integrity checker, Scripture Context Manager
  content/              ContentRegistry - what local content exists, and its provenance/licensing
  intelligence/          Shared intelligence architecture (Phase 2.0) - IntelligenceContext/Engine/Finding, the Bible compatibility adapter, the Music adapter (Phase 2.1, extended with acoustic fusion in Phase 2.2), the Sermon adapter (Phase 2.3), the cross-domain correlation rule engine (Phase 2.4)
  music/                 Song/lyric domain model, MusicProvider trait, deterministic title/alias/number/lyric matcher (Phase 2.1); AcousticMusicRecognizer trait, segmentation, signal-quality gate, evidence fusion (Phase 2.2)
  sermon/                Deterministic sermon taxonomy, structural/theme detection, sermon-state inference - extended under Phase 2.6 with Takeaway/FoodForThought and a state->section candidate mapping; `foundation/` submodule: Sermon entity/lifecycle/section/speaker/segment model (Phase 2.5, per the authoritative Phase 2 roadmap)
  service/              ServiceSession + AudioEngine
  presentation/         PresentationItem
  search/               SearchEngine
  ai/                    SpeechEngine + Suggestion
  confidence/            ConfidenceResult (shared by every domain above)

database/              Local-first SQLite: migrations, schema docs, seeds
integrations/          Provider/adaptor implementations (bible - incl. dataset importer, content, audio, music - incl. dataset importer, music-acoustic, web, obs, vmix)
ai/                    AI backend implementations (speech, embeddings, classifiers, models)
presentation/          Rendering subsystem (renderer, templates, outputs)
tests/                 Cross-crate integration tests
docs/                  Architecture, setup, and reference documentation
```

Every `core/*` crate defines contracts, not implementations that reach out
to the OS, network, or a specific AI backend - those live in
`integrations/*` and `ai/*` and depend on `core`, never the other way
around. See [`docs/architecture.md`](docs/architecture.md#boundaries) for
the enforced boundaries.

## Quick start

```sh
pnpm install
pnpm --filter @cip/desktop tauri dev
```

See [`docs/development.md`](docs/development.md) for the full command
reference (typecheck, lint, Rust tests, database validation) and for what
each requires to be installed locally.
