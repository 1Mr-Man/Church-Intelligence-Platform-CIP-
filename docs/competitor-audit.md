# Competitor audit and CIP advantage plan

Date: 2026-10-13

This audit covers the local artifacts supplied for comparison. It is a product
benchmark, not a code or branding copy exercise. CIP should reproduce the user
outcomes with its own architecture, safety model, and visual identity.

## Evidence inspected

| Product | Evidence | Findings |
| --- | --- | --- |
| Pewbeam 2.6.14 | Installed package at `%LOCALAPPDATA%\\Pewbeam` | Six bundled translation packs: ESV, KJV, NASB, NIV, NKJV, and NLT; each includes a complete JSON/SQL dataset and per-book files. The package also includes background/typography assets and a Windows NDI runtime. |
| Pewbeam | `Pewbeam.exe` launched locally | The process exited immediately with Windows status `0xC0000409` on this machine, so runtime screens were not treated as verified evidence. |
| Loghema 1.0.9 | `Loghema_1.0.9_x64-setup.exe` | Unsigned NSIS installer, SHA-256 `D39A4332F399ED71E154AB23283E6F64A8116919A02D8F61ADA80473A4A730E3`. Windows returned `0xC0000142` on both normal and elevated launch, no Loghema window appeared, and no installed application was found. Static strings expose NDI/OBS-related installer markers, suggesting production integration is a positioning area worth validating. |

## What CIP already does better

- Explicit operator approval gates before content can reach a display.
- Context-aware Bible matching with explicit-reference parsing, semantic
  matching, and false-positive regression coverage.
- Service replay, transcript history, timeline events, diagnostics, and honest
  model-readiness reporting.
- Separate Stage, Confidence Monitor, and Lobby/Overflow display roles.
- Local-first Bible data and a licensing gate instead of silently bundling
  copyrighted translations.

## Public feature comparison

The public product pages and documentation add these verified competitor
capabilities:

| Capability | Pewbeam | Loghema | CIP direction |
| --- | --- | --- | --- |
| Direct, spoken, quotation, and semantic reference detection | Yes | Yes | Already present; continue calibration and evidence display. |
| Translation breadth and side-by-side display | Six bundled English translations; installed packs | Many translations and dual-translation display | Translation registry/import is present; this release adds side-by-side comparison in Bible Library. |
| Live transcript and manual override | Yes | Yes | Present in the live workspace and manual search. |
| Confidence threshold / autonomous mode | Configurable threshold and automatic or operator display | Configurable threshold and autonomous live mode | Keep suggestions pending by default; add an explicit per-service policy rather than silent auto-display. |
| Production outputs | NDI, HDMI, local HTTP/OSC | NDI, direct secondary display, OBS/vMix/Wirecast/ProPresenter | Existing local multi-screen and vMix foundations; add opt-in NDI/OBS adapters. |
| Content creation | Sermon notes, AI slides, devotional, tract, newsletter | Sermon slides, announcements, visual editor | Extend CIP's sermon intelligence into reviewed content drafts. |
| Worship library | Lyrics/slides workflow | EasyWorship/OpenLP song import | Add a rights-aware song/slide import boundary. |
| Offline/reliability | Cloud transcription plus offline option on supported Macs | Cloud transcription with local Bible/model assets | CIP's local Whisper path is a strong Windows-first foundation; expose model/index readiness clearly. |

Public references: [Pewbeam feature overview](https://pewbeam.com/pewbeam),
[Pewbeam documentation](https://pewbeam.com/docs), [Loghema documentation](https://www.loghema.com/docs),
[Loghema workflow](https://www.loghema.com/how-it-works), and [Loghema changelog](https://www.loghema.com/changelog).

## Improvements to make CIP the stronger product

### P0 - operator confidence

1. Add a single **Readiness Center** showing microphone, speech model,
   embedding index, Bible pack integrity, display outputs, and the one action
   needed to resolve each warning.
2. Make every Bible suggestion explain its evidence: explicit reference,
   quotation overlap, semantic similarity, surrounding context, translation,
   and why it passed the context gate.
3. Add a clear suggestion-quality filter: high-confidence references can enter
   the review queue; weak semantic candidates remain discoverable but never
   interrupt the operator.

### P1 - translation and production parity

1. Ship a translation-pack manager with licensing metadata, checksum,
   language, verse count, integrity status, and embedding-index status.
2. Bundle only verified public-domain or explicitly licensed packs. Offer an
   import path for church-owned/licensed ESV, KJV, NASB, NIV, NKJV, and NLT
   data instead of copying a competitor's files.
3. Add a production-output adapter boundary for NDI/OBS while retaining the
   existing safe presentation pipeline. NDI/OBS output must be opt-in,
   configured, and visible in diagnostics.

### P2 - workflow advantage

1. Add a compact operator "Now / Next / Why" queue for live suggestions,
   approved passages, and the active output.
2. Add saved service presets for translation, display roles, audio device,
   theme, and approval policy.
3. Add a post-service review that measures accepted/rejected suggestions,
   false positives, missed explicit references, inference latency, overloads,
   and translation coverage.

## Guardrails

- Do not copy competitor binaries, code, UI assets, fonts, branding, or
  copyrighted Bible text.
- Do not install the unsigned Loghema installer without explicit confirmation
  and a controlled test plan.
- Do not auto-display semantic matches. The operator approval boundary remains
  a core CIP differentiator.

## Recommended implementation order

1. Readiness Center and evidence-rich suggestion cards.
2. Translation-pack/licensing/index management.
3. NDI/OBS output adapters and a production validation harness.
4. Service presets and post-service quality analytics.

Current implementation also includes the operator voice-command confirmation
surface and the live **Now / Next / Why** strip. These are local, review-first
features and do not depend on a cloud model or an NDI runtime.

## Implementation decisions

### NDI output

CIP will target the official NDI SDK through a dynamically loaded adapter. The
SDK documentation describes dynamic loading as the preferred approach for
open-source projects and supports locating the Windows runtime through
`NDI_RUNTIME_DIR_V5`. CIP will keep NDI disabled unless the runtime is detected,
show a clear readiness state, and never copy a competitor's NDI DLLs.

The SDK requires its license/attribution terms to be carried by a distributed
application, including the NDI website link and third-party license file. CIP
will not package an NDI runtime until those terms and a vendor/distribution
decision are complete.

### Content generation

CIP will begin with local, draft-only generation from accepted sermon findings,
transcript excerpts, and detected references. This produces reviewable sermon
notes, slide outlines, devotionals, newsletters, and discussion prompts without
requiring a cloud key or transmitting sermon audio/text. An OpenAI-compatible
provider may be added later behind an explicit opt-in configuration; secrets
must remain outside source control and installer assets.

### Voice commands and review analytics

CIP now has a conservative, confirmation-gated operator command layer in the
live Attention Queue. It accepts only a small allow-list such as “display the
next suggestion”, “reject the next item”, “open diagnostics mode”, and “focus
search”. Speech recognition is opportunistic; when the WebView does not expose
it, the operator can type or paste the transcript. The interpreted action is
always shown for explicit confirmation and then routed through the same action
handlers and display confirmation guard used by the visible buttons.

The History view now surfaces correction rate and the five most recent service
outcomes, alongside the existing confidence and detection-method breakdowns.
This makes false-positive review actionable without presenting pending or
uncorrelated suggestions as accurate results.

NDI references: [NDI SDK](https://ndi.video/for-developers/ndi-sdk/),
[dynamic loading](https://docs.ndi.video/all/developing-with-ndi/sdk/dynamic-loading-of-ndi-libraries),
and [licensing](https://docs.ndi.video/all/developing-with-ndi/advanced-sdk/licensing).
