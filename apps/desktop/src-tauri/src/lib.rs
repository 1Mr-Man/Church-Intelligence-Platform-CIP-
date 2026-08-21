mod commands;
mod config;
mod errors;
pub mod events;
pub mod logging;
mod state;

use config::AppConfig;
use logging::LogCategory;
use state::AppState;
use tauri::Manager;

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

            app.manage(AppState::new(config, db, bible_provider));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_config,
            commands::app_health_check,
            commands::list_bible_translations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
