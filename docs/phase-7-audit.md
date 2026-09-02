# Phase 7 — Audit

## Baseline

Phase 6 (Operator Ergonomics, 8 slices: 6.1-6.8) is closed. All 8 gaps
from its own audit are addressed at Environment A; Environment C
(real Windows hardware) verification remains the standing pending gate
for the whole Phase 6 arc, unchanged by this audit.

Phase 6 itself was a UX-quality pass over the existing feature set, not
drawn from `docs/phase-4-master-plan-gap-audit.md`'s "Proposed Phase 4
candidates" list - so that list is still the most current inventory of
this project's own CIP Master Architecture v1.0 gaps. This audit
re-verifies each remaining item against the current codebase before
proposing it as Phase 7 scope.

## Re-verification

| Master-plan item | Status re-confirmed | Evidence |
|---|---|---|
| Real audio fingerprinting | **STILL NOT STARTED** | `integrations/music-acoustic`'s `LocalAcousticRecognizer` remains structural scaffolding - `Null`/`Scripted`/`Local` recognizer variants exist, no spectral-hashing/fingerprint algorithm. |
| Internet/hybrid intelligence | **STILL NOT STARTED** | `integrations/web/src/lib.rs` is a 6-line placeholder ("No logic lives here yet, and nothing in `core` may assume this crate is present"). No `reqwest`/HTTP client usage anywhere in the workspace. |
| Multi-language support | **STILL NOT STARTED** | `ai/speech/src/whisper.rs`'s `language` field defaults to `None` (Whisper auto-detect) with no operator-facing language selection, no Yoruba/Igbo/Hausa/Pidgin handling, no i18n/locale directory anywhere in the repo. |
| Church/user roles & permissions | **STILL NOT STARTED** | No `users`/`roles` table in any of the 12 migrations - the one `role` concept in the schema (`0012_display_role_assignments.sql`) is `DisplayRole` (Stage/Confidence Monitor/Lobby), unrelated to authentication or access control. No login screen, no session/identity concept anywhere in the app. |
| OBS/vMix/livestream integration | **STILL NOT STARTED** | `integrations/obs` and `integrations/vmix` are both 6-line placeholder crates, explicitly marked out of scope since Phase 1, workspace members holding the architectural boundary only. |

All five items the Phase 4 gap audit deferred remain exactly as sized
and scoped as they were then - nothing in Phase 4-6's work touched any
of them.

## The fork

These five are not comparable-effort slices the way Phase 6's 8 items
were - each is its own multi-phase subsystem (the Phase 4 gap audit's
own "Honest sizing" section says as much), and they pull in different
directions:

- **Real audio fingerprinting** extends existing Music Intelligence
  (`core/music`/`integrations/music-acoustic`) - the most contained of
  the five, one domain, no new cross-cutting concern.
- **Multi-language support** extends the existing Whisper pipeline
  (`ai/speech`) - also fairly contained, though open-ended in how many
  languages/dialects to target first.
- **Internet/hybrid intelligence** is a new cross-cutting capability
  (an optional network path touching Bible/Sermon/Content lookups) that
  sits in real tension with this project's own delivered "must never
  depend on the internet to continue" differentiator - it would need
  its own careful scope boundary before any code is written.
- **Church/user roles & permissions** is a foundational, cross-cutting
  change (touches persistence, every command's authorization surface,
  and the UI) that every future multi-user/multi-church feature would
  sit on top of - large or small depending entirely on how much RBAC
  the operator actually needs day one.
- **OBS/vMix/livestream integration** is a new external-system
  integration (scene/output control), largely additive and isolated
  from the existing domains, but requires access to real OBS/vMix
  software this container cannot provide - Environment C verification
  would matter even more than usual here.

Given the size and direction of each, this is a genuine, user-level
choice, not a design detail to resolve unilaterally - the same
discipline the Phase 4 gap audit itself followed ("This document does
not choose the order. See the accompanying question.").
