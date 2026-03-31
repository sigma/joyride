use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use joyride_config::{KeyCombo, Modifier};

fn source() -> CGEventSource {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .expect("failed to create event source")
}

pub struct MouseEmitter {
    cursor_pos: CGPoint,
    button_state: std::collections::HashMap<MouseButtonKind, bool>,
}

impl MouseEmitter {
    pub fn new() -> Self {
        let pos = CGEvent::new(source())
            .map(|e| e.location())
            .unwrap_or(CGPoint::new(500.0, 500.0));
        Self {
            cursor_pos: pos,
            button_state: std::collections::HashMap::new(),
        }
    }

    /// Returns true if any mouse button is currently held down.
    pub fn has_buttons_pressed(&self) -> bool {
        self.button_state.values().any(|&pressed| pressed)
    }

    pub fn move_cursor(&mut self, dx: f64, dy: f64) {
        if let Ok(event) = CGEvent::new(source()) {
            self.cursor_pos = event.location();
        }

        self.cursor_pos.x += dx;
        self.cursor_pos.y += dy;
        self.clamp_to_screen();

        if let Ok(event) =
            CGEvent::new_mouse_event(source(), CGEventType::MouseMoved, self.cursor_pos, CGMouseButton::Left)
        {
            event.post(CGEventTapLocation::Session);
        }
    }

    pub fn scroll(&self, dx: f64, dy: f64) {
        if let Ok(event) = CGEvent::new_scroll_event(
            source(),
            ScrollEventUnit::PIXEL,
            2,
            dy as i32,
            dx as i32,
            0,
        ) {
            event.post(CGEventTapLocation::Session);
        }
    }

    pub fn update_button(&mut self, button: MouseButtonKind, pressed: bool) {
        let was_pressed = self.button_state.get(&button).copied().unwrap_or(false);
        if pressed == was_pressed {
            return;
        }
        self.button_state.insert(button, pressed);

        if let Ok(event) = CGEvent::new(source()) {
            self.cursor_pos = event.location();
        }

        let (event_type, cg_button, button_number) = match (button, pressed) {
            (MouseButtonKind::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left, None),
            (MouseButtonKind::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left, None),
            (MouseButtonKind::Right, true) => {
                (CGEventType::RightMouseDown, CGMouseButton::Right, None)
            }
            (MouseButtonKind::Right, false) => {
                (CGEventType::RightMouseUp, CGMouseButton::Right, None)
            }
            (MouseButtonKind::Middle, true) => {
                (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(2))
            }
            (MouseButtonKind::Middle, false) => {
                (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(2))
            }
            (MouseButtonKind::Back, true) => {
                (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(3))
            }
            (MouseButtonKind::Back, false) => {
                (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(3))
            }
            (MouseButtonKind::Forward, true) => {
                (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(4))
            }
            (MouseButtonKind::Forward, false) => {
                (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(4))
            }
        };

        if let Ok(event) =
            CGEvent::new_mouse_event(source(), event_type, self.cursor_pos, cg_button)
        {
            if let Some(num) = button_number {
                event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, num);
            }
            event.post(CGEventTapLocation::Session);
        }
    }

    pub fn double_click(&mut self, button: MouseButtonKind) {
        if let Ok(event) = CGEvent::new(source()) {
            self.cursor_pos = event.location();
        }

        let (down_type, up_type, cg_button, button_number) = match button {
            MouseButtonKind::Left => (
                CGEventType::LeftMouseDown, CGEventType::LeftMouseUp,
                CGMouseButton::Left, None,
            ),
            MouseButtonKind::Right => (
                CGEventType::RightMouseDown, CGEventType::RightMouseUp,
                CGMouseButton::Right, None,
            ),
            MouseButtonKind::Middle => (
                CGEventType::OtherMouseDown, CGEventType::OtherMouseUp,
                CGMouseButton::Center, Some(2),
            ),
            MouseButtonKind::Back => (
                CGEventType::OtherMouseDown, CGEventType::OtherMouseUp,
                CGMouseButton::Center, Some(3),
            ),
            MouseButtonKind::Forward => (
                CGEventType::OtherMouseDown, CGEventType::OtherMouseUp,
                CGMouseButton::Center, Some(4),
            ),
        };

        for click_count in [1, 2] {
            for event_type in [down_type, up_type] {
                if let Ok(event) =
                    CGEvent::new_mouse_event(source(), event_type, self.cursor_pos, cg_button)
                {
                    event.set_integer_value_field(
                        EventField::MOUSE_EVENT_CLICK_STATE,
                        click_count,
                    );
                    if let Some(num) = button_number {
                        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, num);
                    }
                    event.post(CGEventTapLocation::Session);
                }
            }
        }
    }

    /// Emit a key press (down+up) with modifier flags.
    pub fn key_press(&self, combo: &KeyCombo) {
        let flags = modifiers_to_flags(&combo.modifiers);
        for key_down in [true, false] {
            if let Ok(event) = CGEvent::new_keyboard_event(source(), combo.keycode, key_down) {
                event.set_flags(flags);
                event.post(CGEventTapLocation::Session);
            }
        }
    }

    fn clamp_to_screen(&mut self) {
        let displays = CGDisplay::active_displays().unwrap_or_default();
        if displays.is_empty() {
            return;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for &display_id in &displays {
            let bounds = CGDisplay::new(display_id).bounds();
            min_x = min_x.min(bounds.origin.x);
            min_y = min_y.min(bounds.origin.y);
            max_x = max_x.max(bounds.origin.x + bounds.size.width);
            max_y = max_y.max(bounds.origin.y + bounds.size.height);
        }

        let (x, y) = clamp_point(
            self.cursor_pos.x, self.cursor_pos.y,
            min_x, min_y, max_x, max_y,
        );
        self.cursor_pos.x = x;
        self.cursor_pos.y = y;
    }
}

pub fn clamp_point(
    px: f64, py: f64,
    min_x: f64, min_y: f64,
    max_x: f64, max_y: f64,
) -> (f64, f64) {
    (px.clamp(min_x, max_x - 1.0), py.clamp(min_y, max_y - 1.0))
}

fn modifiers_to_flags(modifiers: &[Modifier]) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    for m in modifiers {
        flags |= match m {
            Modifier::Command => CGEventFlags::CGEventFlagCommand,
            Modifier::Control => CGEventFlags::CGEventFlagControl,
            Modifier::Option => CGEventFlags::CGEventFlagAlternate,
            Modifier::Shift => CGEventFlags::CGEventFlagShift,
        };
    }
    flags
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseButtonKind {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_emitter_constructs() {
        let emitter = MouseEmitter::new();
        assert!(emitter.button_state.is_empty());
    }

    #[test]
    fn mouse_button_kind_all_variants() {
        let variants = [
            MouseButtonKind::Left,
            MouseButtonKind::Right,
            MouseButtonKind::Middle,
            MouseButtonKind::Back,
            MouseButtonKind::Forward,
        ];
        assert_eq!(variants.len(), 5);
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn clamp_point_within_bounds() {
        let (x, y) = clamp_point(500.0, 300.0, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(x, 500.0);
        assert_eq!(y, 300.0);
    }

    #[test]
    fn clamp_point_exceeds_max() {
        let (x, y) = clamp_point(2000.0, 1200.0, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(x, 1919.0);
        assert_eq!(y, 1079.0);
    }

    #[test]
    fn clamp_point_below_min() {
        let (x, y) = clamp_point(-50.0, -10.0, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn clamp_point_negative_origin() {
        let (x, y) = clamp_point(-2000.0, 500.0, -1920.0, 0.0, 1920.0, 1080.0);
        assert_eq!(x, -1920.0);
        assert_eq!(y, 500.0);
    }

    #[test]
    fn update_button_tracks_state() {
        let mut emitter = MouseEmitter::new();
        emitter.update_button(MouseButtonKind::Left, true);
        assert_eq!(emitter.button_state.get(&MouseButtonKind::Left), Some(&true));
        emitter.update_button(MouseButtonKind::Left, false);
        assert_eq!(emitter.button_state.get(&MouseButtonKind::Left), Some(&false));
    }

    #[test]
    fn update_button_idempotent() {
        let mut emitter = MouseEmitter::new();
        emitter.update_button(MouseButtonKind::Right, true);
        // Second press should be no-op (returns early)
        emitter.update_button(MouseButtonKind::Right, true);
        assert_eq!(emitter.button_state.get(&MouseButtonKind::Right), Some(&true));
    }
}
