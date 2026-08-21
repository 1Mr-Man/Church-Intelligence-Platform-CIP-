/**
 * Frontend mirror of `apps/desktop/src-tauri/src/config.rs`'s `AppConfig`.
 * The frontend never resolves these paths itself - it only reads what the
 * backend already resolved, via the `get_app_config` command.
 */

export type AppEnvironment = "development" | "test" | "production";

export interface AppConfig {
  environment: AppEnvironment;
  dataDir: string;
  databasePath: string;
  modelDir: string;
  logDir: string;
}
