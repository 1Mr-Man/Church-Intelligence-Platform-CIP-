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
  /** Phase 3.0: exact file path the speech engine looks for a local
   * Whisper model at - overridable via CIP_WHISPER_MODEL_PATH. Surfaced in
   * the UI so "speech unavailable" names where to put a model, not just
   * that one is missing. */
  whisperModelPath: string;
  logDir: string;
}
