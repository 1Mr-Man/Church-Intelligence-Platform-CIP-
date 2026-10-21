# Competitor static inspection

Date: 2026-10-14

Scope: authorized feature-level inspection of the locally installed Pewbeam application and the supplied Loghema installer. This document records capabilities and packaging signals only; it does not copy proprietary source code, credentials, or copyrighted Bible text.

## Pewbeam

Inspected executable: `C:/Users/hp/AppData/Local/Pewbeam/Pewbeam.exe`

The executable is not a managed .NET assembly, so dnSpy is not the appropriate decompiler for it. Static resource and dependency inspection found:

- NDI runtime support: `vendor/ndi/windows/lib/Processing.NDI.Lib.x64.dll`.
- Multiple bundled Bible translations: ESV, KJV, NASB, NIV, NKJV, and NLT.
- Both JSON and SQL Bible data formats.
- Per-book JSON files for all Bible books.
- Feature/resource vocabulary indicating scripture search, songs, lyrics, slides, live display, queues, themes, fonts, history, audio, and worship workflows.
- The application contains graphics/runtime material consistent with a GPU-accelerated desktop presentation application.

Most relevant CIP opportunities confirmed by this inspection are multi-translation Bible search, offline Bible data, song/lyrics workflows, presentation queues, NDI output, and a stronger operator-facing display workflow.

## Loghema

Supplied installer: `C:/Users/hp/Downloads/Loghema_1.0.9_x64-setup.exe`

The installer is a native NSIS 3.11 package, not a .NET assembly. dnSpy cannot decompile the installer itself. An isolated silent extraction attempt failed with Windows error `0xc0000142` before files were created, so no claim is made here about Loghema's internal implementation.

The provided UI screenshot confirms these operator-facing capabilities:

- Live transcription panel with start control.
- Scripture preview and live Scripture panes.
- Scripture history and queue tabs.
- Direct Bible reference entry and search.
- Compare toggle and translation selector.
- Detected verses and sermon-points views.
- AI paraphrase matches with live/saved filtering.
- Display controls with previous, next, display, and hold actions.
- Slides and lyrics modes.

### Installed-resource confirmation

The installed copy at `C:/Program Files/Loghema` makes the following additional capabilities verifiable without decompiling proprietary source:

- NDI output/runtime: `Processing.NDI.Lib.x64.dll` and `windows/ndi-runtime/NDIRedistV6.exe`.
- Offline speech/runtime assets: `windows/onnxruntime/onnxruntime.dll` and `OFFLINE-STT-NOTICES.md`.
- A Rust backend asset layout (`rust-backend-assets`) with an offline `bible-verses.dat` index.
- At least twelve bundled Bible datasets: AMP, AMPC, ERV, French LSG, GNB, ESV, KJV, NASB, NIV, NKJV, NLT, MSG, Spanish 1569, plus a KJV JSON dataset.
- A browser-based display surface with Scripture, lyrics, comparison, sermon notes, sermon points, slides, themes, and lower-third designs.
- Preset display themes and six lower-third templates.
- A remote operator surface (`remote.html`) with Go Live/Stop Live, Scripture Navigator, translation switching, display controls, and mobile-oriented layout.
- Translation and language infrastructure including English, French, Spanish, Hausa, Igbo, Yoruba, and other language labels.

This confirms Loghema's strongest differentiators are the bundled offline runtime, large translation catalog, remote control surface, and broadcast-oriented display/lower-third design system.

## CIP priorities derived from the inspection

1. Add a translation catalog with explicit licensing/source metadata and offline search.
2. Make Scripture detection safer by separating explicit references, confirmed quotations, and low-confidence semantic suggestions.
3. Add a Scripture queue/history workspace with preview, hold, next, previous, and operator approval states.
4. Add song/lyrics import with EasyWorship/OpenLP-compatible adapters.
5. Add NDI output when the official SDK/runtime is available, while preserving OBS/vMix output.
6. Keep Bible text and competitor assets out of CIP unless licensing permits redistribution.
