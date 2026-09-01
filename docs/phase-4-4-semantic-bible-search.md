# Phase 4.4 — True Semantic (Embedding-Based) Bible Search

## Baseline

`docs/phase-4-master-plan-gap-audit.md` graded "Conceptual/semantic
Scripture matching" **NOT STARTED**: no vector/embedding search existed
anywhere in this codebase (confirmed by that audit's own check of
`Cargo.lock` for `candle`/`onnxruntime`/`hnsw`/`faiss`/`usearch`/`tantivy`).
Phase 4.1 delivered the lexical/keyword-overlap slice of Bible-paraphrase
detection but explicitly, honestly left the harder tier unaddressed: *"A
paraphrase sharing little or no vocabulary with its source verse (e.g.
'Jesus said we should love our enemies' for Matthew 5:44) will not be
found by Phase 4.1's lexical-overlap detector."*

## Why this phase exists

After the Phase 4.3 (Instant Bible Detection) competitive analysis, the
operator gave an explicit, non-negotiable constraint for whatever came
next: *"Remember CIP should run not on an api that needs subscription
every month but on a free model."* This ruled out every cloud embedding
API from the outset (OpenAI, Cohere, Voyage) and framed the whole phase as
"local inference or nothing."

## Architecture decisions (verified, not assumed)

- **candle over ONNX Runtime**: a real, disposable smoke-compile test
  confirmed `candle`/`tokenizers` cross-compile cleanly to
  `x86_64-pc-windows-gnu` with no CMake/C++ build step of its own -
  unlike `whisper-rs-sys`'s multi-fix cross-compile saga (see
  `docs/phase-3-8-6-windows-whisper-build.md`). This phase's real Windows
  rebuild (see below) confirmed the finding held: zero new toolchain fixes
  were needed for the `semantic-search` feature.
- **all-MiniLM-L6-v2**: `candle-transformers`' own `bert.rs` source has
  this exact model's architecture hardcoded (a private
  `_all_mini_lm_l6_v2()` constructor). Reproducing those published values
  publicly in `ai/embeddings/src/candle_engine.rs` means the operator
  supplies only two files (`model.safetensors` + `tokenizer.json`), never
  a third `config.json` - simplifying provisioning to mirror
  `WHISPER_MODEL_FILENAME`'s own single-file precedent as closely as the
  model architecture allows. User confirmed this choice explicitly:
  *"Yes, proceed with all-MiniLM-L6-v2."*

See `pilot-evidence/4.4/build/semantic-search-evidence.json` for the full
backend-selection, model-selection, storage-design, and concurrency-design
reasoning.

## What was built

- **`core/ai::EmbeddingEngine`** - the provider/adaptor trait, mirroring
  `SpeechEngine` exactly: `is_ready()`, `model_id()`, `dimensions()`,
  `embed(text) -> Result<Vec<f32>, EmbeddingEngineError>`.
- **`core/bible::semantic`** - `cosine_similarity`, `is_valid_embedding`,
  the `VerseEmbeddingStore` trait (retrieval only, deliberately separate
  from `BibleProvider` since not every provider has embeddings computed),
  and `best_semantic_match` (scores every candidate, returns the winner
  above a minimum similarity).
- **`core/bible::ReferenceKind::Semantic`** - a new detection kind,
  mirroring `Paraphrase`'s "never produced by `detect_candidates`, only by
  a fallback function" pattern exactly.
- **`ai/embeddings`** - `NullEmbeddingEngine` (always compiled, safe
  default) and `CandleEmbeddingEngine` (real local backend, behind the
  `semantic-search` Cargo feature) + `pooling` (mean-pool/L2-normalize,
  pure functions, always compiled and unit-tested independent of the
  heavy feature).
- **`core/service::try_semantic`** - the live-pipeline fallback, wired via
  a new `process_transcript_segment_with_semantic_search` entry point
  (the existing `process_transcript_segment` is untouched - additive, not
  breaking, mirroring how Phase 4.1's paraphrase fallback was added).
  Attempted only after both an explicit citation and the lexical
  paraphrase fallback have already failed; never mutates the active
  Scripture context; re-validates the winning reference against the real
  `BibleProvider` before ever becoming a suggestion.
- **`database` migration `0013_bible_verse_embeddings`** - one `BLOB` row
  per `(verse_id, model_id)`, keyed so a model switch never silently mixes
  vectors from two different models into one similarity comparison.
- **`apps/desktop`**:
  - `AppConfig::embedding_model_path`/`embedding_tokenizer_path`, with
    `CIP_EMBEDDING_MODEL_PATH`/`CIP_EMBEDDING_TOKENIZER_PATH` overrides,
    mirroring `whisper_model_path` exactly.
  - `create_embedding_engine` (mirrors `create_speech_engine`): a real
    model if the feature is compiled in *and* both files are present,
    `NullEmbeddingEngine` otherwise - missing/invalid model is never
    fatal.
  - `SqliteVerseEmbeddingStore` on its **own dedicated connection**
    (mirroring `SqliteBibleProvider`'s exact shape - `rusqlite::Connection`
    is not `Sync`, and `VerseEmbeddingStore` requires it; a first attempt
    at borrowing the live pipeline's already-locked connection failed to
    compile with a direct `E0277` error, caught before it ever reached a
    real build).
  - `generate_verse_embeddings_for_translation` - embeds every verse of
    a translation not already embedded under the engine's current
    `model_id`; idempotent/resumable; holds its connection's lock only
    briefly per verse, never for the whole run.
  - Four new Tauri commands: `get_embedding_capabilities`,
    `install_embedding_model_file`, `install_embedding_tokenizer_file`
    (mirror `install_whisper_model`'s copy-into-place pattern), and
    `generate_verse_embeddings` (the operator-triggered action that
    populates the table - nothing does this automatically).
  - Live wiring: `finalize_bible_only` and `process_test_transcript` both
    now use `process_transcript_segment_with_semantic_search` whenever
    `AppState.embedding_ready` is true, constructing a `SemanticSearch`
    from the dedicated embedding connection - every other caller (every
    existing test, every other command) keeps using the plain,
    already-proven entry point unchanged.

## Local, free-model path confirmed - no cloud API considered or added

Every embedding backend evaluated was local-only from the start,
mirroring Phase 4.3's own free-local-model constraint for speech. No
OpenAI/Cohere/Voyage embedding API, and no subscription of any kind, was
evaluated or added. `NullEmbeddingEngine` guarantees "no model configured"
degrades to exactly Phase 4.1's existing lexical-paraphrase behavior,
never a hard failure.

## Full regression result

`cargo fmt --check`: clean. `cargo clippy --workspace --all-targets --
-D warnings`: clean under **both** default features and
`--features whisper,semantic-search` together. `cargo test --workspace`:
every crate green under both feature configurations (53 test binaries,
zero failures); `cip-desktop` alone: 289 passed (up from 280 at the Phase
4.3 baseline - 9 new tests: 2 in `config.rs`, 7 in the new
`embeddings.rs`). New test coverage besides the desktop crate: 5 tests in
`core/service::bible_intelligence` (semantic detection, opt-in-only
behavior, never second-guessing a citation, never mutating context, never
suggesting an unvalidated reference), 6 in `core/bible::semantic`
(`best_semantic_match` scoring/threshold/model-isolation/dimension-safety),
2 in `core/ai`'s `EmbeddingEngine` trait tests, 7 in `ai/embeddings`'s
pooling module, 2 in the database migration.

## Windows rebuild

`scripts/build-windows-whisper.sh` was extended to pass
`--features whisper,semantic-search` to every `cargo build`/`tauri build`
invocation (previously `whisper` only) - see the script's own updated
comment block for why `candle` needed none of the `whisper-rs-sys`-specific
fixes. The rebuild succeeded with zero new fixes required. Installer:
`Church Intelligence Platform_0.1.0_x64-setup.exe`, SHA-256
`46a78173c32990b161836f360e18c28ed92d00b2d5cdd859b68ba69b97d431bb`,
13,738,939 bytes (up from 8,645,963 bytes at the Phase 4.3 baseline - the
expected size increase for the new candle/tokenizers dependency set).
`cip-desktop.exe` itself: 58,723,639 bytes. Direct binary proof (new
symbols, new command names, prior-phase symbols confirmed unaffected) is
in `pilot-evidence/4.4/windows/installer-contents-verification.json`.

## Architectural safety diff

- Zero changes to `process_transcript_segment`'s existing signature or
  behavior - the semantic fallback is reached only through a new,
  separate entry point.
- Zero changes to `handle_final_transcript`'s existing signature or
  behavior - same additive pattern
  (`handle_final_transcript_with_semantic_search`).
- Zero changes to any existing database table or column - only a new
  table (`bible_verse_embeddings`).
- Zero changes to the Whisper/speech pipeline, the paraphrase fallback,
  context management, dedup, or presentation - this phase adds a further
  fallback stage after both already run, nothing upstream of it changed.
- `AppState::new`'s parameter list grew (two new arguments); the one call
  site (`lib.rs`'s `setup` hook) was updated - no other constructor
  exists.

## Environment A / B / C

- **Environment A** (this container: compile, lint, unit/integration
  tests, direct binary inspection of the cross-compiled artifact):
  PASSED, fully green, as detailed above.
- **Environment B** (Xvfb GUI reproduction): unavailable in this
  session's container, a pre-existing, already-documented limitation
  since Phase 3.8.5 - not this phase's regression.
- **Environment C** (real Windows hardware, with a real
  `model.safetensors`/`tokenizer.json` pair installed): **NOT YET
  VERIFIED**. This container cannot download a real embedding model
  (the standard model host is blocked by this environment's egress
  policy, exactly as already documented for the Whisper model - see
  `modelPackagingStatement` in `release/windows/release-manifest.json`).
  The decisive pending gate is the operator's own real-hardware test:
  install both model files via the new "Select Existing Model File"-style
  pickers, run `generate_verse_embeddings`, then confirm a conceptual
  paraphrase (sharing little/no vocabulary with its source verse) is
  correctly surfaced as a `Pending` suggestion.

## Known limitations

- **No real embedding model was ever loaded or run in this container** -
  every test above uses either pure math (`cosine_similarity` on
  hand-constructed vectors) or a deterministic fake `EmbeddingEngine`.
  `CandleEmbeddingEngine::load`/`embed`'s actual runtime correctness
  against a real `model.safetensors` file has not been exercised anywhere
  in this project's history. This is the single largest open risk this
  phase carries forward.
- **`MIN_SEMANTIC_SIMILARITY = 0.55` is documented, not empirically
  calibrated** - chosen from published `all-MiniLM-L6-v2` benchmark
  ranges (related pairs 0.6-0.9, unrelated pairs below 0.3), not tuned
  against a labeled dataset, since no real model inference was possible
  here. Revisit once real operator feedback on live services exists.
- **No progress reporting during `generate_verse_embeddings`** - it runs
  synchronously on the calling command thread (Tauri dispatches command
  handlers off the main UI thread, so the app itself does not freeze) but
  reports no incremental progress while it runs; for a full-Bible
  translation on CPU this can take minutes. A future phase could add
  progress events if this proves too opaque in practice.
- **Semantic manual Bible search is not wired up** - this phase only
  reaches the *live detection* pipeline. The Bible Library's manual search
  box (`search_bible`) is still FTS/keyword-only; wiring
  `best_semantic_match` into it (and cross-reference intelligence, which
  falls out of the same infrastructure for free) is a natural, low-effort
  follow-up, not attempted this phase.
- **No hot-swap of a running embedding engine** - installing a new
  model/tokenizer pair takes effect on CIP's next launch, mirroring
  `install_whisper_model`'s own documented limitation exactly.
- Every limitation already documented for Phase 4.1's lexical paraphrase
  detector, Phase 4.2's SIMD fix, and Phase 4.3's fast detection lane
  still applies unchanged - this phase adds a further fallback stage, it
  does not revisit or resolve any of them.

## Deferred work

- Semantic manual Bible search + cross-reference intelligence (same
  infrastructure, different wiring - see "Known limitations" above).
- Confidence-ring UI for `Semantic`-kind suggestions (the `Ambiguous`
  candidate-ranking UI pattern already exists; extending it to semantic
  hits is presentation-layer work only).
- Real embedding model Environment C verification (the decisive pending
  gate named above).

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real model file +
real Windows hardware, both outside this container's reach). This phase
is a real, verifiable, fully-tested software release candidate for the
`semantic-search` feature's plumbing and live-pipeline wiring; whether a
real `all-MiniLM-L6-v2` model genuinely improves detection on a live
service is an operator-verified claim this document does not make.
