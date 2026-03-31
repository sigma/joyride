use std::collections::{HashMap, HashSet};

use joyride_config::{
    apply_deadzone, Action, GamepadState, MouseButtonKind, OutputEvent, OutputEventKind,
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
    pub button_map: HashMap<String, Action>,
}

/// Pure input-to-output translator. Owns all edge-detection state.
/// Call translate() each frame with the current gamepad state and config.
pub struct InputTranslator {
    /// Tracks how many source buttons are actively pressing each MouseButtonKind.
    /// MouseDown emitted when count goes 0→1, MouseUp when count goes 1→0.
    button_press_count: HashMap<MouseButtonKind, u32>,
    /// Per-source-button: which MouseButtonKind it was last contributing to.
    button_source_state: HashMap<String, (MouseButtonKind, bool)>,
    double_click_fired: HashSet<MouseButtonKind>,
    key_press_fired: HashSet<u16>,
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
            button_press_count: HashMap::new(),
            button_source_state: HashMap::new(),
            double_click_fired: HashSet::new(),
            key_press_fired: HashSet::new(),
        }
    }

    /// Returns true if any mouse button is currently held down.
    pub fn has_buttons_pressed(&self) -> bool {
        self.button_press_count.values().any(|&count| count > 0)
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

        // D-pad: slow, precise cursor movement (only for unmapped directions)
        let (dpx, dpy) = state.dpad;
        let dpad_x_mapped =
            !matches!(config.button_map.get("dpadLeft"), Some(Action::None) | None)
                || !matches!(config.button_map.get("dpadRight"), Some(Action::None) | None);
        let dpad_y_mapped =
            !matches!(config.button_map.get("dpadUp"), Some(Action::None) | None)
                || !matches!(config.button_map.get("dpadDown"), Some(Action::None) | None);
        let use_dpx = if dpad_x_mapped { 0.0 } else { dpx };
        let use_dpy = if dpad_y_mapped { 0.0 } else { dpy };
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

        // Buttons: dispatch based on mapping
        for (button_name, action) in &config.button_map {
            let pressed = state.pressed_buttons.contains(button_name.as_str());
            match action {
                Action::None => {}
                Action::LeftClick => self.process_button(button_name, MouseButtonKind::Left, pressed, &mut events),
                Action::RightClick => self.process_button(button_name, MouseButtonKind::Right, pressed, &mut events),
                Action::MiddleClick => self.process_button(button_name, MouseButtonKind::Middle, pressed, &mut events),
                Action::BackClick => self.process_button(button_name, MouseButtonKind::Back, pressed, &mut events),
                Action::ForwardClick => self.process_button(button_name, MouseButtonKind::Forward, pressed, &mut events),
                Action::DoubleLeftClick => self.process_double_click(MouseButtonKind::Left, pressed, &mut events),
                Action::DoubleRightClick => self.process_double_click(MouseButtonKind::Right, pressed, &mut events),
                Action::KeyPress(combo) => self.process_key_press(combo.keycode, &combo.modifiers, pressed, &mut events),
            }
        }

        events
    }

    fn process_button(
        &mut self,
        source: &str,
        button: MouseButtonKind,
        pressed: bool,
        events: &mut Vec<OutputEvent>,
    ) {
        let was_pressed = self.button_source_state
            .get(source)
            .map(|(_, p)| *p)
            .unwrap_or(false);
        if pressed == was_pressed {
            return;
        }
        self.button_source_state.insert(source.to_string(), (button, pressed));

        let count = self.button_press_count.entry(button).or_insert(0);
        if pressed {
            let was_zero = *count == 0;
            *count += 1;
            if was_zero {
                events.push(OutputEvent::immediate(OutputEventKind::MouseDown(button)));
            }
        } else {
            *count = count.saturating_sub(1);
            if *count == 0 {
                events.push(OutputEvent::immediate(OutputEventKind::MouseUp(button)));
            }
        }
    }

    fn process_double_click(
        &mut self,
        button: MouseButtonKind,
        pressed: bool,
        events: &mut Vec<OutputEvent>,
    ) {
        if pressed {
            if !self.double_click_fired.contains(&button) {
                self.double_click_fired.insert(button);
                // First click
                events.push(OutputEvent::immediate(OutputEventKind::MouseDown(button)));
                events.push(OutputEvent::immediate(OutputEventKind::MouseUp(button)));
                // Second click after delay
                events.push(OutputEvent::delayed(DOUBLE_CLICK_DELAY_MS, OutputEventKind::MouseDown(button)));
                events.push(OutputEvent::immediate(OutputEventKind::MouseUp(button)));
            }
        } else {
            self.double_click_fired.remove(&button);
        }
    }

    fn process_key_press(
        &mut self,
        keycode: u16,
        modifiers: &[joyride_config::Modifier],
        pressed: bool,
        events: &mut Vec<OutputEvent>,
    ) {
        let mods = modifiers.to_vec();
        if pressed {
            if !self.key_press_fired.contains(&keycode) {
                self.key_press_fired.insert(keycode);
                events.push(OutputEvent::immediate(OutputEventKind::KeyDown {
                    keycode,
                    modifiers: mods.clone(),
                }));
                events.push(OutputEvent::immediate(OutputEventKind::KeyUp {
                    keycode,
                    modifiers: mods,
                }));
            }
        } else {
            self.key_press_fired.remove(&keycode);
        }
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

    fn config_with_map(map: Vec<(&str, Action)>) -> TranslatorConfig {
        let mut config = default_config();
        config.button_map = map.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        config
    }

    fn state_with_buttons(buttons: &[&str]) -> GamepadState {
        let mut state = GamepadState::default();
        for b in buttons {
            state.pressed_buttons.insert(b.to_string());
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
        state.pressed_buttons.insert("dpadRight".to_string());
        let config = config_with_map(vec![("dpadRight", Action::LeftClick)]);
        let events = t.translate(&state, &config, 1.0 / 120.0);
        // Should have MouseDown but no MoveCursor from dpad
        let move_events: Vec<_> = events.iter().filter(|e| matches!(e.kind, OutputEventKind::MoveCursor { .. })).collect();
        assert!(move_events.is_empty());
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
        let config = config_with_map(vec![("buttonA", Action::LeftClick)]);
        let state = state_with_buttons(&["buttonA"]);
        let events = t.translate(&state, &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseDown(MouseButtonKind::Left)]);
    }

    #[test]
    fn button_release_emits_mouse_up() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![("buttonA", Action::LeftClick)]);
        // Press
        t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        // Release
        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
    }

    #[test]
    fn button_held_no_repeat() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![("buttonA", Action::LeftClick)]);
        let state = state_with_buttons(&["buttonA"]);
        // Frame 1: press
        let events1 = t.translate(&state, &config, 0.0);
        assert_eq!(events1.len(), 1);
        // Frame 2: held — no new events
        let events2 = t.translate(&state, &config, 0.0);
        assert!(events2.is_empty());
    }

    // -- Regression: button release on idle frame --

    #[test]
    fn regression_release_on_idle_frame() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![("buttonA", Action::LeftClick)]);

        // Press
        t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        assert!(t.has_buttons_pressed());

        // Release (idle gamepad)
        let idle = GamepadState::default();
        assert!(idle.is_idle());
        let events = t.translate(&idle, &config, 0.0);

        // Must emit MouseUp even though gamepad is idle
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
        assert!(!t.has_buttons_pressed());
    }

    // -- Regression: double-click fires once --

    #[test]
    fn regression_double_click_once_per_press() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![("buttonX", Action::DoubleLeftClick)]);
        let state = state_with_buttons(&["buttonX"]);

        // Frame 1: fires the full double-click sequence
        let events1 = t.translate(&state, &config, 0.0);
        assert_eq!(events1.len(), 4);
        assert_eq!(events1[0].kind, OutputEventKind::MouseDown(MouseButtonKind::Left));
        assert_eq!(events1[1].kind, OutputEventKind::MouseUp(MouseButtonKind::Left));
        assert_eq!(events1[2].kind, OutputEventKind::MouseDown(MouseButtonKind::Left));
        assert_eq!(events1[2].delay_ms, DOUBLE_CLICK_DELAY_MS);
        assert_eq!(events1[3].kind, OutputEventKind::MouseUp(MouseButtonKind::Left));

        // Frame 2: held — no repeat
        let events2 = t.translate(&state, &config, 0.0);
        assert!(events2.is_empty());

        // Release
        t.translate(&GamepadState::default(), &config, 0.0);

        // Next press fires again
        let events3 = t.translate(&state, &config, 0.0);
        assert_eq!(events3.len(), 4);
    }

    // -- Regression: double-click doesn't corrupt single-click --

    #[test]
    fn regression_double_click_independent_of_single_click() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            ("buttonA", Action::LeftClick),
            ("buttonX", Action::DoubleLeftClick),
        ]);

        // Press A (single click)
        let events = t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        assert!(events.iter().any(|e| e.kind == OutputEventKind::MouseDown(MouseButtonKind::Left)));

        // Hold A, nothing new
        let events = t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        assert!(events.is_empty());

        // Release A
        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert!(events.iter().any(|e| e.kind == OutputEventKind::MouseUp(MouseButtonKind::Left)));

        // Now press X (double-click) — should fire independently
        let events = t.translate(&state_with_buttons(&["buttonX"]), &config, 0.0);
        assert_eq!(events.len(), 4); // full double-click sequence

        // Release X
        t.translate(&GamepadState::default(), &config, 0.0);

        // Press A again — should still work as single click
        let events = t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
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
        let config = config_with_map(vec![("buttonY", Action::KeyPress(combo))]);
        let state = state_with_buttons(&["buttonY"]);
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
        let config = config_with_map(vec![("buttonY", Action::KeyPress(combo))]);
        let state = state_with_buttons(&["buttonY"]);

        let events1 = t.translate(&state, &config, 0.0);
        assert_eq!(events1.len(), 2);

        // Held — no repeat
        let events2 = t.translate(&state, &config, 0.0);
        assert!(events2.is_empty());

        // Release then re-press
        t.translate(&GamepadState::default(), &config, 0.0);
        let events3 = t.translate(&state, &config, 0.0);
        assert_eq!(events3.len(), 2);
    }

    // -- Multiple simultaneous buttons --

    #[test]
    fn multiple_buttons_simultaneous() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            ("buttonA", Action::LeftClick),
            ("buttonB", Action::RightClick),
        ]);
        let state = state_with_buttons(&["buttonA", "buttonB"]);
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

    // -- Multiple sources → same MouseButtonKind --

    #[test]
    fn two_buttons_same_action_no_premature_release() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            ("buttonA", Action::LeftClick),
            ("buttonB", Action::LeftClick),
        ]);

        // Press A → MouseDown
        let events = t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseDown(MouseButtonKind::Left)]);

        // Press B while A held → no new MouseDown (already down)
        let events = t.translate(&state_with_buttons(&["buttonA", "buttonB"]), &config, 0.0);
        assert!(events.is_empty());

        // Release A, B still held → no MouseUp
        let events = t.translate(&state_with_buttons(&["buttonB"]), &config, 0.0);
        assert!(events.is_empty());
        assert!(t.has_buttons_pressed());

        // Release B → MouseUp
        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
        assert!(!t.has_buttons_pressed());
    }

    #[test]
    fn two_buttons_same_action_reverse_release_order() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![
            ("buttonA", Action::LeftClick),
            ("buttonB", Action::LeftClick),
        ]);

        // Press both
        let events = t.translate(&state_with_buttons(&["buttonA", "buttonB"]), &config, 0.0);
        let kinds: Vec<_> = event_kinds(&events);
        // Only one MouseDown despite two sources
        assert_eq!(kinds.iter().filter(|k| ***k == OutputEventKind::MouseDown(MouseButtonKind::Left)).count(), 1);

        // Release B, A still held → no MouseUp
        let events = t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        assert!(events.is_empty());

        // Release A → MouseUp
        let events = t.translate(&GamepadState::default(), &config, 0.0);
        assert_eq!(event_kinds(&events), vec![&OutputEventKind::MouseUp(MouseButtonKind::Left)]);
    }

    #[test]
    fn has_buttons_pressed_tracks_state() {
        let mut t = InputTranslator::new();
        let config = config_with_map(vec![("buttonA", Action::LeftClick)]);
        assert!(!t.has_buttons_pressed());

        t.translate(&state_with_buttons(&["buttonA"]), &config, 0.0);
        assert!(t.has_buttons_pressed());

        t.translate(&GamepadState::default(), &config, 0.0);
        assert!(!t.has_buttons_pressed());
    }
}
