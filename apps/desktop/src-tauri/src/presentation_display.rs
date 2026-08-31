//! The local presentation display window - a second, dedicated Tauri
//! `WebviewWindow` that shows a [`cip_presentation_renderer::RenderedSlide`]
//! to a projector/TV/secondary monitor under direct operator control. This
//! is the smallest real display output CIP has: no OBS, no vMix, no NDI -
//! see `docs/presentation.md`'s "Local display architecture" section for
//! why, and what remains explicitly out of scope.
//!
//! Deliberately thin: every decision about *whether* an item may be
//! displayed, and the `Prepared -> Active -> Stopped` persistence
//! transitions themselves, lives in `presentation.rs` (Tauri-agnostic,
//! independently unit-tested). This module only ever does window
//! lifecycle (open/close/detect) and is exercised by real desktop runtime
//! validation rather than unit tests, matching this project's established
//! "no `tauri::test` harness" convention (see `presentation.rs`'s own
//! module docs).
//!
//! ## Security (spec section 16)
//!
//! The display window is a passive renderer, nothing more. It loads the
//! exact same frontend bundle as the main window (`index.html`) and
//! branches to a distinct, minimal React component purely by reading its
//! own window label (`main.tsx`) - no second Vite entry point, no second
//! build. Its Tauri capability grant (`capabilities/display.json`) is
//! `core:default` only, identical to the main window's own grant, and this
//! app has no `fs`/`shell`/`http`/`dialog` plugin installed at all (see
//! `Cargo.toml`) - so the display window has exactly the same (already
//! minimal) capability surface as `main`, never more. It never receives a
//! database connection, a secret, or an operator-privileged command; it
//! only ever listens for `PRESENTATION_STARTED`/`PRESENTATION_STOPPED`
//! events, the same public event bus every other window already uses.

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Phase 3.10: the three display roles CIP can drive simultaneously.
///
/// - `Stage`: the primary congregation-facing output - this is the *only*
///   display CIP supported before this phase. Its window label (`"display"`)
///   is deliberately unchanged, so every pre-3.10 behavior for this screen
///   (including `display_presentation`'s auto-open) is identical.
/// - `Confidence`: an operator/platform-facing monitor. Mirrors the same
///   active item; the frontend renders it with additional operator-only
///   metadata already present in the existing broadcast payload (template,
///   auto-detected vs. manual) - never new or fabricated data.
/// - `Lobby`: an overflow-room screen. Mirrors Stage exactly.
///
/// All three ever show, at most, the one `Active` `PresentationItem` a
/// service can have (spec section 10, unchanged by this phase) - "multi-
/// screen" means the same output reaching more places, not more outputs.
/// See `docs/phase-3-10-multi-screen-audit.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayScreen {
    Stage,
    Confidence,
    Lobby,
}

impl DisplayScreen {
    pub const ALL: [DisplayScreen; 3] = [
        DisplayScreen::Stage,
        DisplayScreen::Confidence,
        DisplayScreen::Lobby,
    ];

    /// The Tauri window label - distinct from `"main"` (the operator's own
    /// window, declared in `tauri.conf.json`). Never statically declared in
    /// `tauri.conf.json`: a screen's window is created only when an
    /// operator explicitly opens or displays something (spec section 18 -
    /// never opened automatically, including at startup).
    pub fn window_label(&self) -> &'static str {
        match self {
            DisplayScreen::Stage => "display",
            DisplayScreen::Confidence => "display-confidence",
            DisplayScreen::Lobby => "display-lobby",
        }
    }

    /// The native OS window title bar text.
    pub fn window_title(&self) -> &'static str {
        match self {
            DisplayScreen::Stage => "CIP Presentation Display",
            DisplayScreen::Confidence => "CIP Confidence Monitor",
            DisplayScreen::Lobby => "CIP Overflow Display",
        }
    }

    /// The operator-facing label shown in the Presentation card's screen
    /// controls - distinct from `window_title()` (the OS title bar).
    pub fn operator_label(&self) -> &'static str {
        match self {
            DisplayScreen::Stage => "Stage",
            DisplayScreen::Confidence => "Confidence Monitor",
            DisplayScreen::Lobby => "Lobby / Overflow",
        }
    }

    /// The stable snake_case identifier used on the wire (Tauri command
    /// argument, `get_presentation_display_state` response) - mirrored by
    /// the frontend's `PresentationScreen` type.
    pub fn id(&self) -> &'static str {
        match self {
            DisplayScreen::Stage => "stage",
            DisplayScreen::Confidence => "confidence",
            DisplayScreen::Lobby => "lobby",
        }
    }

    /// Parses the wire identifier back into a `DisplayScreen` - `None` for
    /// anything else, so an unrecognized screen id is rejected explicitly
    /// by the caller rather than silently defaulting to one.
    pub fn parse(id: &str) -> Option<DisplayScreen> {
        match id {
            "stage" => Some(DisplayScreen::Stage),
            "confidence" => Some(DisplayScreen::Confidence),
            "lobby" => Some(DisplayScreen::Lobby),
            _ => None,
        }
    }
}

/// Whether `screen`'s display window currently exists (open or merely not
/// yet closed) - `false` after that screen's `close_display_window` or a
/// manual close, and always `false` immediately after app startup (nothing
/// creates any screen eagerly).
pub fn is_display_window_open(app: &AppHandle, screen: DisplayScreen) -> bool {
    app.get_webview_window(screen.window_label()).is_some()
}

/// Whether *any* of the three screens currently has an open window -
/// Phase 3.10's generalization of the pre-3.10 single-window "is the
/// display open" check, used to decide whether closing/destroying one
/// screen's window should reconcile the active item to `Stopped` (only
/// when it was the *last* screen still showing it).
pub fn any_display_window_open(app: &AppHandle) -> bool {
    DisplayScreen::ALL
        .into_iter()
        .any(|screen| is_display_window_open(app, screen))
}

/// Opens the display window if it doesn't already exist, or brings an
/// existing one to the front - never creates a second window for the same
/// label (spec section 17: "duplicate open request" must be safe).
///
/// Registers a `Destroyed` handler so a manually-closed window (spec
/// section 9/17: "closing the display window does not crash CIP") is
/// reconciled the same way an explicit Stop action would be - via
/// [`crate::commands::clear_active_presentation`], so persistence never
/// disagrees with what the operator can actually see.
///
/// # Phase 3.10.4: reopening an already-open window can reposition it
///
/// Before this phase, calling this function on an already-open screen
/// only showed/focused it - a monitor reassigned in the Display Registry,
/// or a previously-disconnected monitor reconnecting, never moved a
/// window that was already open on the wrong place. This phase makes
/// that case useful: when `placement` is `Some` (a connected, assigned
/// monitor is currently resolved for this screen), an already-open
/// window is also moved/resized to it via `set_position`/`set_size` -
/// letting the operator's existing "Open"/"Reposition" action in the
/// Presentation card genuinely reconnect a screen after its monitor came
/// back, without closing and reopening the window. When `placement` is
/// `None` (nothing assigned, or the assigned monitor is not currently
/// connected), an already-open window is left exactly where it is - this
/// deliberately never undoes an operator's own manual drag on a machine
/// with no Display Registry assignment, matching this phase's own
/// "never touch what the operator already positioned by hand" boundary.
///
/// # Phase 3.8.4: callers MUST be `async fn` Tauri commands on Windows
///
/// This function's `WebviewWindowBuilder::build()` call is a documented
/// Tauri/WRY known issue on Windows
/// (<https://github.com/tauri-apps/wry/issues/583>, referenced directly
/// from the vendored `tauri` crate's own doc comments on
/// `WebviewWindowBuilder::new`/`build`): calling it from a synchronous
/// `#[tauri::command] fn` (as opposed to `async fn`) deadlocks the
/// WebView2 control's initialization on Windows specifically - the native
/// window frame can still appear (created by the OS), but the webview
/// inside it never finishes navigating, leaving WebView2's own default
/// *white* background rather than this app's CSS (which never gets a
/// chance to run). This function itself stays synchronous (it is not
/// `.await`ed anywhere); the requirement is on its *callers*
/// (`commands::display_presentation`, `commands::open_presentation_display`),
/// which are `async fn` for exactly this reason - see
/// `docs/phase-3-8-4-audit.md` section D for the real Windows evidence and
/// the exact vendored-source citation this is based on. Phase 3.10.2 does
/// not change this ordering: `placement` only changes *what* is passed to
/// `.position`/`.inner_size` before `.build()`, never *when* `.build()`
/// itself is called relative to the caller's `async fn` boundary.
///
/// `placement`, when `Some` (Phase 3.10.2 - the Display Registry has an
/// assigned, connected monitor for this screen's role), positions the new
/// window at that monitor's origin and sizes it to that monitor's full
/// resolution - opening "directly on" the assigned monitor, filling it,
/// per the operator's own requested behavior. `WebviewWindowBuilder::position`/
/// `inner_size` take **logical** pixels while `MonitorPlacement` (built
/// from `tauri::window::Monitor`) is in **physical** pixels - converted
/// here via `PhysicalPosition`/`PhysicalSize::to_logical`, never by a
/// hand-rolled division, to avoid a DPI-scaling placement bug on a
/// non-100%-scaled monitor (common on Windows laptops). `None` (nothing
/// assigned, or the assigned monitor is currently disconnected) preserves
/// the exact pre-3.10.2 behavior: an unpositioned 1280x720 window the
/// operator places themselves.
pub fn open_display_window(
    app: &AppHandle,
    screen: DisplayScreen,
    placement: Option<crate::display_registry::MonitorPlacement>,
) -> tauri::Result<()> {
    let logical_position =
        placement.map(|p| tauri::PhysicalPosition::new(p.x, p.y).to_logical::<f64>(p.scale_factor));
    let logical_size = placement
        .map(|p| tauri::PhysicalSize::new(p.width, p.height).to_logical::<f64>(p.scale_factor))
        .unwrap_or(tauri::LogicalSize::new(1280.0, 720.0));

    if let Some(existing) = app.get_webview_window(screen.window_label()) {
        existing.show()?;
        // Phase 3.10.4: only reposition when a real, connected, assigned
        // monitor is resolved - never move a window an operator placed
        // manually on a machine with no Display Registry assignment (see
        // this function's own docs above).
        if let Some(pos) = logical_position {
            existing.set_position(pos)?;
            existing.set_size(logical_size)?;
        }
        existing.set_focus()?;
        return Ok(());
    }

    let mut builder = WebviewWindowBuilder::new(
        app,
        screen.window_label(),
        WebviewUrl::App("index.html".into()),
    )
    .title(screen.window_title())
    .inner_size(logical_size.width, logical_size.height)
    .resizable(true)
    .visible(true)
    // Phase 3.8.4 TEMPORARY DIAGNOSTIC: the only way to observe, from
    // Rust, whether the display webview's document navigation itself
    // ever starts/finishes - boundaries C/D/E of the operator's audit.
    // Without this, "the window appears but stays blank" was
    // indistinguishable between "navigation never happened" and
    // "navigation finished but the frontend never ran."
    .on_page_load(|_webview, payload| {
        let event_name = match payload.event() {
            PageLoadEvent::Started => "page-load-started",
            PageLoadEvent::Finished => "page-load-finished",
        };
        log::info!(
            target: crate::logging::LogCategory::Presentation.target(),
            "[diagnostic] display window: {event_name} url={}",
            payload.url()
        );
    });
    if let Some(pos) = logical_position {
        builder = builder.position(pos.x, pos.y);
    }
    let window = builder.build()?;

    log::info!(
        target: crate::logging::LogCategory::Presentation.target(),
        "[diagnostic] display window created (checkpoint 1) placed={}",
        logical_position.is_some()
    );

    // Phase 3.8.3: a real Windows-only defect class (a newly created
    // secondary WebView2-backed window sometimes does not paint its
    // initial frame until it receives a resize/redraw signal - the
    // window exists and responds, but shows nothing) was the
    // best-supported remaining explanation after real end-to-end
    // reproduction on Linux/WebKitGTK under Xvfb found the entire
    // pipeline working correctly, including under adversarial
    // near-zero-delay timing (see docs/phase-3-8-3-audit.md section D-F).
    // Forcing an explicit resize to the same target size immediately
    // after creation triggers WebView2's paint without touching any
    // other platform's already-proven-correct behavior. Phase 3.10.2:
    // nudges to `logical_size` (whatever was actually requested above),
    // not a separately hardcoded constant - nudging to a stale 1280x720
    // here would silently override a real monitor-fill placement on
    // Windows specifically, the one platform this workaround targets.
    #[cfg(target_os = "windows")]
    {
        if let Err(e) = window.set_size(logical_size) {
            log::warn!(
                target: crate::logging::LogCategory::Presentation.target(),
                "failed to nudge the display window's initial paint via resize: {e}"
            );
        }
    }

    let app_handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            // Best-effort: a display screen disappearing (manual close,
            // Alt+F4, OS window-manager action) must never leave an
            // `Active` row persisted with nothing actually showing it -
            // reconcile exactly as an explicit Stop would. Phase 3.10: only
            // when *no* screen remains open - closing one of several open
            // screens must not blank the others that are still genuinely
            // showing the active item. Errors are logged, never propagated
            // (there is no command call site here to return them to).
            if !any_display_window_open(&app_handle) {
                if let Err(e) = crate::commands::clear_active_presentation(&app_handle) {
                    log::warn!(
                        target: crate::logging::LogCategory::Presentation.target(),
                        "failed to reconcile presentation state after the last display screen closed: {e}"
                    );
                }
            }
        }
    });

    Ok(())
}

/// Closes `screen`'s display window if it exists - a safe no-op if it's
/// already closed (spec section 9/17). Triggers the same `Destroyed`
/// reconciliation [`open_display_window`] registers, so an explicit Close
/// always leaves the same persisted state a manual close would.
pub fn close_display_window(app: &AppHandle, screen: DisplayScreen) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(screen.window_label()) {
        window.close()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_screen_id_round_trips_through_parse() {
        for screen in DisplayScreen::ALL {
            assert_eq!(DisplayScreen::parse(screen.id()), Some(screen));
        }
    }

    #[test]
    fn every_screen_has_a_distinct_window_label() {
        let labels: Vec<_> = DisplayScreen::ALL
            .iter()
            .map(|s| s.window_label())
            .collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(labels.len(), unique.len(), "window labels must be distinct");
    }

    #[test]
    fn stage_keeps_the_pre_3_10_window_label_unchanged() {
        // Backward compatibility: the Stage screen is the only display CIP
        // supported before Phase 3.10 - its window label must stay exactly
        // "display" so nothing about its pre-3.10 behavior changes.
        assert_eq!(DisplayScreen::Stage.window_label(), "display");
    }

    #[test]
    fn parse_rejects_an_unknown_screen_id() {
        assert_eq!(DisplayScreen::parse("projector"), None);
        assert_eq!(DisplayScreen::parse(""), None);
    }
}
