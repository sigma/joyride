use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2_foundation::{NSString, NSUserDefaults};

use joyride_config::{Action, Config, InputId, Profile};

/// Runtime settings backed by NSUserDefaults.
/// Holds multiple named profiles and tracks the active one.
pub struct Settings {
    cli_defaults: Config,
    pub profiles: Vec<Profile>,
    active_profile: usize,
    /// When true, auto-switching is disabled and the default profile is forced.
    pub profile_locked: bool,
    pub debug: bool,
    pub excluded_bundle_ids: Vec<String>,
    /// Incremented on save/reset to signal cache invalidation.
    pub generation: u64,
    /// Cached mapping from bundle ID → profile index. Rebuilt on generation change.
    bundle_id_cache: HashMap<String, usize>,
}

impl Settings {
    pub fn new(cli: Config) -> Rc<RefCell<Self>> {
        let ud = NSUserDefaults::standardUserDefaults();
        let debug = ud_bool(&ud, "debugLogging").unwrap_or(cli.debug);

        // Load profiles from UserDefaults
        let mut profiles = load_profiles(&ud, &cli);
        if profiles.is_empty() {
            // Migration: load legacy single-profile settings into "Default"
            let default = load_legacy_profile(&ud, &cli);
            profiles.push(default);
        }

        let bundle_id_cache = build_bundle_id_cache(&profiles);
        let settings = Self {
            excluded_bundle_ids: cli.excluded_bundle_ids.clone(),
            profiles,
            active_profile: 0,
            profile_locked: false,
            debug,
            cli_defaults: cli,
            generation: 0,
            bundle_id_cache,
        };

        Rc::new(RefCell::new(settings))
    }

    pub fn active_profile_index(&self) -> usize {
        self.active_profile
    }

    /// Set the active profile index. Clamps to valid range.
    pub fn set_active_profile(&mut self, index: usize) {
        if !self.profiles.is_empty() {
            self.active_profile = index.min(self.profiles.len() - 1);
        }
    }

    pub fn active(&self) -> &Profile {
        &self.profiles[self.active_profile]
    }

    pub fn active_mut(&mut self) -> &mut Profile {
        &mut self.profiles[self.active_profile]
    }

    pub fn cursor_speed(&self) -> f64 { self.active().cursor_speed }
    pub fn dpad_speed(&self) -> f64 { self.active().dpad_speed }
    pub fn scroll_speed(&self) -> f64 { self.active().scroll_speed }
    pub fn deadzone(&self) -> f32 { self.active().deadzone }
    pub fn poll_hz(&self) -> f64 { self.active().poll_hz }
    pub fn natural_scroll(&self) -> bool { self.active().natural_scroll }
    pub fn button_map(&self) -> &HashMap<InputId, Action> { &self.active().button_map }

    pub fn poll_interval(&self) -> f64 {
        1.0 / self.poll_hz()
    }

    /// Rebuild internal caches after directly modifying profiles.
    pub fn rebuild_caches(&mut self) {
        self.bundle_id_cache = build_bundle_id_cache(&self.profiles);
    }

    /// Find the profile index matching a bundle ID, if any. O(1) via cache.
    pub fn profile_for_bundle_id(&self, bundle_id: &str) -> Option<usize> {
        self.bundle_id_cache.get(bundle_id).copied()
    }

    pub fn save(&mut self) {
        self.generation += 1;
        self.bundle_id_cache = build_bundle_id_cache(&self.profiles);
        let ud = NSUserDefaults::standardUserDefaults();
        ud.setBool_forKey(self.debug, &NSString::from_str("debugLogging"));
        save_profiles(&ud, &self.profiles);
    }

    pub fn reset_to_defaults(&mut self) {
        let ud = NSUserDefaults::standardUserDefaults();

        // Remove legacy keys
        for key in &["cursorSpeed", "dpadSpeed", "scrollSpeed", "deadzone",
                     "pollHz", "naturalScroll", "debugLogging"] {
            ud.removeObjectForKey(&NSString::from_str(key));
        }
        for &input in InputId::ALL {
            ud.removeObjectForKey(&NSString::from_str(&format!("mapping.{}", input.as_str())));
        }

        // Remove profile keys
        delete_all_profiles(&ud, &self.profiles);

        let default = Profile::from_config(&self.cli_defaults);
        self.profiles = vec![default];
        self.active_profile = 0;
        self.profile_locked = false;
        self.debug = self.cli_defaults.debug;
        self.generation += 1;

        self.save();
    }
}

fn build_bundle_id_cache(profiles: &[Profile]) -> HashMap<String, usize> {
    let mut cache = HashMap::new();
    for (idx, profile) in profiles.iter().enumerate() {
        for bid in &profile.bundle_ids {
            cache.insert(bid.clone(), idx);
        }
    }
    cache
}

// -- Profile persistence --

fn profile_key(name: &str, field: &str) -> String {
    format!("profile.{name}.{field}")
}

fn save_profiles(ud: &NSUserDefaults, profiles: &[Profile]) {
    // Store profile name list
    let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
    let names_str = names.join(",");
    unsafe {
        ud.setObject_forKey(
            Some(&NSString::from_str(&names_str)),
            &NSString::from_str("profileNames"),
        );
    }

    for p in profiles {
        let n = &p.name;
        ud.setDouble_forKey(p.cursor_speed, &NSString::from_str(&profile_key(n, "cursorSpeed")));
        ud.setDouble_forKey(p.dpad_speed, &NSString::from_str(&profile_key(n, "dpadSpeed")));
        ud.setDouble_forKey(p.scroll_speed, &NSString::from_str(&profile_key(n, "scrollSpeed")));
        ud.setDouble_forKey(p.deadzone as f64, &NSString::from_str(&profile_key(n, "deadzone")));
        ud.setDouble_forKey(p.poll_hz, &NSString::from_str(&profile_key(n, "pollHz")));
        ud.setBool_forKey(p.natural_scroll, &NSString::from_str(&profile_key(n, "naturalScroll")));

        // Bundle IDs
        let bids = p.bundle_ids.join(",");
        unsafe {
            ud.setObject_forKey(
                Some(&NSString::from_str(&bids)),
                &NSString::from_str(&profile_key(n, "bundleIds")),
            );
        }

        // Button mappings
        for (input, action) in &p.button_map {
            let key = profile_key(n, &format!("mapping.{}", input.as_str()));
            unsafe {
                ud.setObject_forKey(
                    Some(&NSString::from_str(&action.to_id())),
                    &NSString::from_str(&key),
                );
            }
        }
    }
}

fn load_profiles(ud: &NSUserDefaults, cli: &Config) -> Vec<Profile> {
    let names_str = match ud_string(ud, "profileNames") {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };

    names_str.split(',')
        .map(|name| load_profile(ud, name.trim(), cli))
        .collect()
}

fn load_profile(ud: &NSUserDefaults, name: &str, cli: &Config) -> Profile {
    let base = Profile::from_config(cli);
    let cursor_speed = ud_double(ud, &profile_key(name, "cursorSpeed")).unwrap_or(base.cursor_speed);
    let dpad_speed = ud_double(ud, &profile_key(name, "dpadSpeed")).unwrap_or(base.dpad_speed);
    let scroll_speed = ud_double(ud, &profile_key(name, "scrollSpeed")).unwrap_or(base.scroll_speed);
    let deadzone = ud_double(ud, &profile_key(name, "deadzone")).map(|v| v as f32).unwrap_or(base.deadzone);
    let poll_hz = ud_double(ud, &profile_key(name, "pollHz")).unwrap_or(base.poll_hz);
    let natural_scroll = ud_bool(ud, &profile_key(name, "naturalScroll")).unwrap_or(base.natural_scroll);

    let bundle_ids = ud_string(ud, &profile_key(name, "bundleIds"))
        .map(|s| s.split(',').filter(|b| !b.is_empty()).map(|b| b.to_string()).collect())
        .unwrap_or_default();

    let mut button_map = base.button_map;
    for &input in InputId::ALL {
        let key = profile_key(name, &format!("mapping.{}", input.as_str()));
        if let Some(s) = ud_string(ud, &key) {
            button_map.insert(input, Action::from_id(&s));
        }
    }

    Profile {
        name: name.to_string(),
        bundle_ids,
        cursor_speed,
        dpad_speed,
        scroll_speed,
        deadzone,
        poll_hz,
        natural_scroll,
        button_map,
    }
}

/// Load legacy (pre-profile) settings into a Default profile.
fn load_legacy_profile(ud: &NSUserDefaults, cli: &Config) -> Profile {
    let mut p = Profile::from_config(cli);
    p.cursor_speed = ud_double(ud, "cursorSpeed").unwrap_or(p.cursor_speed);
    p.dpad_speed = ud_double(ud, "dpadSpeed").unwrap_or(p.dpad_speed);
    p.scroll_speed = ud_double(ud, "scrollSpeed").unwrap_or(p.scroll_speed);
    p.deadzone = ud_double(ud, "deadzone").map(|v| v as f32).unwrap_or(p.deadzone);
    p.poll_hz = ud_double(ud, "pollHz").unwrap_or(p.poll_hz);
    p.natural_scroll = ud_bool(ud, "naturalScroll").unwrap_or(p.natural_scroll);

    for &input in InputId::ALL {
        let key = format!("mapping.{}", input.as_str());
        if let Some(s) = ud_string(ud, &key) {
            p.button_map.insert(input, Action::from_id(&s));
        }
    }
    p
}

fn delete_all_profiles(ud: &NSUserDefaults, profiles: &[Profile]) {
    ud.removeObjectForKey(&NSString::from_str("profileNames"));
    for p in profiles {
        let n = &p.name;
        for field in &["cursorSpeed", "dpadSpeed", "scrollSpeed", "deadzone",
                       "pollHz", "naturalScroll", "bundleIds"] {
            ud.removeObjectForKey(&NSString::from_str(&profile_key(n, field)));
        }
        for &input in InputId::ALL {
            ud.removeObjectForKey(&NSString::from_str(&profile_key(n, &format!("mapping.{}", input.as_str()))));
        }
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
        for &input_id in InputId::ALL {
            assert!(
                s.button_map().contains_key(&input_id),
                "missing input mapping: {input_id}"
            );
        }
    }

    #[test]
    fn default_button_map_cli_assignments() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        assert_eq!(*s.button_map().get(&InputId::ButtonA).unwrap(), Action::LeftClick);
        assert_eq!(*s.button_map().get(&InputId::ButtonB).unwrap(), Action::RightClick);
        assert_eq!(*s.button_map().get(&InputId::ButtonX).unwrap(), Action::MiddleClick);
        assert_eq!(*s.button_map().get(&InputId::ButtonY).unwrap(), Action::None);
    }

    #[test]
    fn reset_to_defaults_restores_values() {
        let settings = Settings::new(default_config());
        {
            let mut s = settings.borrow_mut();
            s.active_mut().cursor_speed = 9999.0;
            s.active_mut().natural_scroll = true;
            s.active_mut().button_map.insert(InputId::ButtonA, Action::None);
            s.reset_to_defaults();
        }
        let s = settings.borrow();
        assert_eq!(s.cursor_speed(), 1500.0);
        assert!(!s.natural_scroll());
        assert_eq!(*s.button_map().get(&InputId::ButtonA).unwrap(), Action::LeftClick);
    }

    #[test]
    fn save_does_not_panic() {
        let settings = Settings::new(default_config());
        settings.borrow_mut().save();
    }

    #[test]
    fn active_profile_is_default() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        assert_eq!(s.active().name, "Default");
        assert!(s.profiles.len() >= 1);
    }

    #[test]
    fn profile_from_config() {
        let config = default_config();
        let profile = Profile::from_config(&config);
        assert_eq!(profile.name, "Default");
        assert_eq!(profile.cursor_speed, 1500.0);
        assert!(profile.bundle_ids.is_empty());
        assert_eq!(*profile.button_map.get(&InputId::ButtonA).unwrap(), Action::LeftClick);
    }

    #[test]
    fn profile_for_bundle_id_not_found() {
        let settings = Settings::new(default_config());
        let s = settings.borrow();
        assert_eq!(s.profile_for_bundle_id("com.example.game"), None);
    }

    #[test]
    fn profile_for_bundle_id_found() {
        let settings = Settings::new(default_config());
        {
            let mut s = settings.borrow_mut();
            let mut gaming = Profile::from_config(&Config::parse(&[]).unwrap());
            gaming.name = "Gaming".to_string();
            gaming.bundle_ids = vec!["com.example.game".to_string()];
            s.profiles.push(gaming);
            s.rebuild_caches();
        }
        let s = settings.borrow();
        assert_eq!(s.profile_for_bundle_id("com.example.game"), Some(1));
        assert_eq!(s.profile_for_bundle_id("com.other"), None);
    }
}
