use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

use core_foundation::base::TCFType;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

use joyride_config::{apply_deadzone, Config};
use joyride_core::appwatcher::AppWatcher;
use joyride_core::gamepad::GamepadManager;
use joyride_core::mouse::{MouseButtonKind, MouseEmitter};
use joyride_core::settings::Settings;
use joyride_ui::statusbar::StatusBar;

// Raw libdispatch FFI for timer
extern "C" {
    fn dispatch_source_create(
        type_: *const c_void,
        handle: usize,
        mask: usize,
        queue: *const c_void,
    ) -> *mut c_void;
    fn dispatch_source_set_timer(source: *mut c_void, start: u64, interval: u64, leeway: u64);
    fn dispatch_source_set_event_handler_f(
        source: *mut c_void,
        handler: extern "C" fn(*mut c_void),
    );
    fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
    fn dispatch_resume(object: *mut c_void);
    static _dispatch_main_q: c_void;
    static _dispatch_source_type_timer: c_void;
}

const NSEC_PER_SEC: u64 = 1_000_000_000;
const DISPATCH_TIME_NOW: u64 = 0;

struct PollContext {
    settings: Rc<RefCell<Settings>>,
    gamepad: Rc<GamepadManager>,
    emitter: RefCell<MouseEmitter>,
    watcher: AppWatcher,
    statusbar: StatusBar,
    last_time: RefCell<Instant>,
}

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on main thread");

    let config = Config::from_args();
    let settings = Settings::new(config);

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    check_accessibility();

    let s = settings.borrow();
    let gamepad = GamepadManager::new(s.debug);
    gamepad.start();

    let emitter = MouseEmitter::new();
    let watcher = AppWatcher::new(&s.excluded_bundle_ids);
    watcher.start();

    let statusbar = StatusBar::new(mtm, &gamepad, &settings);

    eprintln!(
        "joyride: running (poll={}Hz, cursor={}, scroll={})",
        s.poll_hz as u32, s.cursor_speed, s.scroll_speed
    );
    if !s.excluded_bundle_ids.is_empty() {
        eprintln!(
            "joyride: excluded apps: {}",
            s.excluded_bundle_ids.join(", ")
        );
    }

    let interval_ns = (NSEC_PER_SEC as f64 / s.poll_hz) as u64;
    drop(s);

    let ctx = Box::new(PollContext {
        settings,
        gamepad,
        emitter: RefCell::new(emitter),
        watcher,
        statusbar,
        last_time: RefCell::new(Instant::now()),
    });
    let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

    unsafe {
        let queue = &_dispatch_main_q as *const _ as *const c_void;
        let timer = dispatch_source_create(
            &_dispatch_source_type_timer as *const _ as *const c_void,
            0,
            0,
            queue,
        );
        dispatch_source_set_timer(timer, DISPATCH_TIME_NOW, interval_ns, 0);
        dispatch_set_context(timer, ctx_ptr);
        dispatch_source_set_event_handler_f(timer, poll_callback);
        dispatch_resume(timer);
    }

    app.run();
}

extern "C" fn poll_callback(ctx_ptr: *mut c_void) {
    let ctx = unsafe { &*(ctx_ptr as *const PollContext) };

    if !ctx.statusbar.is_enabled() {
        return;
    }
    if ctx.watcher.is_excluded_active.get() {
        return;
    }

    let state = ctx.gamepad.state.borrow();
    if state.left_stick == (0.0, 0.0)
        && state.right_stick == (0.0, 0.0)
        && state.dpad == (0.0, 0.0)
        && state.pressed_buttons.is_empty()
    {
        return;
    }
    drop(state);

    let settings = ctx.settings.borrow();

    // Framerate-independent timing
    let now = Instant::now();
    let mut last = ctx.last_time.borrow_mut();
    let dt = now.duration_since(*last).as_secs_f64().min(0.1);
    *last = now;
    drop(last);

    let dz = settings.deadzone as f32;
    let state = ctx.gamepad.state.borrow();
    let mut emitter = ctx.emitter.borrow_mut();

    // Left stick: fast cursor movement
    let (lx, ly) = state.left_stick;
    if lx.abs() > dz || ly.abs() > dz {
        let x = apply_deadzone(lx, dz);
        let y = apply_deadzone(ly, dz);
        let dx = x as f64 * settings.cursor_speed * dt;
        let dy = -y as f64 * settings.cursor_speed * dt;
        emitter.move_cursor(dx, dy);
    }

    // D-pad: slow, precise cursor movement
    let (dpx, dpy) = state.dpad;
    if dpx.abs() > 0.1 || dpy.abs() > 0.1 {
        let dx = dpx as f64 * settings.dpad_speed * dt;
        let dy = -dpy as f64 * settings.dpad_speed * dt;
        emitter.move_cursor(dx, dy);
    }

    // Right stick: scroll
    let (rx, ry) = state.right_stick;
    if rx.abs() > dz || ry.abs() > dz {
        let x = apply_deadzone(rx, dz);
        let y = apply_deadzone(ry, dz);
        let scroll_dir: f64 = if settings.natural_scroll { -1.0 } else { 1.0 };
        let sdx = x as f64 * settings.scroll_speed;
        let sdy = y as f64 * settings.scroll_speed * scroll_dir;
        emitter.scroll(sdx, sdy);
    }

    // Buttons: dispatch based on mapping
    for (button_name, action) in &settings.button_map {
        let pressed = state.pressed_buttons.contains(button_name.as_str());
        let mouse_button = match action.as_str() {
            "leftClick" => Some(MouseButtonKind::Left),
            "rightClick" => Some(MouseButtonKind::Right),
            "middleClick" => Some(MouseButtonKind::Middle),
            "backClick" => Some(MouseButtonKind::Back),
            "forwardClick" => Some(MouseButtonKind::Forward),
            _ => None,
        };
        if let Some(mb) = mouse_button {
            emitter.update_button(mb, pressed);
        }
    }
}

fn check_accessibility() {
    let trusted = unsafe { accessibility_sys::AXIsProcessTrusted() };
    if trusted {
        return;
    }
    eprintln!("joyride: Accessibility permission not granted — requesting once");
    let key = core_foundation::string::CFString::new("AXTrustedCheckOptionPrompt");
    let value = core_foundation::boolean::CFBoolean::true_value();
    let opts = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[(
        key,
        value.as_CFType(),
    )]);
    unsafe {
        accessibility_sys::AXIsProcessTrustedWithOptions(
            opts.as_concrete_TypeRef() as *const _,
        );
    }
    // Wait briefly for the user to grant, then check again
    std::thread::sleep(std::time::Duration::from_secs(1));
    if unsafe { accessibility_sys::AXIsProcessTrusted() } {
        eprintln!("joyride: Accessibility permission granted");
    } else {
        eprintln!("joyride: Accessibility permission still pending — cursor control will not work until granted");
    }
}
