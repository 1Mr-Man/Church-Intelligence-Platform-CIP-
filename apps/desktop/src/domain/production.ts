/**
 * Production Integration domain contracts (Phase 8). Mirrors
 * `apps/desktop/src-tauri/src/production.rs` and the wire-format DTOs in
 * `commands.rs` (`ObsTargetConfig`/`VmixTargetConfig`/
 * `ProductionIntegrationConfigInput`/`ProductionIntegrationStatusDto`).
 * Pushes CIP's currently-displayed presentation text into an
 * operator-configured OBS text source and/or vMix title - see
 * `docs/phase-8-audit.md` for the full design reasoning.
 */

export interface ObsTargetConfig {
  host: string;
  port: number;
  password: string | null;
  sourceName: string;
}

export interface VmixTargetConfig {
  host: string;
  port: number;
  input: string;
  selectedName: string | null;
}

export interface ProductionIntegrationConfigInput {
  obs: ObsTargetConfig | null;
  vmix: VmixTargetConfig | null;
}

export interface PushOutcome {
  success: boolean;
  errorText: string | null;
  at: string;
}

export interface ProductionIntegrationStatus {
  obsLastPush: PushOutcome | null;
  vmixLastPush: PushOutcome | null;
}
