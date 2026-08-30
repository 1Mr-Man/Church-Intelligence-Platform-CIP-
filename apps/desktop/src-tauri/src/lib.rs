mod acoustic;
mod bible_production_dataset;
mod commands;
mod config;
mod content;
mod content_intelligence;
mod cross_domain;
mod errors;
pub mod events;
mod harvest;
mod intelligence;
pub mod logging;
mod music;
mod persistence;
mod pilot_evidence;
mod pipeline;
mod presentation;
mod presentation_display;
mod segmentation;
mod sermon;
mod sermon_foundation;
mod service;
mod state;
mod timeline;

use cip_core_ai::SpeechEngine;
use cip_core_music::AcousticMusicRecognizer;
use config::AppConfig;
use logging::LogCategory;
use state::AppState;
use tauri::Manager;

/// Choose a `SpeechEngine`: a local Whisper model if the `whisper` feature
/// is compiled in *and* a model file is actually present at the
/// configured path, `NullSpeechEngine` otherwise. Missing/no model is
/// never fatal - see `docs/live-speech.md`'s "model absence" section.
///
/// Phase 3.8.6: also returns a [`state::SpeechDiagnostics`] snapshot of
/// what this attempt actually observed - previously the real
/// `SpeechEngineError` text from a failed load was logged with
/// `log::warn!` and then discarded, so no command could ever report *why*
/// live transcription was unavailable on a build that does have the
/// `whisper` feature compiled in. Nothing about engine selection itself
/// changes: a missing/invalid model still falls back to `NullSpeechEngine`,
/// never fatal.
#[cfg_attr(not(feature = "whisper"), allow(unused_variables))]
fn create_speech_engine(config: &AppConfig) -> (Box<dyn SpeechEngine>, state::SpeechDiagnostics) {
    let feature_compiled = cfg!(feature = "whisper");
    #[cfg(feature = "whisper")]
    let result: (Box<dyn SpeechEngine>, state::SpeechDiagnostics) = {
        let model_path = &config.whisper_model_path;
        match cip_ai_speech::WhisperSpeechEngine::load(model_path) {
            Ok(engine) => {
                log::info!(target: LogCategory::Speech.target(), "loaded local speech model from {}", model_path.display());
                (
                    Box::new(engine),
                    state::SpeechDiagnostics {
                        feature_compiled,
                        model_load_attempted: true,
                        model_loaded: true,
                        model_load_error: None,
                        ..Default::default()
                    },
                )
            }
            Err(e) => {
                log::warn!(
                    target: LogCategory::Speech.target(),
                    "local speech model not available ({e}); live transcription is unavailable until one is configured"
                );
                (
                    Box::new(cip_ai_speech::NullSpeechEngine),
                    state::SpeechDiagnostics {
                        feature_compiled,
                        model_load_attempted: true,
                        model_loaded: false,
                        model_load_error: Some(e.to_string()),
                        ..Default::default()
                    },
                )
            }
        }
    };
    #[cfg(not(feature = "whisper"))]
    let result: (Box<dyn SpeechEngine>, state::SpeechDiagnostics) = {
        log::info!(
            target: LogCategory::Speech.target(),
            "built without the `whisper` feature; live transcription is unavailable (manual operation still works)"
        );
        (
            Box::new(cip_ai_speech::NullSpeechEngine),
            state::SpeechDiagnostics {
                feature_compiled,
                ..Default::default()
            },
        )
    };
    result
}

/// Choose an `AcousticMusicRecognizer` (Phase 2.2): `LocalAcousticMusicRecognizer`,
/// configured from `AppConfig.acoustic` - honestly reports `Disabled`/
/// `Unavailable`/`Error` (never fake recognition) when turned off, or when
/// no model manifest is configured/present/well-formed, exactly mirroring
/// `create_speech_engine`'s "missing model is never fatal" discipline. See
/// `docs/acoustic-music.md` for why this build never reports `Available`.
fn create_acoustic_recognizer(config: &AppConfig) -> Box<dyn AcousticMusicRecognizer> {
    let recognizer = cip_integrations_music_acoustic::LocalAcousticMusicRecognizer::configure(
        cip_integrations_music_acoustic::LocalAcousticConfig {
            model_dir: Some(config.acoustic.model_dir.clone()),
            enabled: config.acoustic.enabled,
        },
    );
    log::info!(
        target: LogCategory::Music.target(),
        "acoustic recognizer status: {:?} ({})",
        recognizer.status(),
        recognizer.status_reason().unwrap_or_default()
    );
    Box::new(recognizer)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
                ])
                .build(),
        )
        // Phase 3.8.7.1: the ONLY plugin capability added for model
        // provisioning - a native "open file" dialog so an operator can
        // pick a Whisper model file they've already downloaded themselves,
        // scoped via capabilities/default.json to `dialog:allow-open` only
        // (no save dialog, no message boxes). The display window's own
        // capability deliberately does not grant this - see
        // capabilities/display.json.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle();
            let config = AppConfig::resolve(handle)?;

            log::info!(
                target: LogCategory::App.target(),
                "starting CIP desktop (environment: {:?}, data_dir: {})",
                config.environment,
                config.data_dir.display()
            );

            // Phase 3.8.7.1: `cip_database::open` creates `config.data_dir`
            // itself as a side effect of opening the sqlite file under it,
            // but `config.model_dir` is a sibling subdirectory that
            // nothing else touches - an operator following the documented
            // "place ggml-tiny.en.bin under .../models/" instructions on a
            // fresh install would otherwise have to create that folder by
            // hand first. Created unconditionally and ignored if it
            // already exists; never fails startup if this can't be
            // created (e.g. a read-only data dir) - the existing
            // `WhisperModelDiagnostic::Missing`/`Unreadable` reporting
            // already covers that case honestly.
            if let Err(e) = std::fs::create_dir_all(&config.model_dir) {
                log::warn!(
                    target: LogCategory::Speech.target(),
                    "could not create model directory {}: {e}",
                    config.model_dir.display()
                );
            }

            let mut db = cip_database::open(&config.database_path)?;
            let applied = cip_database::run_migrations(&mut db)?;
            log::info!(target: LogCategory::Database.target(), "{} migration(s) applied", applied.len());

            // Local presentation display foundation: restart must never
            // automatically project anything (see `presentation_display.rs`'s
            // module docs and `docs/presentation.md`). Any presentation
            // item left `Active` by a previous, uncleanly-ended run is
            // reconciled to `Stopped` here, before anything else touches
            // presentation state or any window is created.
            let reconciled = persistence::reconcile_stale_active_presentation_items(&db)?;
            if reconciled > 0 {
                log::info!(
                    target: LogCategory::Presentation.target(),
                    "reconciled {reconciled} presentation item(s) left active by a previous run to stopped"
                );
            }

            // Dev/test convenience only: a handful of verses so the UI has
            // something to query. Never applied in Production, and never a
            // full Bible dataset - see database/seeds/dev_seed.sql.
            if config.environment != config::AppEnvironment::Production {
                let translation_count: i64 =
                    db.query_row("SELECT count(*) FROM bible_translations", [], |row| row.get(0))?;
                if translation_count == 0 {
                    cip_database::seed::apply_dev_seed(&db)?;
                    log::info!(target: LogCategory::Database.target(), "dev seed applied");
                }
            }

            let bible_conn = cip_database::open(&config.database_path)?;
            let bible_provider = Box::new(cip_integrations_bible::SqliteBibleProvider::new(bible_conn));

            let content_conn = cip_database::open(&config.database_path)?;
            let content_registry = Box::new(cip_integrations_content::SqliteContentRegistry::new(
                content_conn,
            ));
            // Dev/test convenience only, mirroring the dev-seed guard
            // above: the dev-seeded KJV translation gets a Content
            // Registry entry too, so it shows up in diagnostics - with
            // every licensing field honestly `UNKNOWN` (`None`), since
            // the dev fixture's real provenance was never recorded. See
            // docs/bible-datasets.md.
            if config.environment != config::AppEnvironment::Production {
                content::register_dev_seed_content_if_missing(content_registry.as_ref())?;
            }

            // Real Bible dataset production import milestone: the complete
            // 66-book Berean Standard Bible (docs/bible-production-dataset.md)
            // installs here, idempotently, in every environment including
            // Production - unlike the dev fixture above, this is real
            // product content the app always needs, not test data.
            // `import_and_register`'s licensing gate and transactional
            // writes make this safe to call on every launch: the first
            // launch inserts everything, every later launch finds it all
            // already present and writes nothing (see
            // `bible_production_dataset.rs`'s module docs).
            // Phase 3.0: a BSB import failure is no longer allowed to crash
            // the entire application before any window renders. Every
            // other domain in this function already degrades on its own
            // failure (a missing speech model, an absent audio device, no
            // acoustic recognizer) without taking the rest of CIP down -
            // Bible readiness now follows the same discipline. The
            // operator sees this as `LiveStatus.bible == null` (via
            // `get_live_status`) and the "Bible: NOT AVAILABLE" header,
            // never a silent crash with no explanation. A real database or
            // migration failure above remains fatal, since nothing in CIP
            // can function without a database at all - this is narrower:
            // one dataset failing to import, not the storage layer itself.
            match content::import_and_register(
                &db,
                content_registry.as_ref(),
                &bible_production_dataset::bsb_dataset(),
            ) {
                Ok(bsb_report) => log::info!(
                    target: LogCategory::Database.target(),
                    "{} production Bible dataset: {} book(s), {} chapter(s), {} verse(s) total ({} imported, {} already present)",
                    bible_production_dataset::BSB_TRANSLATION_ID,
                    bsb_report.books,
                    bsb_report.chapters,
                    bsb_report.verses_total,
                    bsb_report.imported,
                    bsb_report.already_present
                ),
                Err(e) => log::error!(
                    target: LogCategory::Database.target(),
                    "production Bible dataset import failed: {e} - CIP will start, but Bible search/detection/presentation will be unavailable until this is resolved and CIP is restarted"
                ),
            }

            // Phase 2.0: the intelligence engine registry gets its own
            // BibleProvider connection too, mirroring `bible_conn`/
            // `content_conn` above - an independent read path for the
            // Bible compatibility adapter's own Scripture Context state,
            // never contending with the live pipeline's connection.
            let intelligence_bible_conn = cip_database::open(&config.database_path)?;
            let mut intelligence_registry = intelligence::build_registry(
                Box::new(cip_integrations_bible::SqliteBibleProvider::new(
                    intelligence_bible_conn,
                )),
                state::DEFAULT_TRANSLATION_ID,
            );

            // Phase 2.1: Music's own read path, mirroring `bible_conn`
            // above, plus a second dedicated connection for the Music
            // engine's own copy of the same provider - the engine and
            // `AppState.music_provider` never share a connection, same
            // discipline as Bible's `intelligence_bible_conn`/`bible_conn`
            // split.
            let music_conn = cip_database::open(&config.database_path)?;
            let music_provider = Box::new(cip_integrations_music::SqliteMusicProvider::new(music_conn));

            let intelligence_music_conn = cip_database::open(&config.database_path)?;
            music::register_music_engine(
                &mut intelligence_registry,
                Box::new(cip_integrations_music::SqliteMusicProvider::new(
                    intelligence_music_conn,
                )),
            )?;

            // Dev/test convenience only, mirroring the Bible/content
            // dev-seed guards above: register the three dev-fixture music
            // datasets so Music Intelligence has something to recognize
            // against outside Production.
            if config.environment != config::AppEnvironment::Production {
                music::register_dev_seed_music_content_if_missing(content_registry.as_ref())?;
            }

            // Phase 2.3: a Sermon engine registered for diagnostic/
            // failure-isolation symmetry with Bible/Music only - see
            // `sermon.rs`'s module docs for why the app's actual, live-used
            // instance is a separate one on `AppState.sermon_engine`.
            // Needs no provider/database connection, unlike Bible/Music.
            sermon::register_sermon_engine(&mut intelligence_registry)?;
            log::info!(
                target: LogCategory::App.target(),
                "sermon intelligence engine initialized (deterministic, offline)"
            );

            // Phase 2.4 (Service Intelligence, per the authoritative Phase
            // 2 roadmap): a Service engine registered for diagnostic/
            // failure-isolation symmetry with Bible/Music/Sermon only -
            // see `service.rs`'s module docs for why the app's actual,
            // live-used instance is a separate one on
            // `AppState.service_engine`. Needs no provider/database
            // connection, unlike Bible/Music.
            service::register_service_engine(&mut intelligence_registry)?;
            log::info!(
                target: LogCategory::App.target(),
                "service intelligence engine initialized (deterministic, offline)"
            );

            // Phase 2.5 (Sermon Foundation, per the authoritative Phase 2
            // roadmap): deliberately has no `IntelligenceEngine` to
            // register - see `sermon_foundation.rs`'s module docs. State
            // lives on `AppState.active_sermon`/`active_sermon_section`,
            // initialized empty in `AppState::new` below.
            log::info!(
                target: LogCategory::App.target(),
                "sermon foundation initialized (deterministic, offline, no engine registration)"
            );

            // Phase 2.7 (Content Intelligence, per the authoritative Phase
            // 2 roadmap): also has no `IntelligenceEngine` to register -
            // `ContentIntelligenceEngine` produces `ContentCandidate`s, a
            // structurally different type than `IntelligenceFinding`, the
            // same reason `CrossDomainCorrelationEngine` (Phase 2.4) isn't
            // registered either. See `content_intelligence.rs`'s module
            // docs. State lives on `AppState.content_candidate_queue`,
            // initialized empty in `AppState::new` below.
            log::info!(
                target: LogCategory::App.target(),
                "content intelligence initialized (deterministic, offline, no engine registration)"
            );

            let audio_engine: Box<dyn cip_core_service::AudioEngine> =
                Box::new(cip_integrations_audio::CpalAudioEngine::new());
            let (speech_engine, speech_diagnostics) = create_speech_engine(&config);

            // Phase 2.2: a fourth independent Music read path, dedicated
            // to acoustic analysis - same "every independent read path
            // gets its own connection" discipline as
            // `intelligence_music_conn`/`music_conn` above (see
            // `state::AppState::acoustic_music_engine`'s docs for why this
            // can't just reuse the trait object already registered in
            // `intelligence_registry`).
            let acoustic_music_conn = cip_database::open(&config.database_path)?;
            let acoustic_music_engine = cip_core_intelligence::MusicIntelligenceEngine::new(
                Box::new(cip_integrations_music::SqliteMusicProvider::new(
                    acoustic_music_conn,
                )),
            );
            let acoustic_recognizer = create_acoustic_recognizer(&config);

            app.manage(AppState::new(
                config,
                db,
                bible_provider,
                content_registry,
                intelligence_registry,
                music_provider,
                audio_engine,
                speech_engine,
                speech_diagnostics,
                acoustic_music_engine,
                acoustic_recognizer,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_config,
            commands::app_health_check,
            commands::get_pilot_diagnostics,
            commands::install_whisper_model,
            commands::backup_database,
            commands::list_bible_translations,
            commands::start_service,
            commands::pause_service,
            commands::resume_service,
            commands::end_service,
            commands::list_audio_devices,
            commands::start_listening,
            commands::stop_listening,
            commands::process_test_transcript,
            commands::list_transcript,
            commands::list_suggestions,
            commands::approve_suggestion,
            commands::edit_suggestion,
            commands::reject_suggestion,
            commands::preview_presentation,
            commands::preview_scripture,
            commands::prepare_presentation,
            commands::create_manual_presentation,
            commands::list_prepared_presentations,
            commands::get_presentation_item,
            commands::cancel_presentation,
            commands::open_presentation_display,
            commands::get_presentation_display_state,
            commands::display_presentation,
            commands::clear_presentation_display,
            commands::close_presentation_display,
            commands::log_display_diagnostic,
            commands::list_presentation_history,
            commands::search_bible,
            commands::list_bible_books,
            commands::save_scripture,
            commands::list_saved_scriptures,
            commands::delete_saved_scripture,
            commands::list_content_registry,
            commands::get_content_metadata,
            commands::set_content_enabled,
            commands::import_bible_dataset,
            commands::check_bible_dataset_integrity,
            commands::get_intelligence_capabilities,
            commands::search_music,
            commands::import_music_dataset,
            commands::analyze_music_transcript,
            commands::list_music_findings,
            commands::accept_music_finding,
            commands::reject_music_finding,
            commands::clear_current_song,
            commands::analyze_music_audio,
            commands::get_live_status,
            commands::list_timeline,
            commands::list_service_history,
            commands::get_service,
            commands::resolve_ambiguous_reference,
            commands::correct_scripture_context,
            commands::analyze_sermon_transcript,
            commands::list_sermon_findings,
            commands::accept_sermon_finding,
            commands::reject_sermon_finding,
            commands::get_sermon_state,
            commands::get_sermon_foundation_state,
            commands::start_sermon,
            commands::pause_sermon,
            commands::resume_sermon,
            commands::end_sermon,
            commands::set_sermon_title,
            commands::assign_sermon_speaker,
            commands::change_sermon_section,
            commands::link_transcript_segment_to_sermon,
            commands::list_sermon_segments,
            commands::list_sermon_sections,
            commands::list_sermon_history,
            commands::get_sermon,
            commands::harvest_sermon,
            commands::analyze_bible_transcript,
            commands::analyze_cross_domain,
            commands::list_cross_domain_correlations,
            commands::review_cross_domain_correlation,
            commands::dismiss_cross_domain_correlation,
            commands::analyze_content_intelligence,
            commands::list_content_candidates,
            commands::list_accepted_content_candidates,
            commands::accept_content_candidate,
            commands::reject_content_candidate,
            commands::list_saved_content,
            commands::analyze_service_transcript,
            commands::get_service_intelligence_state,
            commands::list_service_transitions,
            commands::list_service_anomalies,
            commands::mark_service_phase,
            commands::correct_service_phase,
            commands::acknowledge_service_anomaly,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
