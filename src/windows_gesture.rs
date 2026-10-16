//! The pure hold, double-tap lock, and Escape-cancel gesture behind the Windows
//! dictation shortcut. The low-level keyboard hook feeds it physical trigger and
//! Escape transitions; it decides which events to emit and which keys to
//! swallow. It mirrors the X11 gesture, except Windows auto-repeat never sends a
//! synthetic release, so no release grace period is needed.

use std::time::{Duration, Instant};

pub(crate) const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyEvent {
    Start,
    Finish,
    Cancel,
}

/// What the hook does with one physical key transition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Decision {
    /// Keep the key from reaching the foreground application.
    pub suppress: bool,
    pub event: Option<HotkeyEvent>,
}

impl Decision {
    const PASS: Self = Self {
        suppress: false,
        event: None,
    };

    fn swallow(event: Option<HotkeyEvent>) -> Self {
        Self {
            suppress: true,
            event,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Gesture {
    double_tap_enabled: bool,
    /// The trigger is physically down and its press belongs to HEX.
    trigger_owned: bool,
    /// The trigger is physically down but its press belongs to another app.
    trigger_foreign: bool,
    escape_owned: bool,
    active: bool,
    second_tap: bool,
    locked: bool,
    last_release: Option<Instant>,
}

impl Gesture {
    pub(crate) fn new(double_tap_enabled: bool) -> Self {
        Self {
            double_tap_enabled,
            trigger_owned: false,
            trigger_foreign: false,
            escape_owned: false,
            active: false,
            second_tap: false,
            locked: false,
            last_release: None,
        }
    }

    /// A trigger key-down. `modifiers_match` is whether exactly the configured
    /// modifiers are held. Auto-repeat arrives as further key-downs.
    pub(crate) fn trigger_down(&mut self, now: Instant, modifiers_match: bool) -> Decision {
        if self.trigger_owned {
            return Decision::swallow(None);
        }
        if self.trigger_foreign {
            return Decision::PASS;
        }
        if self.locked {
            self.locked = false;
            self.trigger_owned = true;
            return Decision::swallow(Some(HotkeyEvent::Finish));
        }
        if !modifiers_match {
            self.trigger_foreign = true;
            return Decision::PASS;
        }
        self.trigger_owned = true;
        self.active = true;
        self.second_tap = self
            .last_release
            .take()
            .is_some_and(|released| now.saturating_duration_since(released) < DOUBLE_TAP_WINDOW);
        Decision::swallow(Some(HotkeyEvent::Start))
    }

    pub(crate) fn trigger_up(&mut self, now: Instant) -> Decision {
        if self.trigger_foreign {
            self.trigger_foreign = false;
            return Decision::PASS;
        }
        if !self.trigger_owned {
            return Decision::PASS;
        }
        self.trigger_owned = false;
        if !self.active {
            // The press that finished a locked capture, or one Escape cancelled.
            return Decision::swallow(None);
        }
        self.active = false;
        if self.second_tap {
            self.second_tap = false;
            self.locked = true;
            self.last_release = None;
            return Decision::swallow(None);
        }
        self.last_release = self.double_tap_enabled.then_some(now);
        Decision::swallow(Some(HotkeyEvent::Finish))
    }

    pub(crate) fn escape_down(&mut self) -> Decision {
        if self.escape_owned {
            return Decision::swallow(None);
        }
        if !self.active && !self.locked {
            return Decision::PASS;
        }
        self.escape_owned = true;
        self.active = false;
        self.locked = false;
        self.second_tap = false;
        self.last_release = None;
        Decision::swallow(Some(HotkeyEvent::Cancel))
    }

    pub(crate) fn escape_up(&mut self) -> Decision {
        if std::mem::take(&mut self.escape_owned) {
            Decision::swallow(None)
        } else {
            Decision::PASS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(decisions: &[Decision]) -> Vec<HotkeyEvent> {
        decisions
            .iter()
            .filter_map(|decision| decision.event)
            .collect()
    }

    #[test]
    fn hold_and_release_starts_and_finishes_while_swallowing_repeats() {
        let mut gesture = Gesture::new(true);
        let now = Instant::now();
        let decisions = [
            gesture.trigger_down(now, true),
            gesture.trigger_down(now + Duration::from_millis(500), true),
            gesture.trigger_down(now + Duration::from_millis(530), true),
            gesture.trigger_up(now + Duration::from_secs(2)),
        ];
        assert!(decisions.iter().all(|decision| decision.suppress));
        assert_eq!(
            events(&decisions),
            [HotkeyEvent::Start, HotkeyEvent::Finish]
        );
    }

    #[test]
    fn other_modifiers_pass_through_until_released() {
        let mut gesture = Gesture::new(true);
        let now = Instant::now();
        assert_eq!(gesture.trigger_down(now, false), Decision::PASS);
        // Repeats stay foreign even if the modifiers now match.
        assert_eq!(gesture.trigger_down(now, true), Decision::PASS);
        assert_eq!(gesture.trigger_up(now), Decision::PASS);
        assert_eq!(
            gesture.trigger_down(now, true).event,
            Some(HotkeyEvent::Start)
        );
    }

    #[test]
    fn second_tap_within_window_locks_until_the_next_press() {
        let mut gesture = Gesture::new(true);
        let now = Instant::now();
        let ms = |value| now + Duration::from_millis(value);
        let decisions = [
            gesture.trigger_down(ms(0), true),
            gesture.trigger_up(ms(100)),
            gesture.trigger_down(ms(250), true),
            gesture.trigger_up(ms(350)),
            gesture.trigger_down(ms(5_000), true),
            gesture.trigger_up(ms(5_100)),
        ];
        assert!(decisions.iter().all(|decision| decision.suppress));
        assert_eq!(
            events(&decisions),
            [
                HotkeyEvent::Start,
                HotkeyEvent::Finish,
                HotkeyEvent::Start,
                HotkeyEvent::Finish,
            ]
        );
        // The finishing press does not start another capture on release.
        assert!(!gesture.active && !gesture.locked);
    }

    #[test]
    fn slow_second_tap_or_disabled_lock_is_an_ordinary_hold() {
        for (enabled, gap) in [(true, 400), (false, 100)] {
            let mut gesture = Gesture::new(enabled);
            let now = Instant::now();
            let ms = |value| now + Duration::from_millis(value);
            gesture.trigger_down(ms(0), true);
            gesture.trigger_up(ms(100));
            gesture.trigger_down(ms(100 + gap), true);
            assert_eq!(
                gesture.trigger_up(ms(200 + gap)).event,
                Some(HotkeyEvent::Finish)
            );
            assert!(!gesture.locked);
        }
    }

    #[test]
    fn escape_cancels_held_and_locked_captures_only() {
        let mut gesture = Gesture::new(true);
        let now = Instant::now();
        assert_eq!(gesture.escape_down(), Decision::PASS);
        assert_eq!(gesture.escape_up(), Decision::PASS);

        gesture.trigger_down(now, true);
        let cancel = gesture.escape_down();
        assert!(cancel.suppress);
        assert_eq!(cancel.event, Some(HotkeyEvent::Cancel));
        assert_eq!(gesture.escape_down(), Decision::swallow(None));
        assert_eq!(gesture.escape_up(), Decision::swallow(None));
        // Releasing the cancelled trigger emits nothing and is still swallowed.
        assert_eq!(gesture.trigger_up(now), Decision::swallow(None));

        let ms = |value| now + Duration::from_millis(value);
        gesture.trigger_down(ms(1_000), true);
        gesture.trigger_up(ms(1_100));
        gesture.trigger_down(ms(1_200), true);
        gesture.trigger_up(ms(1_300));
        assert!(gesture.locked);
        assert_eq!(gesture.escape_down().event, Some(HotkeyEvent::Cancel));
        assert!(!gesture.locked);
    }

    #[test]
    fn cancel_forgets_a_pending_double_tap() {
        let mut gesture = Gesture::new(true);
        let now = Instant::now();
        let ms = |value| now + Duration::from_millis(value);
        gesture.trigger_down(ms(0), true);
        gesture.trigger_up(ms(100));
        gesture.trigger_down(ms(150), true);
        gesture.escape_down();
        gesture.escape_up();
        gesture.trigger_up(ms(200));
        gesture.trigger_down(ms(250), true);
        assert_eq!(gesture.trigger_up(ms(300)).event, Some(HotkeyEvent::Finish));
    }
}
