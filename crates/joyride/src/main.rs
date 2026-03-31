use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

use core_foundation::base::TCFType;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

use log::{info, warn};

use joyride_config::{Config, EventEmitter};
use joyride_core::appwatcher::AppWatcher;
use joyride_core::gamepad::GamepadManager;
use joyride_core::mouse::MouseEmitter;
use joyride_core::settings::Settings;
use joyride_core::translator::{InputTranslator, TranslatorConfig};
use joyride_ui::statusbar::StatusBar;

use objc2_app_kit::{NSImage};
use objc2_foundation::NSString;

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
    translator: RefCell<InputTranslator>,
    emitter: RefCell<MouseEmitter>,
    watcher: AppWatcher,
    statusbar: StatusBar,
    last_time: RefCell<Instant>,
    lock_combo_was_held: RefCell<bool>,
    cached_config: RefCell<Option<TranslatorConfig>>,
    cached_profile_idx: RefCell<usize>,
    cached_generation: RefCell<u64>,
}

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on main thread");

    let config = Config::from_args();

    // Init logger: --debug → Debug level, otherwise Info
    let level = if config.debug { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format_timestamp_millis()
        .init();

    let settings = Settings::new(config);

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Set app icon from SF Symbol so it appears in Accessibility prefs and dock
    if let Some(icon) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("gamecontroller.fill"),
        Some(&NSString::from_str("joyride")),
    ) {
        unsafe { app.setApplicationIconImage(Some(&icon)) };
    }

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
        translator: RefCell::new(InputTranslator::new()),
        emitter: RefCell::new(emitter),
        watcher,
        statusbar,
        last_time: RefCell::new(Instant::now()),
        lock_combo_was_held: RefCell::new(false),
        cached_config: RefCell::new(None),
        cached_profile_idx: RefCell::new(usize::MAX),
        cached_generation: RefCell::new(u64::MAX),
    });
    // Intentional leak: PollContext lives for the entire process lifetime.
    // The dispatch timer holds a raw pointer to it; there's no cleanup path
    // because NSApplication::run() never returns.
    let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

    unsafe {
        let queue = &_dispatch_main_q as *const c_void;
        let timer = dispatch_source_create(
            &_dispatch_source_type_timer as *const c_void,
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
        let combo_held = state.pressed_buttons.contains(&joyride_config::InputId::ButtonMenu)
            && state.pressed_buttons.contains(&joyride_config::InputId::ButtonOptions);
        let mut was_held = ctx.lock_combo_was_held.borrow_mut();
        if combo_held && !*was_held {
            let mut settings = ctx.settings.borrow_mut();
            settings.profile_locked = !settings.profile_locked;
            if settings.profile_locked {
                settings.set_active_profile(0);
                info!("profile locked to Default");
            } else {
                info!("profile auto-switching re-enabled");
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
            if settings.active_profile_index() != target {
                let name = settings.profiles[target].name.clone();
                info!("switched to profile '{name}'");
                settings.set_active_profile(target);
            }
        }
    }

    // Early-return if gamepad is idle and translator has no pending button state
    let state = ctx.gamepad.state.borrow();
    if state.is_idle() && !ctx.translator.borrow().has_buttons_pressed() {
        return;
    }
    drop(state);

    // Compute dt
    let now = Instant::now();
    let mut last = ctx.last_time.borrow_mut();
    let dt = now.duration_since(*last).as_secs_f64().min(0.1);
    *last = now;
    drop(last);

    // Rebuild config snapshot only when the active profile changes
    {
        let settings = ctx.settings.borrow();
        let current_idx = settings.active_profile_index();
        let current_gen = settings.generation;
        let mut cached_idx = ctx.cached_profile_idx.borrow_mut();
        let mut cached_gen = ctx.cached_generation.borrow_mut();
        if *cached_idx != current_idx || *cached_gen != current_gen || ctx.cached_config.borrow().is_none() {
            *cached_idx = current_idx;
            *cached_gen = current_gen;
            let new_config = TranslatorConfig {
                cursor_speed: settings.cursor_speed(),
                dpad_speed: settings.dpad_speed(),
                scroll_speed: settings.scroll_speed(),
                deadzone: settings.deadzone(),
                natural_scroll: settings.natural_scroll(),
                button_map: settings.button_map().clone(),
            };
            // Flush buttons that are no longer mapped in the new config
            let flush_events = ctx.translator.borrow_mut().flush_stale_buttons(&new_config);
            if !flush_events.is_empty() {
                ctx.emitter.borrow_mut().emit(&flush_events);
            }
            *ctx.cached_config.borrow_mut() = Some(new_config);
        }
    }

    // Translate input to output events
    let config = ctx.cached_config.borrow();
    let config = config.as_ref().unwrap();
    let state = ctx.gamepad.state.borrow();
    let events = ctx.translator.borrow_mut().translate(&state, config, dt);
    drop(state);

    // Emit events to the OS
    ctx.emitter.borrow_mut().emit(&events);
}

fn check_accessibility() {
    let trusted = unsafe { accessibility_sys::AXIsProcessTrusted() };
    if trusted {
        return;
    }
    warn!("Accessibility permission not granted — requesting once");
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
    std::thread::sleep(std::time::Duration::from_secs(1));
    if unsafe { accessibility_sys::AXIsProcessTrusted() } {
        info!("Accessibility permission granted");
    } else {
        warn!("Accessibility permission still pending — cursor control will not work until granted");
    }
}
