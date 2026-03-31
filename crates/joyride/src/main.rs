use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

use core_foundation::base::TCFType;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

use joyride_config::{apply_deadzone, Action, Config};
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
    /// Tracks whether Menu+Options was held last frame (for edge detection).
    lock_combo_was_held: RefCell<bool>,
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
    let watcher = AppWatcher::new();
    watcher.start();

    let statusbar = StatusBar::new(mtm, &gamepad, &settings);

    eprintln!(
        "joyride: running (poll={}Hz, cursor={}, scroll={})",
        s.poll_hz() as u32, s.cursor_speed(), s.scroll_speed()
    );
    if !s.excluded_bundle_ids.is_empty() {
        eprintln!(
            "joyride: excluded apps: {}",
            s.excluded_bundle_ids.join(", ")
        );
    }

    let interval_ns = (NSEC_PER_SEC as f64 / s.poll_hz()) as u64;
    drop(s);

    let ctx = Box::new(PollContext {
        settings,
        gamepad,
        emitter: RefCell::new(emitter),
        watcher,
        statusbar,
        last_time: RefCell::new(Instant::now()),
        lock_combo_was_held: RefCell::new(false),
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

    // Detect Menu+Options combo for profile lock toggle
    {
        let state = ctx.gamepad.state.borrow();
        let combo_held = state.pressed_buttons.contains("buttonMenu")
            && state.pressed_buttons.contains("buttonOptions");
        let mut was_held = ctx.lock_combo_was_held.borrow_mut();
        if combo_held && !*was_held {
            let mut settings = ctx.settings.borrow_mut();
            settings.profile_locked = !settings.profile_locked;
            if settings.profile_locked {
                settings.active_profile = 0;
                eprintln!("joyride: profile locked to Default");
            } else {
                eprintln!("joyride: profile auto-switching re-enabled");
            }
        }
        *was_held = combo_held;
    }

    // Switch active profile based on frontmost app (unless locked)
    {
        let bundle_id = ctx.watcher.frontmost_bundle_id.borrow();
        let mut settings = ctx.settings.borrow_mut();
        let excluded = settings.excluded_bundle_ids.contains(&*bundle_id);
        if excluded {
            return;
        }
        if !settings.profile_locked {
            let target = settings.profile_for_bundle_id(&bundle_id).unwrap_or(0);
            if settings.active_profile != target {
                let name = settings.profiles[target].name.clone();
                eprintln!("joyride: switched to profile '{name}'");
                settings.active_profile = target;
            }
        }
    }

    let state = ctx.gamepad.state.borrow();
    if state.is_idle() && !ctx.emitter.borrow().has_buttons_pressed() {
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

    let dz = settings.deadzone() as f32;
    let bmap = settings.button_map();
    let state = ctx.gamepad.state.borrow();
    let mut emitter = ctx.emitter.borrow_mut();

    // Left stick: fast cursor movement
    let (lx, ly) = state.left_stick;
    if lx.abs() > dz || ly.abs() > dz {
        let x = apply_deadzone(lx, dz);
        let y = apply_deadzone(ly, dz);
        let dx = x as f64 * settings.cursor_speed() * dt;
        let dy = -y as f64 * settings.cursor_speed() * dt;
        emitter.move_cursor(dx, dy);
    }

    // D-pad: slow, precise cursor movement (only for unmapped directions)
    let (dpx, dpy) = state.dpad;
    let dpad_x_mapped = !matches!(bmap.get("dpadLeft"), Some(Action::None) | None)
        || !matches!(bmap.get("dpadRight"), Some(Action::None) | None);
    let dpad_y_mapped = !matches!(bmap.get("dpadUp"), Some(Action::None) | None)
        || !matches!(bmap.get("dpadDown"), Some(Action::None) | None);
    let use_dpx = if dpad_x_mapped { 0.0 } else { dpx };
    let use_dpy = if dpad_y_mapped { 0.0 } else { dpy };
    if use_dpx.abs() > 0.1 || use_dpy.abs() > 0.1 {
        let dx = use_dpx as f64 * settings.dpad_speed() * dt;
        let dy = -use_dpy as f64 * settings.dpad_speed() * dt;
        emitter.move_cursor(dx, dy);
    }

    // Right stick: scroll
    let (rx, ry) = state.right_stick;
    if rx.abs() > dz || ry.abs() > dz {
        let x = apply_deadzone(rx, dz);
        let y = apply_deadzone(ry, dz);
        let scroll_dir: f64 = if settings.natural_scroll() { -1.0 } else { 1.0 };
        let sdx = x as f64 * settings.scroll_speed();
        let sdy = y as f64 * settings.scroll_speed() * scroll_dir;
        emitter.scroll(sdx, sdy);
    }

    // Buttons: dispatch based on mapping
    for (button_name, action) in bmap {
        let pressed = state.pressed_buttons.contains(button_name.as_str());
        match action {
            Action::None => {}
            Action::LeftClick => emitter.update_button(MouseButtonKind::Left, pressed),
            Action::RightClick => emitter.update_button(MouseButtonKind::Right, pressed),
            Action::MiddleClick => emitter.update_button(MouseButtonKind::Middle, pressed),
            Action::BackClick => emitter.update_button(MouseButtonKind::Back, pressed),
            Action::ForwardClick => emitter.update_button(MouseButtonKind::Forward, pressed),
            Action::DoubleLeftClick => {
                if pressed {
                    emitter.double_click(MouseButtonKind::Left);
                } else {
                    emitter.reset_double_click(MouseButtonKind::Left);
                }
            }
            Action::DoubleRightClick => {
                if pressed {
                    emitter.double_click(MouseButtonKind::Right);
                } else {
                    emitter.reset_double_click(MouseButtonKind::Right);
                }
            }
            Action::KeyPress(combo) => {
                if pressed {
                    emitter.key_press(combo);
                }
            }
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
