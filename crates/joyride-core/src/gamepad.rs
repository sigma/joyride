use std::cell::RefCell;
use std::ffi::c_float;
use std::ptr::NonNull;
use std::rc::Rc;

use joyride_config::InputId;
use log::{debug, info, warn};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{NSNotification, NSObjectProtocol};
use objc2_game_controller::{
    GCController, GCControllerButtonInput, GCControllerDidConnectNotification,
    GCControllerDirectionPad, GCDevice,
};

pub use joyride_config::GamepadState;

/// Manages GCController connections and input handlers.
/// Updates a shared [`GamepadState`] from controller events.
pub struct GamepadManager {
    pub state: Rc<RefCell<GamepadState>>,
    debug: bool,
    _observers: RefCell<Vec<Retained<objc2::runtime::ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl GamepadManager {
    pub fn new(debug: bool) -> Rc<Self> {
        Rc::new(Self {
            state: Rc::new(RefCell::new(GamepadState::default())),
            debug,
            _observers: RefCell::new(Vec::new()),
        })
    }

    pub fn start(self: &Rc<Self>) {
        unsafe {
            GCController::setShouldMonitorBackgroundEvents(true);
        }

        let this = Rc::clone(self);
        let connect_block = RcBlock::new(move |notif: NonNull<NSNotification>| {
            let notif = unsafe { notif.as_ref() };
            let controller = notif.object();
            if let Some(obj) = controller {
                let gc: &GCController =
                    unsafe { &*(Retained::as_ptr(&obj) as *const GCController) };
                this.attach_controller(gc);
            }
        });

        let center = objc2_foundation::NSNotificationCenter::defaultCenter();
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(GCControllerDidConnectNotification),
                None,
                None,
                &connect_block,
            )
        };
        self._observers.borrow_mut().push(observer);

        let completion = RcBlock::new(|| {});
        unsafe {
            GCController::startWirelessControllerDiscoveryWithCompletionHandler(Some(&completion));
        }

        let controllers = unsafe { GCController::controllers() };
        if !controllers.is_empty() {
            self.attach_controller(&controllers.objectAtIndex(0));
        }
    }

    fn attach_controller(&self, gc: &GCController) {
        let name = unsafe {
            gc.vendorName()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        };
        let category = unsafe { gc.productCategory().to_string() };
        info!("connected to {name} ({category})");

        self.setup_handlers(gc);
    }

    fn setup_handlers(&self, gc: &GCController) {
        let pad = unsafe { gc.extendedGamepad() };
        let Some(pad) = pad else {
            warn!("no extendedGamepad profile");
            return;
        };

        // Left thumbstick
        let state = Rc::clone(&self.state);
        let debug = self.debug;
        let handler = RcBlock::new(
            move |_: NonNull<GCControllerDirectionPad>, x: c_float, y: c_float| {
                state.borrow_mut().left_stick = (x, y);
                if debug {
                    debug!("L({x}, {y})");
                }
            },
        );
        unsafe {
            pad.leftThumbstick()
                .setValueChangedHandler(&*handler as *const _ as *mut _)
        };

        // Right thumbstick
        let state = Rc::clone(&self.state);
        let debug = self.debug;
        let handler = RcBlock::new(
            move |_: NonNull<GCControllerDirectionPad>, x: c_float, y: c_float| {
                state.borrow_mut().right_stick = (x, y);
                if debug {
                    debug!("R({x}, {y})");
                }
            },
        );
        unsafe {
            pad.rightThumbstick()
                .setValueChangedHandler(&*handler as *const _ as *mut _)
        };

        // D-pad: store analog values and emit discrete button events with hysteresis
        let state = Rc::clone(&self.state);
        let debug = self.debug;
        let dpad_active = Rc::new(RefCell::new(std::collections::HashSet::<InputId>::new()));
        let dpad_active_clone = Rc::clone(&dpad_active);
        let handler = RcBlock::new(
            move |_: NonNull<GCControllerDirectionPad>, x: c_float, y: c_float| {
                let mut s = state.borrow_mut();
                s.dpad = (x, y);
                let mut active = dpad_active_clone.borrow_mut();
                joyride_config::apply_dpad_hysteresis(x, y, &mut active, &mut s.pressed_buttons);
                if debug {
                    debug!("D({x}, {y})");
                }
            },
        );
        unsafe {
            pad.dpad()
                .setValueChangedHandler(&*handler as *const _ as *mut _)
        };

        // Buttons
        self.setup_button(InputId::ButtonA, unsafe { &pad.buttonA() });
        self.setup_button(InputId::ButtonB, unsafe { &pad.buttonB() });
        self.setup_button(InputId::ButtonX, unsafe { &pad.buttonX() });
        self.setup_button(InputId::ButtonY, unsafe { &pad.buttonY() });
        self.setup_button(InputId::LeftShoulder, unsafe { &pad.leftShoulder() });
        self.setup_button(InputId::RightShoulder, unsafe { &pad.rightShoulder() });
        self.setup_button(InputId::LeftTrigger, unsafe { &pad.leftTrigger() });
        self.setup_button(InputId::RightTrigger, unsafe { &pad.rightTrigger() });
        self.setup_button(InputId::ButtonMenu, unsafe { &pad.buttonMenu() });
        if let Some(opts) = unsafe { pad.buttonOptions() } {
            self.setup_button(InputId::ButtonOptions, &opts);
        }
    }

    fn setup_button(&self, id: InputId, button: &GCControllerButtonInput) {
        let state = Rc::clone(&self.state);
        let debug = self.debug;
        let handler = RcBlock::new(
            move |_: NonNull<GCControllerButtonInput>, _value: c_float, pressed: Bool| {
                let pressed = pressed.as_bool();
                let mut s = state.borrow_mut();
                if pressed {
                    s.pressed_buttons.insert(id);
                } else {
                    s.pressed_buttons.remove(&id);
                }
                if debug {
                    debug!("{} {}", id, if pressed { "down" } else { "up" });
                }
            },
        );
        unsafe {
            button.setPressedChangedHandler(&*handler as *const _ as *mut _);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamepad_state_default_zeroed() {
        let state = GamepadState::default();
        assert_eq!(state.left_stick, (0.0, 0.0));
        assert_eq!(state.right_stick, (0.0, 0.0));
        assert_eq!(state.dpad, (0.0, 0.0));
        assert!(state.pressed_buttons.is_empty());
    }

    #[test]
    fn gamepad_state_clone() {
        let mut state = GamepadState::default();
        state.left_stick = (0.5, -0.3);
        state.pressed_buttons.insert(InputId::ButtonA);
        let cloned = state.clone();
        assert_eq!(cloned.left_stick, (0.5, -0.3));
        assert!(cloned.pressed_buttons.contains(&InputId::ButtonA));
    }

    #[test]
    fn is_idle_default() {
        assert!(GamepadState::default().is_idle());
    }

    #[test]
    fn is_idle_with_stick() {
        let mut state = GamepadState::default();
        state.left_stick = (0.5, 0.0);
        assert!(!state.is_idle());
    }

    #[test]
    fn is_idle_with_button() {
        let mut state = GamepadState::default();
        state.pressed_buttons.insert(InputId::ButtonA);
        assert!(!state.is_idle());
    }

    #[test]
    fn gamepad_manager_constructs() {
        let manager = GamepadManager::new(false);
        let state = manager.state.borrow();
        assert_eq!(state.left_stick, (0.0, 0.0));
    }

    #[test]
    fn gamepad_manager_debug_mode() {
        let manager = GamepadManager::new(true);
        assert!(manager.debug);
    }
}
