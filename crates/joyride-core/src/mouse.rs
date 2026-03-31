use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

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

        self.cursor_pos.x = self.cursor_pos.x.clamp(min_x, max_x - 1.0);
        self.cursor_pos.y = self.cursor_pos.y.clamp(min_y, max_y - 1.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButtonKind {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}
