use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2_foundation::{NSString, NSUserDefaults};

use joyride_config::{Config, ALL_BUTTONS};

pub struct Settings {
    cli_defaults: Config,
    pub cursor_speed: f64,
    pub dpad_speed: f64,
    pub scroll_speed: f64,
    pub deadzone: f64,
    pub poll_hz: f64,
    pub natural_scroll: bool,
    pub debug: bool,
    pub excluded_bundle_ids: Vec<String>,
    /// Maps gamepad button name → action name (e.g. "buttonA" → "leftClick")
    pub button_map: HashMap<String, String>,
}

impl Settings {
    pub fn new(cli: Config) -> Rc<RefCell<Self>> {
        let ud = NSUserDefaults::standardUserDefaults();

        let cursor_speed = ud_double(&ud, "cursorSpeed").unwrap_or(cli.cursor_speed);
        let dpad_speed = ud_double(&ud, "dpadSpeed").unwrap_or(cli.dpad_speed);
        let scroll_speed = ud_double(&ud, "scrollSpeed").unwrap_or(cli.scroll_speed);
        let deadzone = ud_double(&ud, "deadzone").unwrap_or(cli.deadzone as f64);
        let poll_hz = ud_double(&ud, "pollHz").unwrap_or(cli.poll_hz);
        let natural_scroll = ud_bool(&ud, "naturalScroll").unwrap_or(cli.natural_scroll);
        let debug = ud_bool(&ud, "debugLogging").unwrap_or(cli.debug);

        // Build button map: load from UserDefaults per-button, fall back to CLI defaults
        let mut button_map = HashMap::new();
        let cli_defaults_map = cli.cli_button_map();
        for (btn, _) in ALL_BUTTONS {
            let key = format!("mapping.{btn}");
            let action = ud_string(&ud, &key)
                .unwrap_or_else(|| cli_defaults_map.get(*btn).cloned().unwrap_or("none".into()));
            button_map.insert(btn.to_string(), action);
        }

        let settings = Self {
            excluded_bundle_ids: cli.excluded_bundle_ids.clone(),
            cli_defaults: cli,
            cursor_speed,
            dpad_speed,
            scroll_speed,
            deadzone,
            poll_hz,
            natural_scroll,
            debug,
            button_map,
        };

        Rc::new(RefCell::new(settings))
    }

    pub fn poll_interval(&self) -> f64 {
        1.0 / self.poll_hz
    }

    pub fn save(&self) {
        let ud = NSUserDefaults::standardUserDefaults();
        ud.setDouble_forKey(self.cursor_speed, &NSString::from_str("cursorSpeed"));
        ud.setDouble_forKey(self.dpad_speed, &NSString::from_str("dpadSpeed"));
        ud.setDouble_forKey(self.scroll_speed, &NSString::from_str("scrollSpeed"));
        ud.setDouble_forKey(self.deadzone, &NSString::from_str("deadzone"));
        ud.setDouble_forKey(self.poll_hz, &NSString::from_str("pollHz"));
        ud.setBool_forKey(self.natural_scroll, &NSString::from_str("naturalScroll"));
        ud.setBool_forKey(self.debug, &NSString::from_str("debugLogging"));

        for (btn, action) in &self.button_map {
            let key = format!("mapping.{btn}");
            unsafe {
                ud.setObject_forKey(
                    Some(&NSString::from_str(action)),
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
        for (btn, _) in ALL_BUTTONS {
            ud.removeObjectForKey(&NSString::from_str(&format!("mapping.{btn}")));
        }

        self.cursor_speed = self.cli_defaults.cursor_speed;
        self.dpad_speed = self.cli_defaults.dpad_speed;
        self.scroll_speed = self.cli_defaults.scroll_speed;
        self.deadzone = self.cli_defaults.deadzone as f64;
        self.poll_hz = self.cli_defaults.poll_hz;
        self.natural_scroll = self.cli_defaults.natural_scroll;
        self.debug = self.cli_defaults.debug;
        self.button_map = self.cli_defaults.cli_button_map();
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
        assert_eq!(s.cursor_speed, 1500.0);
        assert_eq!(s.dpad_speed, 150.0);
        assert_eq!(s.scroll_speed, 8.0);
        assert_eq!(s.poll_hz, 120.0);
        assert!((s.deadzone - 0.15).abs() < 1e-6);
        assert!(!s.natural_scroll);
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
    fn default_button_map_has_all_buttons() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        for (btn_id, _) in ALL_BUTTONS {
            assert!(
                s.button_map.contains_key(*btn_id),
                "missing button mapping: {btn_id}"
            );
        }
    }

    #[test]
    fn default_button_map_cli_assignments() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        assert_eq!(s.button_map.get("buttonA").unwrap(), "leftClick");
        assert_eq!(s.button_map.get("buttonB").unwrap(), "rightClick");
        assert_eq!(s.button_map.get("buttonX").unwrap(), "middleClick");
        // Unassigned buttons default to "none"
        assert_eq!(s.button_map.get("buttonY").unwrap(), "none");
    }

    #[test]
    fn reset_to_defaults_restores_values() {
        let settings = Settings::new(default_config());
        {
            let mut s = settings.borrow_mut();
            s.cursor_speed = 9999.0;
            s.natural_scroll = true;
            s.button_map.insert("buttonA".to_string(), "none".to_string());
            s.reset_to_defaults();
        }
        let s = settings.borrow();
        assert_eq!(s.cursor_speed, 1500.0);
        assert!(!s.natural_scroll);
        assert_eq!(s.button_map.get("buttonA").unwrap(), "leftClick");
    }

    #[test]
    fn save_does_not_panic() {
        let settings = Settings::new(default_config());
        settings.borrow().save();
    }
}
