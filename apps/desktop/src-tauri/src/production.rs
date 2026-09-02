//! Phase 8 (Production Integration): pushes CIP's currently-displayed
//! presentation text into an operator-configured OBS text source and/or
//! vMix title. See `docs/phase-8-audit.md` for the full design reasoning.
//!
//! Deliberately narrow: this module never switches scenes, never toggles
//! source visibility, and never controls recording/streaming - it only
//! ever overwrites one text field the operator has already pointed it
//! at. Config is in-memory/session-scoped (an explicit, named deferred
//! gap - see the audit's "What is explicitly deferred") and live-editable
//! without a restart, unlike this codebase's model-provisioning commands:
//! there is no engine to rebuild here, only connection parameters read
//! fresh on every push.
//!
//! A push is always best-effort and always happens on its own worker
//! thread (mirroring `spawn_acoustic_worker`/`spawn_speech_worker`'s own
//! precedent) - a failure to reach OBS/vMix, or no target configured at
//! all, must never block, delay, or degrade CIP's own local Stage/
//! Confidence/Lobby display.

use chrono::{DateTime, Utc};
use cip_integrations_obs::ObsTarget;
use cip_integrations_vmix::VmixTarget;
use cip_presentation_renderer::RenderedSlide;
use tauri::{AppHandle, Manager};

use crate::logging::LogCategory;
use crate::state::AppState;

/// The operator's current OBS/vMix targets, if any. `None` for a target
/// means that integration is disabled - the default for every new
/// session, so a build with nothing configured behaves identically to
/// before this phase.
#[derive(Debug, Clone, Default)]
pub struct ProductionIntegrationConfig {
    pub obs: Option<ObsTarget>,
    pub vmix: Option<VmixTarget>,
}

/// The outcome of the most recent push attempt to one target, surfaced to
/// the operator via `get_production_integration_status` - never silently
/// dropped on failure.
#[derive(Debug, Clone)]
pub struct PushOutcome {
    pub success: bool,
    pub error_text: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct ProductionIntegrationStatus {
    pub obs_last_push: Option<PushOutcome>,
    pub vmix_last_push: Option<PushOutcome>,
}

/// The plain-text form of a rendered slide this module pushes to OBS/
/// vMix - heading, then every body line, then the footer if present, one
/// per line. Deliberately the simplest honest representation of "what
/// CIP is currently showing," not a themed/formatted overlay - scene/
/// source styling belongs to the operator's own OBS/vMix setup.
pub fn slide_push_text(slide: &RenderedSlide) -> String {
    let mut lines = Vec::with_capacity(slide.body_lines.len() + 2);
    lines.push(slide.heading.clone());
    lines.extend(slide.body_lines.iter().cloned());
    if let Some(footer) = &slide.footer {
        lines.push(footer.clone());
    }
    lines.join("\n")
}

/// Best-effort push of `text` to every currently-configured production
/// target, on a dedicated worker thread - returns immediately, never
/// blocks the caller (see this module's own docs on why). A no-op (no
/// thread spawned at all) when nothing is configured.
pub fn push_to_configured_targets(app: &AppHandle, text: String) {
    let state = app.state::<AppState>();
    let config = state
        .production_integration_config
        .lock()
        .expect("production_integration_config mutex poisoned")
        .clone();
    if config.obs.is_none() && config.vmix.is_none() {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();

        if let Some(target) = &config.obs {
            let outcome = push_obs_and_log(target, &text);
            state
                .production_integration_status
                .lock()
                .expect("production_integration_status mutex poisoned")
                .obs_last_push = Some(outcome);
        }

        if let Some(target) = &config.vmix {
            let outcome = push_vmix_and_log(target, &text);
            state
                .production_integration_status
                .lock()
                .expect("production_integration_status mutex poisoned")
                .vmix_last_push = Some(outcome);
        }
    });
}

fn push_obs_and_log(target: &ObsTarget, text: &str) -> PushOutcome {
    match cip_integrations_obs::push_text(target, text) {
        Ok(()) => {
            log::info!(
                target: LogCategory::Network.target(),
                "production integration: pushed text to OBS source \"{}\"",
                target.source_name
            );
            PushOutcome {
                success: true,
                error_text: None,
                at: Utc::now(),
            }
        }
        Err(e) => {
            log::warn!(
                target: LogCategory::Network.target(),
                "production integration: OBS push failed (source \"{}\"): {e} - CIP's own display is unaffected",
                target.source_name
            );
            PushOutcome {
                success: false,
                error_text: Some(e.to_string()),
                at: Utc::now(),
            }
        }
    }
}

fn push_vmix_and_log(target: &VmixTarget, text: &str) -> PushOutcome {
    match cip_integrations_vmix::push_text(target, text) {
        Ok(()) => {
            log::info!(
                target: LogCategory::Network.target(),
                "production integration: pushed text to vMix input \"{}\"",
                target.input
            );
            PushOutcome {
                success: true,
                error_text: None,
                at: Utc::now(),
            }
        }
        Err(e) => {
            log::warn!(
                target: LogCategory::Network.target(),
                "production integration: vMix push failed (input \"{}\"): {e} - CIP's own display is unaffected",
                target.input
            );
            PushOutcome {
                success: false,
                error_text: Some(e.to_string()),
                at: Utc::now(),
            }
        }
    }
}

/// Blocking connection test - unlike `push_to_configured_targets`, this
/// is called directly from an operator-initiated "Test Connection"
/// button press, so returning the real outcome synchronously (rather
/// than firing a worker thread and polling status) is the more direct,
/// simpler UX; the command wrapping this runs it off the async runtime's
/// own thread pool exactly like every other blocking Tauri command in
/// this codebase already does.
pub fn test_obs_connection(target: &ObsTarget) -> Result<(), String> {
    cip_integrations_obs::push_text(target, "CIP connection test").map_err(|e| e.to_string())
}

pub fn test_vmix_connection(target: &VmixTarget) -> Result<(), String> {
    cip_integrations_vmix::push_text(target, "CIP connection test").map_err(|e| e.to_string())
}
