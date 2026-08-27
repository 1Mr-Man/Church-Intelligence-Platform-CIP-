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

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The display window's Tauri label - distinct from `"main"` (the
/// operator's own window, declared in `tauri.conf.json`). Never
/// statically declared in `tauri.conf.json`: the window is created only
/// when an operator explicitly opens or displays something (spec section
/// 18 - never opened automatically, including at startup).
pub const DISPLAY_WINDOW_LABEL: &str = "display";

/// Whether the display window currently exists (open or merely not yet
/// closed) - `false` after `close_display_window` or a manual close, and
/// always `false` immediately after app startup (nothing creates it
/// eagerly).
pub fn is_display_window_open(app: &AppHandle) -> bool {
    app.get_webview_window(DISPLAY_WINDOW_LABEL).is_some()
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
pub fn open_display_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(existing) = app.get_webview_window(DISPLAY_WINDOW_LABEL) {
        existing.show()?;
        existing.set_focus()?;
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        DISPLAY_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("CIP Presentation Display")
    .inner_size(1280.0, 720.0)
    .resizable(true)
    .visible(true)
    .build()?;

    let app_handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            // Best-effort: the display window disappearing (manual close,
            // Alt+F4, OS window-manager action) must never leave an
            // `Active` row persisted with nothing actually showing it -
            // reconcile exactly as an explicit Stop would. Errors are
            // logged, never propagated (there is no command call site
            // here to return them to).
            if let Err(e) = crate::commands::clear_active_presentation(&app_handle) {
                log::warn!(
                    target: crate::logging::LogCategory::Presentation.target(),
                    "failed to reconcile presentation state after the display window closed: {e}"
                );
            }
        }
    });

    Ok(())
}

/// Closes the display window if it exists - a safe no-op if it's already
/// closed (spec section 9/17). Triggers the same `Destroyed` reconciliation
/// [`open_display_window`] registers, so an explicit Close always leaves
/// the same persisted state a manual close would.
pub fn close_display_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(DISPLAY_WINDOW_LABEL) {
        window.close()?;
    }
    Ok(())
}
