use std::collections::HashMap;
use std::time::Instant;

use joyride_config::{
    apply_deadzone, Action, GamepadState, InputId, MouseButtonKind, OutputEvent, OutputEventKind,
};

/// Configuration snapshot for one translate() call.
/// Built from the active profile's settings each frame.
#[derive(Clone)]
pub struct TranslatorConfig {
    pub cursor_speed: f64,
    pub dpad_speed: f64,
    pub scroll_speed: f64,
    pub deadzone: f32,
    pub natural_scroll: bool,
    pub button_map: HashMap<InputId, Action>,
}

/// Per-source-button state, tracking what was last emitted and when.
#[derive(Clone, Debug)]
enum SourceState {
    /// Not active (button released or never pressed).
    Idle,
    /// Holding a mouse button (momentary click) since the given instant.
    MouseHeld { button: MouseButtonKind, since: Instant },
    /// Fired a one-shot action (double-click, key press) at the given instant,
    /// waiting for release.
    FiredOnce { since: Instant },
}

/// Pure input-to-output translator. Owns all edge-detection state.
/// Call translate() each frame with the current gamepad state and config.
pub struct InputTranslator {
    /// Per-source state keyed by gamepad button name.
    source_state: HashMap<InputId, SourceState>,
    /// Reference count of how many sources are pressing each MouseButtonKind.
    /// MouseDown emitted at 0→1, MouseUp at 1→0.
    mouse_press_count: HashMap<MouseButtonKind, u32>,
}

/// Delay in ms between clicks in a double-click sequence.
const DOUBLE_CLICK_DELAY_MS: u32 = 50;

impl Default for InputTranslator {
    fn default() -> Self {
        Self::new()
    }
}

impl InputTranslator {
    pub fn new() -> Self {
        Self {
            source_state: HashMap::new(),
            mouse_press_count: HashMap::new(),
        }
    }

    /// Returns how long the given source button has been held, or None if idle.
    pub fn hold_duration(&self, source: InputId) -> Option<std::time::Duration> {
        match self.source_state.get(&source)? {
            SourceState::Idle => None,
            SourceState::MouseHeld { since, .. } | SourceState::FiredOnce { since } => {
                Some(since.elapsed())
            }
        }
    }

    /// Returns true if any mouse button is currently held down.
    pub fn has_buttons_pressed(&self) -> bool {
        self.mouse_press_count.values().any(|&count| count > 0)
    }

    /// Release any buttons whose source is no longer present in the config's button_map,
    /// or whose mapping has changed. Call this when the active profile changes.
    pub fn flush_stale_buttons(
        &mut self,
        config: &TranslatorConfig,
    ) -> Vec<OutputEvent> {
        let mut events = Vec::new();

        let stale: Vec<(InputId, SourceState)> = self
            .source_state
            .iter()
            .filter(|(_, state)| !matches!(state, SourceState::Idle))
            .filter(|(source, state)| {
                let current_action = config.button_map.get(source);
                match (state, current_action) {
                    (SourceState::MouseHeld { button: btn, .. }, Some(action)) => {
                        action_mouse_button(action) != Some(*btn)
                    }
                    (_, None) => true,
                    (_, Some(Action::None)) => true,
                    (SourceState::FiredOnce { .. }, Some(_)) => false, // will reset on release
                    _ => true,
                }
            })
            .map(|(source, state)| (*source, state.clone()))
            .collect();

        for (source, state) in stale {
            if let SourceState::MouseHeld { button: btn, .. } = state {
                self.release_mouse(btn, &mut events);
            }
            self.source_state.insert(source, SourceState::Idle);
        }

        events
    }

    /// Translate gamepad state + config into output events.
    pub fn translate(
        &mut self,
        state: &GamepadState,
        config: &TranslatorConfig,
        dt: f64,
    ) -> Vec<OutputEvent> {
        let mut events = Vec::with_capacity(8);
        let dz = config.deadzone;

        // Left stick: fast cursor movement
        let (lx, ly) = state.left_stick;
        if lx.abs() > dz || ly.abs() > dz {
            let x = apply_deadzone(lx, dz);
            let y = apply_deadzone(ly, dz);
            let dx = x as f64 * config.cursor_speed * dt;
            let dy = -y as f64 * config.cursor_speed * dt;
            events.push(OutputEvent::immediate(OutputEventKind::MoveCursor { dx, dy }));
        }

        // D-pad: slow, precise cursor movement.
        // Suppress only the contribution from mapped directions, not the entire axis.
        let (dpx, dpy) = state.dpad;
        let left_mapped = !matches!(config.button_map.get(&InputId::DpadLeft), Some(Action::None) | None);
        let right_mapped = !matches!(config.button_map.get(&InputId::DpadRight), Some(Action::None) | None);
        let up_mapped = !matches!(config.button_map.get(&InputId::DpadUp), Some(Action::None) | None);
        let down_mapped = !matches!(config.button_map.get(&InputId::DpadDown), Some(Action::None) | None);
        let use_dpx = if (dpx < 0.0 && left_mapped) || (dpx > 0.0 && right_mapped) {
            0.0
        } else {
            dpx
        };
        let use_dpy = if (dpy > 0.0 && up_mapped) || (dpy < 0.0 && down_mapped) {
            0.0
        } else {
            dpy
        };
        if use_dpx.abs() > 0.1 || use_dpy.abs() > 0.1 {
            let dx = use_dpx as f64 * config.dpad_speed * dt;
            let dy = -use_dpy as f64 * config.dpad_speed * dt;
            events.push(OutputEvent::immediate(OutputEventKind::MoveCursor { dx, dy }));
        }

        // Right stick: scroll
        let (rx, ry) = state.right_stick;
        if rx.abs() > dz || ry.abs() > dz {
            let x = apply_deadzone(rx, dz);
            let y = apply_deadzone(ry, dz);
            let scroll_dir: f64 = if config.natural_scroll { -1.0 } else { 1.0 };
            let sdx = x as f64 * config.scroll_speed;
            let sdy = y as f64 * config.scroll_speed * scroll_dir;
            events.push(OutputEvent::immediate(OutputEventKind::Scroll { dx: sdx, dy: sdy }));
        }

        // Buttons: unified action dispatch
        for (&source, action) in &config.button_map {
            let pressed = state.pressed_buttons.contains(&source);
            self.dispatch_action(source, action, pressed, &mut events);
        }

        events
    }

    /// Unified action dispatch. Determines behavior from the Action variant.
    fn dispatch_action(
        &mut self,
        source: InputId,
        action: &Action,
        pressed: bool,
        events: &mut Vec<OutputEvent>,
    ) {
        match action {
            Action::None => {}

            // Momentary mouse buttons: MouseDown on press, MouseUp on release
            Action::LeftClick | Action::RightClick | Action::MiddleClick
            | Action::BackClick | Action::ForwardClick => {
                let button = action_mouse_button(action).unwrap();
                self.dispatch_momentary_click(source, button, pressed, events);
            }

            // Fire-once on press edge: emit full double-click sequence
            Action::DoubleLeftClick | Action::DoubleRightClick => {
                let button = match action {
                    Action::DoubleLeftClick => MouseButtonKind::Left,
                    Action::DoubleRightClick => MouseButtonKind::Right,
                    _ => unreachable!(),
                };
                self.dispatch_fire_once(source, pressed, || vec![
                    OutputEvent::immediate(OutputEventKind::MouseDown(button)),
                    OutputEvent::immediate(OutputEventKind::MouseUp(button)),
                    OutputEvent::delayed(DOUBLE_CLICK_DELAY_MS, OutputEventKind::MouseDown(button)),
                    OutputEvent::immediate(OutputEventKind::MouseUp(button)),
                ], events);
            }

            // Fire-once on press edge: emit key down+up
            Action::KeyPress(combo) => {
                let keycode = combo.keycode;
                let mods = combo.modifiers.clone();
                self.dispatch_fire_once(source, pressed, || vec![
                    OutputEvent::immediate(OutputEventKind::KeyDown {
                        keycode,
                        modifiers: mods.clone(),
                    }),
                    OutputEvent::immediate(OutputEventKind::KeyUp {
                        keycode,
                        modifiers: mods,
                    }),
                ], events);
            }
        }
    }

    /// Momentary click: track press/release with reference counting.
    fn dispatch_momentary_click(
        &mut self,
        source: InputId,
        button: MouseButtonKind,
        pressed: bool,
        events: &mut Vec<OutputEvent>,
    ) {
        let current = self.source_state.get(&source).cloned().unwrap_or(SourceState::Idle);
        let was_active = matches!(current, SourceState::MouseHeld { .. });

        if pressed && !was_active {
            self.source_state.insert(source, SourceState::MouseHeld {
                button,
                since: Instant::now(),
            });
            self.press_mouse(button, events);
        } else if !pressed && was_active {
            self.source_state.insert(source, SourceState::Idle);
            if let SourceState::MouseHeld { button: btn, .. } = current {
                self.release_mouse(btn, events);
            }
        }
    }

    /// Fire-once: emit events on press edge, ignore while held, reset on release.
    fn dispatch_fire_once(
        &mut self,
        source: InputId,
        pressed: bool,
        make_events: impl FnOnce() -> Vec<OutputEvent>,
        events: &mut Vec<OutputEvent>,
    ) {
        let current = self.source_state.get(&source).cloned().unwrap_or(SourceState::Idle);

        if pressed && matches!(current, SourceState::Idle) {
            self.source_state.insert(source, SourceState::FiredOnce {
                since: Instant::now(),
            });
            events.extend(make_events());
        } else if !pressed && matches!(current, SourceState::FiredOnce { .. }) {
            self.source_state.insert(source, SourceState::Idle);
        }
    }

    fn press_mouse(&mut self, button: MouseButtonKind, events: &mut Vec<OutputEvent>) {
        let count = self.mouse_press_count.entry(button).or_insert(0);
        let was_zero = *count == 0;
        *count += 1;
        if was_zero {
            events.push(OutputEvent::immediate(OutputEventKind::MouseDown(button)));
        }
    }

    fn release_mouse(&mut self, button: MouseButtonKind, events: &mut Vec<OutputEvent>) {
        let count = self.mouse_press_count.entry(button).or_insert(0);
        *count = count.saturating_sub(1);
        if *count == 0 {
            events.push(OutputEvent::immediate(OutputEventKind::MouseUp(button)));
        }
    }
}

/// Map an Action variant to its MouseButtonKind, if applicable.
/// Map an Action variant to its MouseButtonKind, if applicable.
/// Exhaustive match — adding a new Action variant forces updating this.
fn action_mouse_button(action: &Action) -> Option<MouseButtonKind> {
    match action {
        Action::LeftClick => Some(MouseButtonKind::Left),
        Action::RightClick => Some(MouseButtonKind::Right),
        Action::MiddleClick => Some(MouseButtonKind::Middle),
        Action::BackClick => Some(MouseButtonKind::Back),
        Action::ForwardClick => Some(MouseButtonKind::Forward),
        Action::None
        | Action::DoubleLeftClick
        | Action::DoubleRightClick
        | Action::KeyPress(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use joyride_config::KeyCombo;

    fn default_config() -> TranslatorConfig {
        TranslatorConfig {
            cursor_speed: 1500.0,
            dpad_speed: 150.0,
            scroll_speed: 8.0,
            deadzone: 0.15,
            natural_scroll: false,
            button_map: HashMap::new(),
        }
    }

    fn config_with_map(map: Vec<(&InputId, Action)>) -> TranslatorConfig {
        let mut config = default_config();
        config.button_map = map.into_iter().map(|(k, v)| (*k, v)).collect();
        config
    }

    fn state_with_buttons(buttons: &[InputId]) -> GamepadState {
        let mut state = GamepadState::default();
        for &b in buttons {
            state.pressed_buttons.insert(b);
        }
        state
    }

    fn event_kinds(events: &[OutputEvent]) -> Vec<&OutputEventKind> {
        events.iter().map(|e| &e.kind).collect()
    }

    // -- Stick movement --

    #[test]
    fn left_stick_produces_move_cursor() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.left_stick = (0.5, -0.3);
        let events = t.translate(&state, &default_config(), 1.0 / 120.0);
        assert!(events.iter().any(|e| matches!(e.kind, OutputEventKind::MoveCursor { .. })));
    }

    #[test]
    fn left_stick_within_deadzone_no_events() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.left_stick = (0.1, -0.1);
        let events = t.translate(&state, &default_config(), 1.0 / 120.0);
        assert!(events.is_empty());
    }

    #[test]
    fn dt_scales_cursor_movement() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.left_stick = (1.0, 0.0);

        let events_fast = t.translate(&state, &default_config(), 1.0 / 60.0);
        let events_slow = t.translate(&state, &default_config(), 1.0 / 120.0);

        let dx_fast = match &events_fast[0].kind {
            OutputEventKind::MoveCursor { dx, .. } => *dx,
            _ => panic!("expected MoveCursor"),
        };
        let dx_slow = match &events_slow[0].kind {
            OutputEventKind::MoveCursor { dx, .. } => *dx,
            _ => panic!("expected MoveCursor"),
        };
        assert!((dx_fast / dx_slow - 2.0).abs() < 0.01);
    }

    // -- D-pad --

    #[test]
    fn dpad_unmapped_produces_cursor() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.dpad = (1.0, 0.0);
        let events = t.translate(&state, &default_config(), 1.0 / 120.0);
        assert!(events.iter().any(|e| matches!(e.kind, OutputEventKind::MoveCursor { .. })));
    }

    #[test]
    fn dpad_mapped_suppresses_cursor() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.dpad = (1.0, 0.0);
        state.pressed_buttons.insert(InputId::DpadRight);
        let config = config_with_map(vec![(&InputId::DpadRight, Action::LeftClick)]);
        let events = t.translate(&state, &config, 1.0 / 120.0);
        let move_events: Vec<_> = events.iter().filter(|e| matches!(e.kind, OutputEventKind::MoveCursor { .. })).collect();
        assert!(move_events.is_empty());
    }

    #[test]
    fn dpad_mapped_right_does_not_suppress_left_cursor() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.dpad = (-1.0, 0.0);
        let config = config_with_map(vec![(&InputId::DpadRight, Action::LeftClick)]);
        let events = t.translate(&state, &config, 1.0 / 120.0);
        let move_events: Vec<_> = events.iter().filter(|e| matches!(e.kind, OutputEventKind::MoveCursor { .. })).collect();
        assert!(!move_events.is_empty(), "unmapped dpadLeft should still move cursor");
        if let OutputEventKind::MoveCursor { dx, .. } = &move_events[0].kind {
            assert!(*dx < 0.0, "cursor should move left");
        }
    }

    #[test]
    fn dpad_mapped_up_does_not_suppress_down_cursor() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.dpad = (0.0, -1.0);
        let config = config_with_map(vec![(&InputId::DpadUp, Action::RightClick)]);
        let events = t.translate(&state, &config, 1.0 / 120.0);
        let move_events: Vec<_> = events.iter().filter(|e| matches!(e.kind, OutputEventKind::MoveCursor { .. })).collect();
        assert!(!move_events.is_empty(), "unmapped dpadDown should still move cursor");
    }

    // -- Scroll --

    #[test]
    fn right_stick_produces_scroll() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.right_stick = (0.0, 0.5);
        let events = t.translate(&state, &default_config(), 1.0 / 120.0);
        assert!(events.iter().any(|e| matches!(e.kind, OutputEventKind::Scroll { .. })));
    }

    #[test]
    fn natural_scroll_inverts_direction() {
        let mut t = InputTranslator::new();
        let mut state = GamepadState::default();
        state.right_stick = (0.0, 1.0);

        let mut config_normal = default_config();
        config_normal.natural_scroll = false;
        let events_normal = t.translate(&state, &config_normal, 1.0);

        let mut config_natural = default_config();
        config_natural.natural_scroll = true;
        let events_natural = t.translate(&state, &config_natural, 1.0);

        let dy_normal = match &events_normal[0].kind {
            OutputEventKind::Scroll { dy, .. } => *dy,
            _ => panic!("expected Scroll"),
        };
        let dy_natural = match &events_natural[0].kind {
            OutputEventKind::Scroll { dy, .. } => *dy,
            _ => panic!("expected Scroll"),
        };
        assert!((dy_normal + dy_natural).abs() < 1e-6, "natural scroll should invert dy");
    }

    // -- Button press/release --

    #[test]
    fn button_press_emits_mouse_down() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        let state = state_with_buttons(&[InputId::ButtonA]);
        let events = t.translate(&state, &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseDown(MouseButtonKind::Left)]);
    }

    #[test]
    fn button_release_emits_mouse_up() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
    }

    #[test]
    fn button_held_no_repeat() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        let state = state_with_buttons(&[InputId::ButtonA]);
        let events1 = t.translate(&state, &config, 0.0);
        assert_eq!(events1.len(), 1);
        let events2 = t.translate(&state, &config, 0.0);
        assert!(events2.is_empty());
    }

    // -- Regression: button release on idle frame --

    #[test]
    fn regression_release_on_idle_frame() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert!(t.has_buttons_pressed());

        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
        assert!(!t.has_buttons_pressed());
    }

    // -- Regression: double-click fires once --

    #[test]
    fn regression_double_click_once_per_press() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonX, Action::DoubleLeftClick)]);
        let state = state_with_buttons(&[InputId::ButtonX]);

        let events1 = t.translate(&state, &config, 0.0);
        assert_eq!(events1.len(), 4);
        assert_eq!(events1[0].kind, OutputEventKind::MouseDown(MouseButtonKind::Left));
        assert_eq!(events1[1].kind, OutputEventKind::MouseUp(MouseButtonKind::Left));
        assert_eq!(events1[2].kind, OutputEventKind::MouseDown(MouseButtonKind::Left));
        assert_eq!(events1[2].delay_ms, DOUBLE_CLICK_DELAY_MS);
        assert_eq!(events1[3].kind, OutputEventKind::MouseUp(MouseButtonKind::Left));

        let events2 = t.translate(&state, &config, 0.0);
        assert!(events2.is_empty());

        t.translate(&GamepadState::default(), &config, 0.0);
        let events3 = t.translate(&state, &config, 0.0);
        assert_eq!(events3.len(), 4);
    }

    // -- Regression: double-click doesn't corrupt single-click --

    #[test]
    fn regression_double_click_independent_of_single_click() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            (&InputId::ButtonA, Action::LeftClick),
            (&InputId::ButtonX, Action::DoubleLeftClick),
        ]);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert!(events.iter().any(|e| e.kind == OutputEventKind::MouseDown(MouseButtonKind::Left)));

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert!(events.is_empty());

        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert!(events.iter().any(|e| e.kind == OutputEventKind::MouseUp(MouseButtonKind::Left)));

        let events = t.translate(&state_with_buttons(&[InputId::ButtonX]), &config, 0.0);
        assert_eq!(events.len(), 4);

        t.translate(&GamepadState::default(), &config, 0.0);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseDown(MouseButtonKind::Left)]);
    }

    // -- Key press --

    #[test]
    fn key_press_emits_down_up() {
        let mut t = InputTranslator::new();
        let combo = KeyCombo {
            modifiers: vec![joyride_config::Modifier::Command],
            keycode: 0x00,
            key_name: "A".to_string(),
        };
        let config = config_with_map(vec![(&InputId::ButtonY, Action::KeyPress(combo))]);
        let state = state_with_buttons(&[InputId::ButtonY]);
        let events = t.translate(&state, &config, 0.0);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0].kind, OutputEventKind::KeyDown { keycode: 0x00, .. }));
        assert!(matches!(events[1].kind, OutputEventKind::KeyUp { keycode: 0x00, .. }));
    }

    #[test]
    fn key_press_no_repeat_while_held() {
        let mut t = InputTranslator::new();
        let combo = KeyCombo {
            modifiers: vec![],
            keycode: 0x31,
            key_name: "Space".to_string(),
        };
        let config = config_with_map(vec![(&InputId::ButtonY, Action::KeyPress(combo))]);
        let state = state_with_buttons(&[InputId::ButtonY]);

        let events1 = t.translate(&state, &config, 0.0);
        assert_eq!(events1.len(), 2);

        let events2 = t.translate(&state, &config, 0.0);
        assert!(events2.is_empty());

        t.translate(&GamepadState::default(), &config, 0.0);
        let events3 = t.translate(&state, &config, 0.0);
        assert_eq!(events3.len(), 2);
    }

    // -- Multiple sources → same MouseButtonKind --

    #[test]
    fn two_buttons_same_action_no_premature_release() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            (&InputId::ButtonA, Action::LeftClick),
            (&InputId::ButtonB, Action::LeftClick),
        ]);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseDown(MouseButtonKind::Left)]);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA, InputId::ButtonB]), &config, 0.0);
        assert!(events.is_empty());

        let events = t.translate(&state_with_buttons(&[InputId::ButtonB]), &config, 0.0);
        assert!(events.is_empty());
        assert!(t.has_buttons_pressed());

        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
        assert!(!t.has_buttons_pressed());
    }

    #[test]
    fn two_buttons_same_action_reverse_release_order() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            (&InputId::ButtonA, Action::LeftClick),
            (&InputId::ButtonB, Action::LeftClick),
        ]);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA, InputId::ButtonB]), &config, 0.0);
        let kinds: Vec<_> = event_kinds(&events);
        assert_eq!(kinds.iter().filter(|k| ***k == OutputEventKind::MouseDown(MouseButtonKind::Left)).count(), 1);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert!(events.is_empty());

        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
    }

    // -- Profile switch with held buttons --

    #[test]
    fn profile_switch_releases_orphaned_buttons() {
        let mut t = InputTranslator::new();
        let config1 = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        let config2 = config_with_map(vec![(&InputId::ButtonA, Action::None)]);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config1, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseDown(MouseButtonKind::Left)]);
        assert!(t.has_buttons_pressed());

        let flush = t.flush_stale_buttons(&config2);
        assert_eq!(event_kinds(&flush), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
        assert!(!t.has_buttons_pressed());
    }

    #[test]
    fn profile_switch_remapped_button_releases_old() {
        let mut t = InputTranslator::new();
        let config1 = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        let config2 = config_with_map(vec![(&InputId::ButtonA, Action::RightClick)]);

        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config1, 0.0);

        let flush = t.flush_stale_buttons(&config2);
        assert_eq!(event_kinds(&flush), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);

        let events = t.translate(&state_with_buttons(&[InputId::ButtonA]), &config2, 0.0);
        assert!(events.iter().any(|e| e.kind == OutputEventKind::MouseDown(MouseButtonKind::Right)));
    }

    #[test]
    fn profile_switch_no_flush_when_mapping_unchanged() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);

        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);

        let flush = t.flush_stale_buttons(&config);
        assert!(flush.is_empty());
        assert!(t.has_buttons_pressed());
    }

    // -- Multiple simultaneous buttons --

    #[test]
    fn multiple_buttons_simultaneous() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            (&InputId::ButtonA, Action::LeftClick),
            (&InputId::ButtonB, Action::RightClick),
        ]);
        let state = state_with_buttons(&[InputId::ButtonA, InputId::ButtonB]);
        let events = t.translate(&state, &config, 0.0);
        let kinds: Vec<_> = event_kinds(&events);
        assert!(kinds.contains(&&OutputEventKind::MouseDown(MouseButtonKind::Left)));
        assert!(kinds.contains(&&OutputEventKind::MouseDown(MouseButtonKind::Right)));
    }

    // -- Idle state --

    #[test]
    fn idle_no_events() {
        let mut t = InputTranslator::new();
        let events = t.translate(&GamepadState::default(), &default_config(), 1.0 / 120.0);
        assert!(events.is_empty());
    }

    // -- Hold duration tracking --

    #[test]
    fn hold_duration_none_when_idle() {
        let t = InputTranslator::new();
        assert!(t.hold_duration(InputId::ButtonA).is_none());
    }

    #[test]
    fn hold_duration_some_when_pressed() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        let dur = t.hold_duration(InputId::ButtonA);
        assert!(dur.is_some());
        // Should be very short (just pressed)
        assert!(dur.unwrap().as_millis() < 100);
    }

    #[test]
    fn hold_duration_none_after_release() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        t.translate(&GamepadState::default(), &config, 0.0);
        assert!(t.hold_duration(InputId::ButtonA).is_none());
    }

    #[test]
    fn has_buttons_pressed_tracks_state() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![(&InputId::ButtonA, Action::LeftClick)]);
        assert!(!t.has_buttons_pressed());

        t.translate(&state_with_buttons(&[InputId::ButtonA]), &config, 0.0);
        assert!(t.has_buttons_pressed());

        t.translate(&GamepadState::default(), &config, 0.0);
        assert!(!t.has_buttons_pressed());
    }
}
