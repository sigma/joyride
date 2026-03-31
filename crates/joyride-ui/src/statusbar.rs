use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, DeclaredClass};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSString};

use log::info;

use joyride_core::gamepad::GamepadManager;
use joyride_core::settings::Settings;
use crate::settings_window::SettingsWindow;

struct MenuTargetIvars {
    enabled: Rc<Cell<bool>>,
    enabled_item: RefCell<Option<Retained<NSMenuItem>>>,
    status_button: RefCell<Option<Retained<NSStatusBarButton>>>,
    settings: Rc<RefCell<Settings>>,
    settings_window: RefCell<SettingsWindow>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "JoyrideMenuTarget"]
    #[ivars = MenuTargetIvars]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: &NSObject) {
            let iv = self.ivars();
            let was = iv.enabled.get();
            iv.enabled.set(!was);
            let now = !was;

            if let Some(item) = iv.enabled_item.borrow().as_ref() {
                item.setState(if now { 1 } else { 0 });
            }
            if let Some(button) = iv.status_button.borrow().as_ref() {
                let name = if now { "gamecontroller.fill" } else { "gamecontroller" };
                let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                    &NSString::from_str(name),
                    Some(&NSString::from_str("joyride")),
                );
                if let Some(image) = image {
                    button.setImage(Some(&image));
                }
            }
            info!("{}", if now { "enabled" } else { "disabled" });
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &NSObject) {
            self.ivars()
                .settings_window
                .borrow_mut()
                .show(&self.ivars().settings);
        }
    }
);

pub struct StatusBar {
    _status_item: Retained<NSStatusItem>,
    _menu_target: Retained<MenuTarget>,
    enabled: Rc<Cell<bool>>,
}

impl StatusBar {
    pub fn new(
        mtm: MainThreadMarker,
        _gamepad: &Rc<GamepadManager>,
        settings: &Rc<RefCell<Settings>>,
    ) -> Self {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
        let button = status_item.button(mtm);

        if let Some(ref button) = button {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("gamecontroller.fill"),
                Some(&NSString::from_str("joyride")),
            );
            if let Some(image) = image {
                button.setImage(Some(&image));
            }
        }

        let enabled = Rc::new(Cell::new(true));

        let menu_target = mtm.alloc::<MenuTarget>().set_ivars(MenuTargetIvars {
            enabled: Rc::clone(&enabled),
            enabled_item: RefCell::new(None),
            status_button: RefCell::new(button),
            settings: Rc::clone(settings),
            settings_window: RefCell::new(SettingsWindow::new()),
        });
        let menu_target: Retained<MenuTarget> = unsafe { msg_send![super(menu_target), init] };

        let menu = NSMenu::new(mtm);

        let info_item = NSMenuItem::new(mtm);
        let version = env!("CARGO_PKG_VERSION");
        info_item.setTitle(&NSString::from_str(&format!("joyride v{version}")));
        info_item.setEnabled(false);
        menu.addItem(&info_item);
        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let enabled_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Enabled"),
                Some(sel!(toggleEnabled:)),
                &NSString::from_str(""),
            )
        };
        enabled_item.setState(1);
        unsafe { enabled_item.setTarget(Some(&menu_target)) };
        menu.addItem(&enabled_item);
        *menu_target.ivars().enabled_item.borrow_mut() = Some(enabled_item);

        let settings_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Settings..."),
                Some(sel!(openSettings:)),
                &NSString::from_str(","),
            )
        };
        unsafe { settings_item.setTarget(Some(&menu_target)) };
        menu.addItem(&settings_item);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let quit_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Quit"),
                Some(sel!(terminate:)),
                &NSString::from_str("q"),
            )
        };
        menu.addItem(&quit_item);

        status_item.setMenu(Some(&menu));

        Self {
            _status_item: status_item,
            _menu_target: menu_target,
            enabled,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }
}
