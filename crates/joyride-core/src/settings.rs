use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2_foundation::{NSString, NSUserDefaults};

use joyride_config::{Action, Config, Profile, ALL_INPUTS};

pub struct Settings {
    cli_defaults: Config,
    pub profiles: Vec<Profile>,
    pub active_profile: usize,
    pub debug: bool,
    pub excluded_bundle_ids: Vec<String>,
}

impl Settings {
    pub fn new(cli: Config) -> Rc<RefCell<Self>> {
        let ud = NSUserDefaults::standardUserDefaults();

        // Load default profile from UserDefaults, falling back to CLI
        let mut default_profile = Profile::from_config(&cli);
        default_profile.cursor_speed = ud_double(&ud, "cursorSpeed").unwrap_or(default_profile.cursor_speed);
        default_profile.dpad_speed = ud_double(&ud, "dpadSpeed").unwrap_or(default_profile.dpad_speed);
        default_profile.scroll_speed = ud_double(&ud, "scrollSpeed").unwrap_or(default_profile.scroll_speed);
        default_profile.deadzone = ud_double(&ud, "deadzone").unwrap_or(default_profile.deadzone);
        default_profile.poll_hz = ud_double(&ud, "pollHz").unwrap_or(default_profile.poll_hz);
        default_profile.natural_scroll = ud_bool(&ud, "naturalScroll").unwrap_or(default_profile.natural_scroll);

        // Load button mappings
        for (input, _) in ALL_INPUTS {
            let key = format!("mapping.{input}");
            if let Some(s) = ud_string(&ud, &key) {
                default_profile.button_map.insert(input.to_string(), Action::from_id(&s));
            }
        }

        let debug = ud_bool(&ud, "debugLogging").unwrap_or(cli.debug);

        let settings = Self {
            excluded_bundle_ids: cli.excluded_bundle_ids.clone(),
            profiles: vec![default_profile],
            active_profile: 0,
            debug,
            cli_defaults: cli,
        };

        Rc::new(RefCell::new(settings))
    }

    /// The currently active profile.
    pub fn active(&self) -> &Profile {
        &self.profiles[self.active_profile]
    }

    /// The currently active profile (mutable).
    pub fn active_mut(&mut self) -> &mut Profile {
        &mut self.profiles[self.active_profile]
    }

    // Convenience accessors delegating to active profile
    pub fn cursor_speed(&self) -> f64 { self.active().cursor_speed }
    pub fn dpad_speed(&self) -> f64 { self.active().dpad_speed }
    pub fn scroll_speed(&self) -> f64 { self.active().scroll_speed }
    pub fn deadzone(&self) -> f64 { self.active().deadzone }
    pub fn poll_hz(&self) -> f64 { self.active().poll_hz }
    pub fn natural_scroll(&self) -> bool { self.active().natural_scroll }
    pub fn button_map(&self) -> &HashMap<String, Action> { &self.active().button_map }

    pub fn poll_interval(&self) -> f64 {
        1.0 / self.poll_hz()
    }

    pub fn save(&self) {
        let ud = NSUserDefaults::standardUserDefaults();
        let p = self.active();
        ud.setDouble_forKey(p.cursor_speed, &NSString::from_str("cursorSpeed"));
        ud.setDouble_forKey(p.dpad_speed, &NSString::from_str("dpadSpeed"));
        ud.setDouble_forKey(p.scroll_speed, &NSString::from_str("scrollSpeed"));
        ud.setDouble_forKey(p.deadzone, &NSString::from_str("deadzone"));
        ud.setDouble_forKey(p.poll_hz, &NSString::from_str("pollHz"));
        ud.setBool_forKey(p.natural_scroll, &NSString::from_str("naturalScroll"));
        ud.setBool_forKey(self.debug, &NSString::from_str("debugLogging"));

        for (btn, action) in &p.button_map {
            let key = format!("mapping.{btn}");
            unsafe {
                ud.setObject_forKey(
                    Some(&NSString::from_str(action.to_id())),
                    &NSString::from_str(&key),
                );
            }
        }
    }

    pub fn reset_to_defaults(&mut self) {
        let ud = NSUserDefaults::standardUserDefaults();
        let keys = [
            "cursorSpeed", "dpadSpeed", "scrollSpeed", "deadzone",
            "pollHz", "naturalScroll", "debugLogging",
        ];
        for key in &keys {
            ud.removeObjectForKey(&NSString::from_str(key));
        }
        for (input, _) in ALL_INPUTS {
            ud.removeObjectForKey(&NSString::from_str(&format!("mapping.{input}")));
        }

        let default = Profile::from_config(&self.cli_defaults);
        self.profiles[0] = default;
        self.active_profile = 0;
        self.debug = self.cli_defaults.debug;
    }
}

fn ud_string(ud: &NSUserDefaults, key: &str) -> Option<String> {
    ud.stringForKey(&NSString::from_str(key)).map(|s| s.to_string())
}

fn ud_double(ud: &NSUserDefaults, key: &str) -> Option<f64> {
    let nskey = NSString::from_str(key);
    ud.objectForKey(&nskey).map(|_| ud.doubleForKey(&nskey))
}

fn ud_bool(ud: &NSUserDefaults, key: &str) -> Option<bool> {
    let nskey = NSString::from_str(key);
    ud.objectForKey(&nskey).map(|_| ud.boolForKey(&nskey))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> Config {
        Config::parse(&[]).unwrap()
    }

    #[test]
    fn new_returns_cli_defaults() {
        let config = default_config();
        let settings = Settings::new(config);
        let s = settings.borrow();
        assert_eq!(s.cursor_speed(), 1500.0);
        assert_eq!(s.dpad_speed(), 150.0);
        assert_eq!(s.scroll_speed(), 8.0);
        assert_eq!(s.poll_hz(), 120.0);
        assert!((s.deadzone() - 0.15).abs() < 1e-6);
        assert!(!s.natural_scroll());
        assert!(!s.debug);
    }

    #[test]
    fn poll_interval_calculation() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        let expected = 1.0 / 120.0;
        assert!((s.poll_interval() - expected).abs() < 1e-10);
    }

    #[test]
    fn default_button_map_has_all_inputs() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        for (input_id, _) in ALL_INPUTS {
            assert!(
                s.button_map().contains_key(*input_id),
                "missing input mapping: {input_id}"
            );
        }
    }

    #[test]
    fn default_button_map_cli_assignments() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        assert_eq!(*s.button_map().get("buttonA").unwrap(), Action::LeftClick);
        assert_eq!(*s.button_map().get("buttonB").unwrap(), Action::RightClick);
        assert_eq!(*s.button_map().get("buttonX").unwrap(), Action::MiddleClick);
        assert_eq!(*s.button_map().get("buttonY").unwrap(), Action::None);
    }

    #[test]
    fn reset_to_defaults_restores_values() {
        let settings = Settings::new(default_config());
        {
            let mut s = settings.borrow_mut();
            s.active_mut().cursor_speed = 9999.0;
            s.active_mut().natural_scroll = true;
            s.active_mut().button_map.insert("buttonA".to_string(), Action::None);
            s.reset_to_defaults();
        }
        let s = settings.borrow();
        assert_eq!(s.cursor_speed(), 1500.0);
        assert!(!s.natural_scroll());
        assert_eq!(*s.button_map().get("buttonA").unwrap(), Action::LeftClick);
    }

    #[test]
    fn save_does_not_panic() {
        let settings = Settings::new(default_config());
        settings.borrow().save();
    }

    #[test]
    fn active_profile_is_default() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        assert_eq!(s.active().name, "Default");
        assert_eq!(s.profiles.len(), 1);
    }

    #[test]
    fn profile_from_config() {
        let config = default_config();
        let profile = Profile::from_config(&config);
        assert_eq!(profile.name, "Default");
        assert_eq!(profile.cursor_speed, 1500.0);
        assert!(profile.bundle_ids.is_empty());
        assert_eq!(*profile.button_map.get("buttonA").unwrap(), Action::LeftClick);
    }
}
