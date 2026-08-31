//! Phase 3.10.2: the Display Registry - a logical model of which physical
//! monitor plays which presentation role, built on the monitor-enumeration
//! infrastructure `get_pilot_diagnostics` has used since Phase 3.2-3.4
//! (`AppHandle::available_monitors`/`primary_monitor`), and the
//! `presentation_display::DisplayScreen` window-role pattern from Phase
//! 3.10 - unifying the two for the first time. See
//! `docs/phase-3-10-1-audit.md` sections C/E/F for the audit that
//! identified both as the real, already-available extension points this
//! module now uses, and `docs/phase-3-10-2-display-registry.md` for this
//! phase's own design notes.
//!
//! ## A real naming distinction, stated plainly
//!
//! [`DisplayRole::Projector`] here is the *physical monitor* role for
//! congregation-facing output - it is deliberately not called `Stage`,
//! because `presentation_display::DisplayScreen::Stage` already means
//! something specific (the primary window/content stream, unchanged
//! since Phase 3.10) and this module must not silently collide with it.
//! [`DisplayRole::Stage`] here means "speaker-facing information" per the
//! operator's own target architecture - a role with no corresponding
//! `DisplayScreen`/content stream built yet (see [`screen_role`]'s docs).
//!
//! ## What this module does NOT do
//!
//! It does not touch `PresentationItem`/`PresentationContent` rendering,
//! does not change the `Prepared -> Active -> Stopped` lifecycle, and does
//! not change the async-command / `WebviewWindowBuilder` / resize-nudge
//! ordering `presentation_display::open_display_window` already uses -
//! per the operator's own explicit instruction, this phase only adds an
//! optional [`MonitorPlacement`] input to that existing, otherwise-
//! untouched function.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

/// The role a physical monitor is assigned in the Display Registry -
/// distinct from [`crate::presentation_display::DisplayScreen`] (see this
/// module's own docs above). `Unassigned` is the default for every
/// newly-detected monitor; nothing is auto-assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayRole {
    Unassigned,
    /// The operator's own laptop/desktop screen - never a presentation
    /// target. Recorded so the registry can be self-documenting (an
    /// operator can mark "this is my screen" to keep it out of the
    /// Projector/Stage/Confidence/Lobby candidate list), not because any
    /// code branches on it today.
    Operator,
    /// Congregation-facing output (Bible verses, lyrics, sermon content)
    /// - maps to `DisplayScreen::Stage` (see [`screen_role`]).
    Projector,
    /// Speaker-facing information - no corresponding `DisplayScreen`/
    /// content stream exists yet (Phase 3.10.3+ scope).
    Stage,
    /// Operator preview / next-item monitor - maps to
    /// `DisplayScreen::Confidence`.
    Confidence,
    /// Independently routed overflow-room content - maps to
    /// `DisplayScreen::Lobby`.
    Lobby,
}

impl DisplayRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            DisplayRole::Unassigned => "unassigned",
            DisplayRole::Operator => "operator",
            DisplayRole::Projector => "projector",
            DisplayRole::Stage => "stage",
            DisplayRole::Confidence => "confidence",
            DisplayRole::Lobby => "lobby",
        }
    }

    pub fn parse(role: &str) -> Option<DisplayRole> {
        match role {
            "unassigned" => Some(DisplayRole::Unassigned),
            "operator" => Some(DisplayRole::Operator),
            "projector" => Some(DisplayRole::Projector),
            "stage" => Some(DisplayRole::Stage),
            "confidence" => Some(DisplayRole::Confidence),
            "lobby" => Some(DisplayRole::Lobby),
            _ => None,
        }
    }
}

/// Which [`DisplayRole`] a given [`crate::presentation_display::DisplayScreen`]
/// should be positioned on, when one is assigned - the bridge between the
/// pre-existing 3-window system (Phase 3.10) and this phase's 6-role
/// physical-monitor registry. `DisplayRole::Stage` has no corresponding
/// screen (see this module's top-level docs) and so is unreachable from
/// this function - a future phase that adds a speaker-facing content
/// stream would extend `DisplayScreen` and this mapping together.
pub fn screen_role(screen: crate::presentation_display::DisplayScreen) -> DisplayRole {
    use crate::presentation_display::DisplayScreen;
    match screen {
        DisplayScreen::Stage => DisplayRole::Projector,
        DisplayScreen::Confidence => DisplayRole::Confidence,
        DisplayScreen::Lobby => DisplayRole::Lobby,
    }
}

/// CIP's own best-effort stable identifier for a monitor - Tauri's
/// monitor API exposes no real OS-issued id (`tauri::window::Monitor` has
/// only `name`/`size`/`position`/`work_area`/`scale_factor`). Prefers the
/// OS-reported name (typically stable across reboots, e.g.
/// `"\\.\DISPLAY2"` on Windows); falls back to a position+resolution
/// fingerprint when no name is reported, which is stable only until the
/// operator rearranges Display Settings - an honest, documented
/// limitation (see `0012_display_role_assignments.sql`'s own comment),
/// not a defect.
pub fn compute_monitor_id(name: Option<&str>, x: i32, y: i32, width: u32, height: u32) -> String {
    match name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => format!("unnamed@{x},{y}-{width}x{height}"),
    }
}

/// One physical monitor as Tauri's monitor API reports it right now -
/// the Tauri-facing half of a [`Display`], before any role assignment is
/// merged in.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalMonitor {
    pub monitor_id: String,
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

/// Enumerates every monitor this process can currently detect, via
/// `AppHandle::available_monitors`/`primary_monitor` - the exact same
/// Tauri API `get_pilot_diagnostics` has used since Phase 3.2-3.4, now
/// also feeding the Display Registry. Requires a live `AppHandle`, so
/// (matching this project's established "no `tauri::test` harness"
/// convention - see `presentation_display.rs`'s own module docs) this
/// function itself is exercised by real desktop runtime validation, not
/// a unit test; [`compute_monitor_id`] and [`merge_displays`], the pure
/// logic around it, are unit tested below.
pub fn enumerate_monitors(app: &AppHandle) -> Vec<PhysicalMonitor> {
    let primary_position = app.primary_monitor().ok().flatten().map(|m| *m.position());
    app.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let x = m.position().x;
            let y = m.position().y;
            let width = m.size().width;
            let height = m.size().height;
            PhysicalMonitor {
                monitor_id: compute_monitor_id(m.name().map(String::as_str), x, y, width, height),
                name: m.name().cloned(),
                x,
                y,
                width,
                height,
                scale_factor: m.scale_factor(),
                is_primary: primary_position == Some(*m.position()),
            }
        })
        .collect()
}

/// A full Display Registry entry - a physical monitor merged with its
/// (possibly absent) persisted role assignment. Mirrors the operator's
/// own requested shape: `monitor_id`/`name`/`position`/`width`/`height`/
/// `scale_factor`/`is_primary`/`assigned_role`/`connected`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub monitor_id: String,
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
    pub assigned_role: DisplayRole,
    /// `false` for a monitor with a persisted role assignment that is
    /// not among the currently-enumerated physical monitors (e.g.
    /// unplugged since the assignment was made) - its geometry fields
    /// are zeroed/placeholder in that case, since this module never
    /// stores last-known geometry, only the assignment itself.
    pub connected: bool,
}

/// Merges live physical-monitor enumeration with persisted role
/// assignments into the full Display Registry list - pure, no Tauri/SQL
/// dependency, directly unit-testable. Every connected monitor appears
/// exactly once (`Unassigned` if it has no persisted assignment); every
/// persisted assignment whose monitor is not currently connected also
/// appears, marked `connected: false`, so an operator's prior setup is
/// never silently dropped just because a monitor is temporarily
/// unplugged (Phase 3.10.4's disconnect/reconnect scope builds on this).
pub fn merge_displays(
    physical: Vec<PhysicalMonitor>,
    assignments: &HashMap<String, DisplayRole>,
) -> Vec<Display> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut displays: Vec<Display> = physical
        .into_iter()
        .map(|m| {
            seen.insert(m.monitor_id.clone());
            Display {
                assigned_role: assignments
                    .get(&m.monitor_id)
                    .copied()
                    .unwrap_or(DisplayRole::Unassigned),
                monitor_id: m.monitor_id,
                name: m.name,
                x: m.x,
                y: m.y,
                width: m.width,
                height: m.height,
                scale_factor: m.scale_factor,
                is_primary: m.is_primary,
                connected: true,
            }
        })
        .collect();

    for (monitor_id, role) in assignments {
        if seen.contains(monitor_id) {
            continue;
        }
        displays.push(Display {
            monitor_id: monitor_id.clone(),
            name: None,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            scale_factor: 1.0,
            is_primary: false,
            assigned_role: *role,
            connected: false,
        });
    }

    displays
}

/// Where to position a display window, in the same physical-pixel units
/// `tauri::window::Monitor` itself reports - converted to the logical
/// pixels `WebviewWindowBuilder::position`/`inner_size` actually expect
/// only at the point of use (`presentation_display::open_display_window`),
/// since that conversion needs `scale_factor` alongside the values
/// themselves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

/// Finds the connected display assigned `role`, if any - `None` when no
/// display has that role assigned, or when the display that does is not
/// currently connected (its geometry, per [`merge_displays`]'s docs, is
/// not real). The caller (`commands.rs`) treats `None` as "use the
/// existing, unpositioned default" - this phase never regresses the
/// pre-3.10.2 behavior when nothing is configured.
pub fn resolve_role_position(displays: &[Display], role: DisplayRole) -> Option<MonitorPlacement> {
    displays
        .iter()
        .find(|d| d.assigned_role == role && d.connected)
        .map(|d| MonitorPlacement {
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            scale_factor: d.scale_factor,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation_display::DisplayScreen;

    fn monitor(id: &str, x: i32, y: i32, w: u32, h: u32, primary: bool) -> PhysicalMonitor {
        PhysicalMonitor {
            monitor_id: id.to_string(),
            name: Some(id.to_string()),
            x,
            y,
            width: w,
            height: h,
            scale_factor: 1.0,
            is_primary: primary,
        }
    }

    #[test]
    fn every_role_id_round_trips_through_parse() {
        let all = [
            DisplayRole::Unassigned,
            DisplayRole::Operator,
            DisplayRole::Projector,
            DisplayRole::Stage,
            DisplayRole::Confidence,
            DisplayRole::Lobby,
        ];
        for role in all {
            assert_eq!(DisplayRole::parse(role.as_str()), Some(role));
        }
    }

    #[test]
    fn parse_rejects_an_unknown_role() {
        assert_eq!(DisplayRole::parse("kitchen"), None);
        assert_eq!(DisplayRole::parse(""), None);
    }

    #[test]
    fn screen_role_maps_stage_to_projector_confidence_and_lobby_unchanged() {
        assert_eq!(screen_role(DisplayScreen::Stage), DisplayRole::Projector);
        assert_eq!(
            screen_role(DisplayScreen::Confidence),
            DisplayRole::Confidence
        );
        assert_eq!(screen_role(DisplayScreen::Lobby), DisplayRole::Lobby);
    }

    #[test]
    fn compute_monitor_id_prefers_the_os_reported_name() {
        assert_eq!(
            compute_monitor_id(Some("\\\\.\\DISPLAY2"), 1920, 0, 1920, 1080),
            "\\\\.\\DISPLAY2"
        );
    }

    #[test]
    fn compute_monitor_id_falls_back_to_a_position_resolution_fingerprint_when_unnamed() {
        assert_eq!(
            compute_monitor_id(None, 1920, 0, 1920, 1080),
            "unnamed@1920,0-1920x1080"
        );
        assert_eq!(
            compute_monitor_id(Some(""), 0, 0, 800, 600),
            "unnamed@0,0-800x600"
        );
    }

    #[test]
    fn merge_displays_assigns_unassigned_to_a_monitor_with_no_persisted_role() {
        let physical = vec![monitor("laptop", 0, 0, 1920, 1080, true)];
        let displays = merge_displays(physical, &HashMap::new());
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].assigned_role, DisplayRole::Unassigned);
        assert!(displays[0].connected);
    }

    #[test]
    fn merge_displays_attaches_a_persisted_role_to_its_matching_connected_monitor() {
        let physical = vec![
            monitor("laptop", 0, 0, 1920, 1080, true),
            monitor("tv", 1920, 0, 1920, 1080, false),
        ];
        let mut assignments = HashMap::new();
        assignments.insert("tv".to_string(), DisplayRole::Projector);

        let displays = merge_displays(physical, &assignments);
        let laptop = displays.iter().find(|d| d.monitor_id == "laptop").unwrap();
        let tv = displays.iter().find(|d| d.monitor_id == "tv").unwrap();
        assert_eq!(laptop.assigned_role, DisplayRole::Unassigned);
        assert_eq!(tv.assigned_role, DisplayRole::Projector);
        assert!(tv.connected);
    }

    #[test]
    fn merge_displays_keeps_a_persisted_assignment_for_a_now_disconnected_monitor() {
        let physical = vec![monitor("laptop", 0, 0, 1920, 1080, true)];
        let mut assignments = HashMap::new();
        assignments.insert("tv".to_string(), DisplayRole::Projector);

        let displays = merge_displays(physical, &assignments);
        assert_eq!(
            displays.len(),
            2,
            "the unplugged TV's assignment must not be dropped"
        );
        let tv = displays.iter().find(|d| d.monitor_id == "tv").unwrap();
        assert_eq!(tv.assigned_role, DisplayRole::Projector);
        assert!(!tv.connected);
        assert_eq!(
            tv.width, 0,
            "geometry is honestly placeholder, never fabricated"
        );
    }

    #[test]
    fn resolve_role_position_finds_the_assigned_connected_monitor() {
        let displays = vec![
            Display {
                monitor_id: "laptop".into(),
                name: Some("laptop".into()),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale_factor: 1.0,
                is_primary: true,
                assigned_role: DisplayRole::Operator,
                connected: true,
            },
            Display {
                monitor_id: "tv".into(),
                name: Some("tv".into()),
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
                scale_factor: 1.0,
                is_primary: false,
                assigned_role: DisplayRole::Projector,
                connected: true,
            },
        ];
        let placement = resolve_role_position(&displays, DisplayRole::Projector).unwrap();
        assert_eq!(placement.x, 1920);
        assert_eq!(placement.width, 1920);
    }

    #[test]
    fn resolve_role_position_returns_none_when_the_role_is_unassigned() {
        let displays = vec![Display {
            monitor_id: "laptop".into(),
            name: Some("laptop".into()),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
            is_primary: true,
            assigned_role: DisplayRole::Unassigned,
            connected: true,
        }];
        assert_eq!(
            resolve_role_position(&displays, DisplayRole::Projector),
            None
        );
    }

    #[test]
    fn resolve_role_position_returns_none_when_the_assigned_monitor_is_disconnected() {
        let displays = vec![Display {
            monitor_id: "tv".into(),
            name: Some("tv".into()),
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            scale_factor: 1.0,
            is_primary: false,
            assigned_role: DisplayRole::Projector,
            connected: false,
        }];
        assert_eq!(
            resolve_role_position(&displays, DisplayRole::Projector),
            None,
            "a disconnected monitor's placeholder geometry must never be used to position a real window"
        );
    }
}
