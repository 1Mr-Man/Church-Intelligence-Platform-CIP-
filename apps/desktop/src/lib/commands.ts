/**
 * Typed wrappers around the Tauri commands registered in
 * `apps/desktop/src-tauri/src/lib.rs`'s `invoke_handler`. Keeping `invoke`
 * calls behind named, typed functions (rather than calling `invoke` inline
 * from components) is the one indirection Phase 1 needs: it's the single
 * place that has to change if a command's name or payload shape changes.
 */
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, AppEnvironment } from "../config/appConfig";
import type { BibleTranslation } from "../domain";

export interface HealthReport {
  databaseConnected: boolean;
  appliedMigrations: number;
  environment: AppEnvironment;
}

export function getAppConfig(): Promise<AppConfig> {
  return invoke("get_app_config");
}

export function appHealthCheck(): Promise<HealthReport> {
  return invoke("app_health_check");
}

export function listBibleTranslations(): Promise<BibleTranslation[]> {
  return invoke("list_bible_translations");
}
