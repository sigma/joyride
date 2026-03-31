use std::collections::HashMap;
use std::ffi::c_void;
use std::time::Instant;

use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

pub use joyride_config::MouseButtonKind;
use joyride_config::{EventEmitter, Modifier, OutputEvent, OutputEventKind};

// libdispatch FFI for scheduling delayed events
extern "C" {
    fn dispatch_after(when: u64, queue: *const c_void, block: *const c_void);
    fn dispatch_time(base: u64, delta: i64) -> u64;
    static _dispatch_main_q: c_void;
}
const DISPATCH_TIME_NOW: u64 = 0;
const NSEC_PER_MSEC: i64 = 1_000_000;

fn source() -> CGEventSource {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .expect("failed to create event source")
}

/// System double-click interval (500ms is a safe default).
const DOUBLE_CLICK_INTERVAL_MS: u128 = 500;

/// Posts output events to macOS via CoreGraphics.
/// Tracks cursor position and derives click counts from inter-click timing.
/// All edge-detection logic lives in [`InputTranslator`], not here.
pub struct MouseEmitter {
    cursor_pos: CGPoint,
    /// Per-button last-click time for deriving click_count.
    last_click_time: HashMap<MouseButtonKind, Instant>,
    last_click_count: HashMap<MouseButtonKind, i64>,
}

impl Default for MouseEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseEmitter {
    pub fn new() -> Self {
        let pos = CGEvent::new(source())
            .map(|e| e.location())
            .unwrap_or(CGPoint::new(500.0, 500.0));
        Self {
            cursor_pos: pos,
            last_click_time: HashMap::new(),
            last_click_count: HashMap::new(),
        }
    }

    fn refresh_cursor_pos(&mut self) {
        if let Ok(event) = CGEvent::new(source()) {
            self.cursor_pos = event.location();
        }
    }

    fn move_cursor(&mut self, dx: f64, dy: f64) {
        self.refresh_cursor_pos();
        self.cursor_pos.x += dx;
        self.cursor_pos.y += dy;
        self.clamp_to_screen();

        if let Ok(event) = CGEvent::new_mouse_event(
            source(),
            CGEventType::MouseMoved,
            self.cursor_pos,
            CGMouseButton::Left,
        ) {
            event.post(CGEventTapLocation::Session);
        }
    }

    fn scroll(&self, dx: f64, dy: f64) {
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

    fn mouse_down(&mut self, button: MouseButtonKind) {
        self.refresh_cursor_pos();
        let click_count = self.compute_click_count(button);
        self.post_mouse_event(button, true, click_count);
    }

    fn mouse_up(&mut self, button: MouseButtonKind) {
        self.refresh_cursor_pos();
        let click_count = self.last_click_count.get(&button).copied().unwrap_or(1);
        self.post_mouse_event(button, false, click_count);
    }

    fn compute_click_count(&mut self, button: MouseButtonKind) -> i64 {
        let now = Instant::now();
        let count = if let Some(last) = self.last_click_time.get(&button) {
            let elapsed = now.duration_since(*last).as_millis();
            if elapsed < DOUBLE_CLICK_INTERVAL_MS {
                self.last_click_count.get(&button).copied().unwrap_or(0) + 1
            } else {
                1
            }
        } else {
            1
        };
        self.last_click_time.insert(button, now);
        self.last_click_count.insert(button, count);
        count
    }

    fn post_mouse_event(&self, button: MouseButtonKind, pressed: bool, click_count: i64) {
        let (event_type, cg_button, button_number) = match (button, pressed) {
            (MouseButtonKind::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left, None),
            (MouseButtonKind::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left, None),
            (MouseButtonKind::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right, None),
            (MouseButtonKind::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right, None),
            (MouseButtonKind::Middle, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(2)),
            (MouseButtonKind::Middle, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(2)),
            (MouseButtonKind::Back, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(3)),
            (MouseButtonKind::Back, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(3)),
            (MouseButtonKind::Forward, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(4)),
            (MouseButtonKind::Forward, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(4)),
        };

        if let Ok(event) = CGEvent::new_mouse_event(source(), event_type, self.cursor_pos, cg_button) {
            event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, click_count);
            if let Some(num) = button_number {
                event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, num);
            }
            event.post(CGEventTapLocation::Session);
        }
    }

    fn key_down(&self, keycode: u16, modifiers: &[Modifier]) {
        let flags = modifiers_to_flags(modifiers);
        if let Ok(event) = CGEvent::new_keyboard_event(source(), keycode, true) {
            event.set_flags(flags);
            event.post(CGEventTapLocation::Session);
        }
    }

    fn key_up(&self, keycode: u16, modifiers: &[Modifier]) {
        let flags = modifiers_to_flags(modifiers);
        if let Ok(event) = CGEvent::new_keyboard_event(source(), keycode, false) {
            event.set_flags(flags);
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

        let (x, y) = clamp_point(
            self.cursor_pos.x, self.cursor_pos.y,
            min_x, min_y, max_x, max_y,
        );
        self.cursor_pos.x = x;
        self.cursor_pos.y = y;
    }
}

impl EventEmitter for MouseEmitter {
    fn emit(&mut self, events: &[OutputEvent]) {
        // Cumulative delay for scheduling
        let mut accumulated_delay_ms: u64 = 0;

        for event in events {
            accumulated_delay_ms += event.delay_ms as u64;

            if accumulated_delay_ms == 0 {
                self.emit_single(&event.kind);
            } else {
                // Schedule via dispatch_after on the main queue
                let kind = event.kind.clone();
                let block = block2::RcBlock::new(move || {
                    emit_standalone(&kind);
                });
                unsafe {
                    let when = dispatch_time(
                        DISPATCH_TIME_NOW,
                        accumulated_delay_ms as i64 * NSEC_PER_MSEC,
                    );
                    let queue = &_dispatch_main_q as *const c_void;
                    dispatch_after(when, queue, &*block as *const _ as *const c_void);
                }
                // Keep the block alive until dispatch copies it
                std::mem::forget(block);
            }
        }
    }
}

impl MouseEmitter {
    fn emit_single(&mut self, kind: &OutputEventKind) {
        match kind {
            OutputEventKind::MoveCursor { dx, dy } => self.move_cursor(*dx, *dy),
            OutputEventKind::Scroll { dx, dy } => self.scroll(*dx, *dy),
            OutputEventKind::MouseDown(btn) => self.mouse_down(*btn),
            OutputEventKind::MouseUp(btn) => self.mouse_up(*btn),
            OutputEventKind::KeyDown { keycode, modifiers } => self.key_down(*keycode, modifiers),
            OutputEventKind::KeyUp { keycode, modifiers } => self.key_up(*keycode, modifiers),
        }
    }
}

/// Emit a single event without needing a MouseEmitter reference.
/// Used by dispatch_after blocks that can't capture &mut self.
/// Mouse position is read fresh from the system for each event.
fn emit_standalone(kind: &OutputEventKind) {
    match kind {
        OutputEventKind::MouseDown(btn) => post_mouse_standalone(*btn, true),
        OutputEventKind::MouseUp(btn) => post_mouse_standalone(*btn, false),
        OutputEventKind::KeyDown { keycode, modifiers } => {
            let flags = modifiers_to_flags(modifiers);
            if let Ok(event) = CGEvent::new_keyboard_event(source(), *keycode, true) {
                event.set_flags(flags);
                event.post(CGEventTapLocation::Session);
            }
        }
        OutputEventKind::KeyUp { keycode, modifiers } => {
            let flags = modifiers_to_flags(modifiers);
            if let Ok(event) = CGEvent::new_keyboard_event(source(), *keycode, false) {
                event.set_flags(flags);
                event.post(CGEventTapLocation::Session);
            }
        }
        // MoveCursor and Scroll shouldn't be delayed, but handle gracefully
        _ => {}
    }
}

fn post_mouse_standalone(button: MouseButtonKind, pressed: bool) {
    let pos = CGEvent::new(source())
        .map(|e| e.location())
        .unwrap_or(CGPoint::new(0.0, 0.0));

    let (event_type, cg_button, button_number) = match (button, pressed) {
        (MouseButtonKind::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left, None),
        (MouseButtonKind::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left, None),
        (MouseButtonKind::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right, None),
        (MouseButtonKind::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right, None),
        (MouseButtonKind::Middle, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(2)),
        (MouseButtonKind::Middle, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(2)),
        (MouseButtonKind::Back, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(3)),
        (MouseButtonKind::Back, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(3)),
        (MouseButtonKind::Forward, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(4)),
        (MouseButtonKind::Forward, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(4)),
    };

    if let Ok(event) = CGEvent::new_mouse_event(source(), event_type, pos, cg_button) {
        if let Some(num) = button_number {
            event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, num);
        }
        event.post(CGEventTapLocation::Session);
    }
}

/// Clamp a point to within display bounds, leaving 1px margin at max edges.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_emitter_constructs() {
        let emitter = MouseEmitter::new();
        assert!(emitter.last_click_time.is_empty());
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
}
