use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, DeclaredClass, MainThreadOnly};
use objc2_app_kit::*;
use objc2_foundation::*;

use joyride_config::{format_value, Action, ALL_ACTIONS, ALL_INPUTS};
use joyride_core::settings::Settings;

// MARK: - Window delegate

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "JoyrideWindowDelegate"]
    #[ivars = ()]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            // Hide the app from the dock/cmd-tab but don't change activation policy.
            // Toggling Accessory↔Regular repeatedly can trigger spurious system prompts.
            app.hide(None);
        }
    }
);

impl WindowDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

// MARK: - Action targets

struct SliderIvars {
    settings: Rc<RefCell<Settings>>,
    field: &'static str,
    label: RefCell<Option<Retained<NSTextField>>>,
    format: &'static str,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "JoyrideSliderTarget"]
    #[ivars = SliderIvars]
    struct SliderTarget;

    impl SliderTarget {
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: &NSSlider) {
            let value = sender.doubleValue();
            let iv = self.ivars();
            let mut s = iv.settings.borrow_mut();
            match iv.field {
                "cursorSpeed" => s.cursor_speed = value,
                "dpadSpeed" => s.dpad_speed = value,
                "scrollSpeed" => s.scroll_speed = value,
                "deadzone" => s.deadzone = value,
                "pollHz" => s.poll_hz = value,
                _ => {}
            }
            s.save();
            if let Some(label) = iv.label.borrow().as_ref() {
                label.setStringValue(&NSString::from_str(&format_value(value, iv.format)));
            }
        }
    }
);

struct ToggleIvars {
    settings: Rc<RefCell<Settings>>,
    field: &'static str,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "JoyrideToggleTarget"]
    #[ivars = ToggleIvars]
    struct ToggleTarget;

    impl ToggleTarget {
        #[unsafe(method(toggleChanged:))]
        fn toggle_changed(&self, sender: &NSSwitch) {
            let on = sender.state() == 1;
            let iv = self.ivars();
            let mut s = iv.settings.borrow_mut();
            match iv.field {
                "naturalScroll" => s.natural_scroll = on,
                "debugLogging" => s.debug = on,
                _ => {}
            }
            s.save();
        }
    }
);

struct MappingIvars {
    settings: Rc<RefCell<Settings>>,
    button_name: String,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "JoyrideMappingTarget"]
    #[ivars = MappingIvars]
    struct MappingTarget;

    impl MappingTarget {
        #[unsafe(method(mappingChanged:))]
        fn mapping_changed(&self, sender: &NSPopUpButton) {
            let iv = self.ivars();
            let idx = sender.indexOfSelectedItem();
            if idx >= 0 && (idx as usize) < ALL_ACTIONS.len() {
                let action = Action::from_id(ALL_ACTIONS[idx as usize].0);
                let mut s = iv.settings.borrow_mut();
                s.button_map.insert(iv.button_name.clone(), action);
                s.save();
            }
        }
    }
);

struct ResetIvars {
    settings: Rc<RefCell<Settings>>,
    refresh: RefCell<Option<Box<dyn Fn()>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "JoyrideResetTarget"]
    #[ivars = ResetIvars]
    struct ResetTarget;

    impl ResetTarget {
        #[unsafe(method(resetDefaults:))]
        fn reset_defaults(&self, _sender: &NSObject) {
            self.ivars().settings.borrow_mut().reset_to_defaults();
            if let Some(refresh) = self.ivars().refresh.borrow().as_ref() {
                refresh();
            }
            eprintln!("joyride: settings reset to defaults");
        }
    }
);

// MARK: - Tracked controls for refresh

struct TrackedSlider {
    slider: Retained<NSSlider>,
    label: Retained<NSTextField>,
    field: &'static str,
    format: &'static str,
}

struct TrackedToggle {
    switch: Retained<NSSwitch>,
    field: &'static str,
}

struct TrackedMapping {
    popup: Retained<NSPopUpButton>,
    button_id: String,
}

// MARK: - Settings window

pub struct SettingsWindow {
    window: Option<Retained<NSWindow>>,
    _retained: Vec<Retained<NSObject>>,
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self {
            window: None,
            _retained: Vec::new(),
        }
    }

    pub fn show(&mut self, settings: &Rc<RefCell<Settings>>) {
        let mtm = MainThreadMarker::new().unwrap();

        if let Some(ref window) = self.window {
            if window.isVisible() {
                window.makeKeyAndOrderFront(None);
                let app = NSApplication::sharedApplication(mtm);
                app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
                app.activate();
                return;
            }
            self.window = None;
            self._retained.clear();
        }

        let mut retained: Vec<Retained<NSObject>> = Vec::new();
        let mut sliders: Vec<TrackedSlider> = Vec::new();
        let mut toggles: Vec<TrackedToggle> = Vec::new();
        let mut mappings: Vec<TrackedMapping> = Vec::new();

        let s = settings.borrow();

        let stack = NSStackView::stackViewWithViews(&NSArray::new(), mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(8.0);
        stack.setEdgeInsets(NSEdgeInsets {
            top: 20.0, left: 20.0, bottom: 20.0, right: 20.0,
        });

        // -- Cursor Control --
        add_header(&stack, "Cursor Control", mtm);
        let (t, ts) = add_slider(&stack, "Left Stick Speed", settings, "cursorSpeed", s.cursor_speed, 100.0, 5000.0, "int", mtm);
        retained.push(t); sliders.push(ts);
        let (t, ts) = add_slider(&stack, "D-pad Speed", settings, "dpadSpeed", s.dpad_speed, 10.0, 500.0, "int", mtm);
        retained.push(t); sliders.push(ts);
        let (t, ts) = add_slider(&stack, "Deadzone", settings, "deadzone", s.deadzone, 0.0, 0.5, "f2", mtm);
        retained.push(t); sliders.push(ts);

        // -- Scrolling --
        add_spacer(&stack, 8.0);
        add_header(&stack, "Scrolling", mtm);
        let (t, ts) = add_slider(&stack, "Scroll Speed", settings, "scrollSpeed", s.scroll_speed, 1.0, 30.0, "int", mtm);
        retained.push(t); sliders.push(ts);
        let (t, tt) = add_toggle(&stack, "Natural Scrolling", settings, "naturalScroll", s.natural_scroll, mtm);
        retained.push(t); toggles.push(tt);

        // -- Button Mapping --
        add_spacer(&stack, 8.0);
        add_header(&stack, "Button Mapping", mtm);
        for (btn_id, btn_display) in ALL_INPUTS {
            let current = s.button_map.get(*btn_id).map(|a| a.to_id()).unwrap_or("none");
            let (t, tm) = add_mapping(&stack, btn_display, settings, btn_id, current, mtm);
            retained.push(t); mappings.push(tm);
        }

        // -- Advanced --
        add_spacer(&stack, 8.0);
        add_header(&stack, "Advanced", mtm);
        let (t, ts) = add_slider(&stack, "Poll Rate", settings, "pollHz", s.poll_hz, 30.0, 240.0, "hz", mtm);
        retained.push(t); sliders.push(ts);
        let (t, tt) = add_toggle(&stack, "Debug Logging", settings, "debugLogging", s.debug, mtm);
        retained.push(t); toggles.push(tt);

        // -- Reset button --
        add_spacer(&stack, 12.0);
        let reset_target = mtm.alloc::<ResetTarget>().set_ivars(ResetIvars {
            settings: Rc::clone(settings),
            refresh: RefCell::new(None),
        });
        let reset_target: Retained<ResetTarget> = unsafe { msg_send![super(reset_target), init] };
        let reset_btn = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Reset to Defaults"),
                Some(&reset_target),
                Some(sel!(resetDefaults:)),
                mtm,
            )
        };
        stack.addArrangedSubview(&reset_btn);

        // Set up refresh callback
        let settings_for_refresh = Rc::clone(settings);
        *reset_target.ivars().refresh.borrow_mut() = Some(Box::new(move || {
            let s = settings_for_refresh.borrow();
            for ts in &sliders {
                let value = match ts.field {
                    "cursorSpeed" => s.cursor_speed,
                    "dpadSpeed" => s.dpad_speed,
                    "scrollSpeed" => s.scroll_speed,
                    "deadzone" => s.deadzone,
                    "pollHz" => s.poll_hz,
                    _ => 0.0,
                };
                ts.slider.setDoubleValue(value);
                ts.label.setStringValue(&NSString::from_str(&format_value(value, ts.format)));
            }
            for tt in &toggles {
                let on = match tt.field {
                    "naturalScroll" => s.natural_scroll,
                    "debugLogging" => s.debug,
                    _ => false,
                };
                tt.switch.setState(if on { 1 } else { 0 });
            }
            for tm in &mappings {
                let action = s.button_map.get(&tm.button_id).map(|a| a.to_id()).unwrap_or("none");
                let idx = ALL_ACTIONS.iter()
                    .position(|(id, _)| *id == action)
                    .unwrap_or(0);
                tm.popup.selectItemAtIndex(idx as isize);
            }
        }));

        retained.push(Retained::into_super(reset_target));
        drop(s);

        // Layout
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);

        let style = NSWindowStyleMask::Titled
            .union(NSWindowStyleMask::Closable)
            .union(NSWindowStyleMask::Miniaturizable);
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 520.0));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(), frame, style, NSBackingStoreType::Buffered, false,
            )
        };
        window.setTitle(&NSString::from_str("joyride Settings"));

        // Use a scroll view for the content
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&stack));
        scroll.setHasVerticalScroller(true);
        window.setContentView(Some(&scroll));
        window.center();

        if let Some(content_view) = window.contentView() {
            stack.setTranslatesAutoresizingMaskIntoConstraints(false);
            let lc = stack.leadingAnchor().constraintEqualToAnchor(&content_view.leadingAnchor());
            lc.setActive(true);
            let tc = stack.trailingAnchor().constraintEqualToAnchor(&content_view.trailingAnchor());
            tc.setActive(true);
            let top = stack.topAnchor().constraintEqualToAnchor(&content_view.topAnchor());
            top.setActive(true);

            window.setInitialFirstResponder(Some(&content_view));
        }

        let delegate = WindowDelegate::new(mtm);
        window.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)));
        retained.push(Retained::into_super(delegate));

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        window.makeKeyAndOrderFront(None);
        app.activate();

        self.window = Some(window);
        self._retained = retained;
    }
}

// MARK: - Helpers

const LABEL_WIDTH: f64 = 130.0;
const VALUE_WIDTH: f64 = 70.0;

fn add_header(stack: &NSStackView, title: &str, mtm: MainThreadMarker) {
    let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    label.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    stack.addArrangedSubview(&label);
}

fn add_spacer(stack: &NSStackView, height: f64) {
    let mtm = MainThreadMarker::new().unwrap();
    let spacer = NSView::new(mtm);
    let hc = spacer.heightAnchor().constraintEqualToConstant(height);
    hc.setActive(true);
    stack.addArrangedSubview(&spacer);
}

fn add_slider(
    stack: &NSStackView, title: &str, settings: &Rc<RefCell<Settings>>,
    field: &'static str, value: f64, min: f64, max: f64, format: &'static str,
    mtm: MainThreadMarker,
) -> (Retained<NSObject>, TrackedSlider) {
    let value_label =
        NSTextField::labelWithString(&NSString::from_str(&format_value(value, format)), mtm);
    value_label.setSelectable(false);
    value_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, 0.0)));
    let vc = value_label.widthAnchor().constraintEqualToConstant(VALUE_WIDTH);
    vc.setActive(true);
    value_label.setAlignment(NSTextAlignment::Right);

    let target = mtm.alloc::<SliderTarget>().set_ivars(SliderIvars {
        settings: Rc::clone(settings), field,
        label: RefCell::new(Some(value_label.clone())), format,
    });
    let target: Retained<SliderTarget> = unsafe { msg_send![super(target), init] };

    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            value, min, max, Some(&target), Some(sel!(sliderChanged:)), mtm,
        )
    };
    slider.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);

    let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    let lc = label.widthAnchor().constraintEqualToConstant(LABEL_WIDTH);
    lc.setActive(true);

    let row = NSStackView::stackViewWithViews(
        &NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(label)),
            Retained::into_super(Retained::into_super(slider.clone())),
            Retained::into_super(Retained::into_super(value_label.clone())),
        ]),
        mtm,
    );
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setSpacing(8.0);
    row.setDistribution(NSStackViewDistribution::Fill);
    let wc: &NSView = &row;
    let wc = wc.widthAnchor().constraintGreaterThanOrEqualToConstant(400.0);
    wc.setActive(true);
    stack.addArrangedSubview(&row);

    let tracked = TrackedSlider { slider, label: value_label, field, format };
    (Retained::into_super(target), tracked)
}

fn add_toggle(
    stack: &NSStackView, title: &str, settings: &Rc<RefCell<Settings>>,
    field: &'static str, value: bool, mtm: MainThreadMarker,
) -> (Retained<NSObject>, TrackedToggle) {
    let target = mtm.alloc::<ToggleTarget>().set_ivars(ToggleIvars {
        settings: Rc::clone(settings), field,
    });
    let target: Retained<ToggleTarget> = unsafe { msg_send![super(target), init] };

    let switch = NSSwitch::new(mtm);
    switch.setState(if value { 1 } else { 0 });
    unsafe {
        switch.setTarget(Some(&target));
        switch.setAction(Some(sel!(toggleChanged:)));
    }

    let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    let lc = label.widthAnchor().constraintEqualToConstant(LABEL_WIDTH);
    lc.setActive(true);

    let row = NSStackView::stackViewWithViews(
        &NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(label)),
            Retained::into_super(Retained::into_super(switch.clone())),
        ]),
        mtm,
    );
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setSpacing(8.0);
    stack.addArrangedSubview(&row);

    let tracked = TrackedToggle { switch, field };
    (Retained::into_super(target), tracked)
}

fn add_mapping(
    stack: &NSStackView, button_display: &str, settings: &Rc<RefCell<Settings>>,
    button_id: &str, current_action: &str, mtm: MainThreadMarker,
) -> (Retained<NSObject>, TrackedMapping) {
    let target = mtm.alloc::<MappingTarget>().set_ivars(MappingIvars {
        settings: Rc::clone(settings),
        button_name: button_id.to_string(),
    });
    let target: Retained<MappingTarget> = unsafe { msg_send![super(target), init] };

    let popup = NSPopUpButton::initWithFrame_pullsDown(mtm.alloc(), NSRect::ZERO, false);
    let mut selected_idx: isize = 0;
    for (i, (action_id, action_display)) in ALL_ACTIONS.iter().enumerate() {
        popup.addItemWithTitle(&NSString::from_str(action_display));
        if *action_id == current_action {
            selected_idx = i as isize;
        }
    }
    popup.selectItemAtIndex(selected_idx);
    unsafe {
        popup.setTarget(Some(&target));
        popup.setAction(Some(sel!(mappingChanged:)));
    }

    let label = NSTextField::labelWithString(&NSString::from_str(button_display), mtm);
    let lc = label.widthAnchor().constraintEqualToConstant(LABEL_WIDTH);
    lc.setActive(true);

    let row = NSStackView::stackViewWithViews(
        &NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(label)),
            Retained::into_super(Retained::into_super(Retained::into_super(popup.clone()))),
        ]),
        mtm,
    );
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setSpacing(8.0);
    stack.addArrangedSubview(&row);

    let tracked = TrackedMapping { popup, button_id: button_id.to_string() };
    (Retained::into_super(target), tracked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_window_new_has_no_window() {
        let sw = SettingsWindow::new();
        assert!(sw.window.is_none());
        assert!(sw._retained.is_empty());
    }
}
