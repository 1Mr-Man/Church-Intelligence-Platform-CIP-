//! Phase 3.10.3: per-screen Live/Held routing.
//!
//! Every open [`crate::presentation_display::DisplayScreen`] mirrors the
//! same single `Active` presentation item (Phase 3.10/3.10.1's own
//! finding: "multi-screen means the one active item reaching more
//! physical screens, not more concurrent active items"). This module adds
//! exactly one thing on top of that, unchanged, invariant: the operator
//! can independently take a screen out of the live broadcast (`Held`)
//! without touching `presentation.rs`'s domain lifecycle
//! (Prepared/Active/Stopped) or `presentation_display.rs`'s window-
//! creation pipeline at all.
//!
//! `Held` freezes a screen on whatever it currently shows - it does not
//! blank it, and it does not give it different content from `Live`
//! screens. There is still only ever one `Active` item; `Held` only
//! controls whether a given open screen keeps receiving that item's
//! future changes. Switching a screen back to `Live` catches it up via a
//! single targeted re-sync event (see `commands::set_screen_route_mode`),
//! reusing the exact same hydration path a freshly opened screen already
//! uses on mount - no second content-delivery mechanism.
//!
//! See `docs/phase-3-10-3-presentation-router.md`.

use std::collections::HashMap;

use crate::presentation_display::DisplayScreen;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteMode {
    /// Receives every live `PRESENTATION_STARTED`/`PRESENTATION_STOPPED`
    /// broadcast for the one active item - the default, and the exact
    /// pre-3.10.3 behavior for every screen.
    Live,
    /// Frozen on its last-received content; the live broadcast is not
    /// delivered to it until switched back to `Live`.
    Held,
}

impl RouteMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteMode::Live => "live",
            RouteMode::Held => "held",
        }
    }

    pub fn parse(s: &str) -> Option<RouteMode> {
        match s {
            "live" => Some(RouteMode::Live),
            "held" => Some(RouteMode::Held),
            _ => None,
        }
    }
}

/// Which of `open_screens` should receive a live presentation broadcast
/// right now - pure, no Tauri dependency, independently testable. A
/// screen missing from `modes` is `Live` (the default for a screen that
/// has never had its route mode explicitly changed this session).
pub fn screens_to_broadcast(
    open_screens: &[DisplayScreen],
    modes: &HashMap<DisplayScreen, RouteMode>,
) -> Vec<DisplayScreen> {
    open_screens
        .iter()
        .copied()
        .filter(|screen| modes.get(screen).copied().unwrap_or(RouteMode::Live) == RouteMode::Live)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_mode_round_trips_through_parse() {
        for mode in [RouteMode::Live, RouteMode::Held] {
            assert_eq!(RouteMode::parse(mode.as_str()), Some(mode));
        }
    }

    #[test]
    fn parse_rejects_an_unknown_mode() {
        assert_eq!(RouteMode::parse("frozen"), None);
        assert_eq!(RouteMode::parse(""), None);
    }

    #[test]
    fn a_screen_with_no_explicit_mode_defaults_to_live() {
        let open = [DisplayScreen::Stage, DisplayScreen::Confidence];
        let modes = HashMap::new();
        let broadcast = screens_to_broadcast(&open, &modes);
        assert_eq!(
            broadcast,
            vec![DisplayScreen::Stage, DisplayScreen::Confidence]
        );
    }

    #[test]
    fn a_held_screen_is_excluded_from_the_broadcast() {
        let open = [
            DisplayScreen::Stage,
            DisplayScreen::Confidence,
            DisplayScreen::Lobby,
        ];
        let mut modes = HashMap::new();
        modes.insert(DisplayScreen::Confidence, RouteMode::Held);
        let broadcast = screens_to_broadcast(&open, &modes);
        assert_eq!(broadcast, vec![DisplayScreen::Stage, DisplayScreen::Lobby]);
    }

    #[test]
    fn an_explicitly_live_screen_is_included_same_as_the_default() {
        let open = [DisplayScreen::Stage];
        let mut modes = HashMap::new();
        modes.insert(DisplayScreen::Stage, RouteMode::Live);
        assert_eq!(
            screens_to_broadcast(&open, &modes),
            vec![DisplayScreen::Stage]
        );
    }

    #[test]
    fn a_held_screen_that_is_not_open_has_no_effect_either_way() {
        // modes only ever matters for screens actually in open_screens -
        // a Held mode set on a currently-closed screen is simply inert
        // until that screen is reopened.
        let open: [DisplayScreen; 0] = [];
        let mut modes = HashMap::new();
        modes.insert(DisplayScreen::Lobby, RouteMode::Held);
        assert_eq!(
            screens_to_broadcast(&open, &modes),
            Vec::<DisplayScreen>::new()
        );
    }

    #[test]
    fn every_screen_held_leaves_nothing_to_broadcast() {
        let open = [
            DisplayScreen::Stage,
            DisplayScreen::Confidence,
            DisplayScreen::Lobby,
        ];
        let mut modes = HashMap::new();
        for screen in open {
            modes.insert(screen, RouteMode::Held);
        }
        assert_eq!(
            screens_to_broadcast(&open, &modes),
            Vec::<DisplayScreen>::new()
        );
    }
}
