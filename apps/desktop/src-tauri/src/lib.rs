mod commands;
mod config;
mod content;
mod errors;
pub mod events;
mod intelligence;
pub mod logging;
mod music;
mod persistence;
mod pipeline;
mod presentation;
mod state;
mod timeline;

use cip_core_ai::SpeechEngine;
use config::AppConfig;
use logging::LogCategory;
use state::AppState;
use tauri::Manager;

/// Choose a `SpeechEngine`: a local Whisper model if the `whisper` feature
/// is compiled in *and* a model file is actually present at the
/// configured path, `NullSpeechEngine` otherwise. Missing/no model is
/// never fatal - see `docs/live-speech.md`'s "model absence" section.
#[cfg_attr(not(feature = "whisper"), allow(unused_variables))]
fn create_speech_engine(config: &AppConfig) -> Box<dyn SpeechEngine> {
    #[cfg(feature = "whisper")]
    {
        let model_path = config.model_dir.join(config::WHISPER_MODEL_FILENAME);
        match cip_ai_speech::WhisperSpeechEngine::load(&model_path) {
            Ok(engine) => {
                log::info!(target: LogCategory::Speech.target(), "loaded local speech model from {}", model_path.display());
                return Box::new(engine);
            }
            Err(e) => {
                log::warn!(
                    target: LogCategory::Speech.target(),
                    "local speech model not available ({e}); live transcription is unavailable until one is configured"
                );
            }
        }
    }
    #[cfg(not(feature = "whisper"))]
    log::info!(
        target: LogCategory::Speech.target(),
        "built without the `whisper` feature; live transcription is unavailable (manual operation still works)"
    );

    Box::new(cip_ai_speech::NullSpeechEngine)
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
        .setup(|app| {
            let handle = app.handle();
            let config = AppConfig::resolve(handle)?;

            log::info!(
                target: LogCategory::App.target(),
                "starting CIP desktop (environment: {:?}, data_dir: {})",
                config.environment,
                config.data_dir.display()
            );

            let mut db = cip_database::open(&config.database_path)?;
            let applied = cip_database::run_migrations(&mut db)?;
            log::info!(target: LogCategory::Database.target(), "{} migration(s) applied", applied.len());

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

            let audio_engine: Box<dyn cip_core_service::AudioEngine> =
                Box::new(cip_integrations_audio::CpalAudioEngine::new());
            let speech_engine = create_speech_engine(&config);

            app.manage(AppState::new(
                config,
                db,
                bible_provider,
                content_registry,
                intelligence_registry,
                music_provider,
                audio_engine,
                speech_engine,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_config,
            commands::app_health_check,
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
            commands::search_bible,
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
            commands::get_live_status,
            commands::list_timeline,
            commands::list_service_history,
            commands::get_service,
            commands::resolve_ambiguous_reference,
            commands::correct_scripture_context,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
