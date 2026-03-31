use std::collections::HashMap;

pub const ALL_BUTTONS: &[(&str, &str)] = &[
    ("buttonA", "A"),
    ("buttonB", "B"),
    ("buttonX", "X"),
    ("buttonY", "Y"),
    ("leftShoulder", "LB"),
    ("rightShoulder", "RB"),
    ("leftTrigger", "LT"),
    ("rightTrigger", "RT"),
    ("buttonMenu", "Menu"),
    ("buttonOptions", "Options"),
];

pub const ALL_ACTIONS: &[(&str, &str)] = &[
    ("none", "None"),
    ("leftClick", "Left Click"),
    ("rightClick", "Right Click"),
    ("middleClick", "Middle Click"),
    ("backClick", "Back"),
    ("forwardClick", "Forward"),
];

pub struct Config {
    pub excluded_bundle_ids: Vec<String>,
    pub cursor_speed: f64,
    pub dpad_speed: f64,
    pub scroll_speed: f64,
    pub poll_hz: f64,
    pub deadzone: f32,
    pub left_click: String,
    pub right_click: String,
    pub middle_click: String,
    pub natural_scroll: bool,
    pub debug: bool,
}

impl Config {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        match Self::parse(&args[1..]) {
            Ok(config) => config,
            Err(ParseOutcome::Help) => {
                std::process::exit(0);
            }
            Err(ParseOutcome::Error(msg)) => {
                eprintln!("{msg}");
                std::process::exit(1);
            }
        }
    }

    pub fn parse(args: &[String]) -> Result<Self, ParseOutcome> {
        let mut config = Config {
            excluded_bundle_ids: Vec::new(),
            cursor_speed: 1500.0,
            dpad_speed: 150.0,
            scroll_speed: 8.0,
            poll_hz: 120.0,
            deadzone: 0.15,
            left_click: "buttonA".into(),
            right_click: "buttonB".into(),
            middle_click: "buttonX".into(),
            natural_scroll: false,
            debug: false,
        };

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--exclude" => {
                    i += 1;
                    if i < args.len() {
                        config.excluded_bundle_ids = args[i]
                            .split(',')
                            .map(|s| s.to_string())
                            .collect();
                    }
                }
                "--cursor-speed" => {
                    i += 1;
                    if i < args.len() {
                        config.cursor_speed = args[i].parse().unwrap_or(config.cursor_speed);
                    }
                }
                "--dpad-speed" => {
                    i += 1;
                    if i < args.len() {
                        config.dpad_speed = args[i].parse().unwrap_or(config.dpad_speed);
                    }
                }
                "--scroll-speed" => {
                    i += 1;
                    if i < args.len() {
                        config.scroll_speed = args[i].parse().unwrap_or(config.scroll_speed);
                    }
                }
                "--poll-hz" => {
                    i += 1;
                    if i < args.len() {
                        config.poll_hz = args[i].parse().unwrap_or(config.poll_hz);
                    }
                }
                "--deadzone" => {
                    i += 1;
                    if i < args.len() {
                        config.deadzone = args[i].parse().unwrap_or(config.deadzone);
                    }
                }
                "--left-click" => {
                    i += 1;
                    if i < args.len() {
                        config.left_click = args[i].clone();
                    }
                }
                "--right-click" => {
                    i += 1;
                    if i < args.len() {
                        config.right_click = args[i].clone();
                    }
                }
                "--middle-click" => {
                    i += 1;
                    if i < args.len() {
                        config.middle_click = args[i].clone();
                    }
                }
                "--natural-scroll" => config.natural_scroll = true,
                "--debug" => config.debug = true,
                "--help" | "-h" => {
                    return Err(ParseOutcome::Help);
                }
                other => {
                    return Err(ParseOutcome::Error(format!("Unknown option: {other}")));
                }
            }
            i += 1;
        }
        Ok(config)
    }

    pub fn cli_button_map(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(self.left_click.clone(), "leftClick".into());
        m.insert(self.right_click.clone(), "rightClick".into());
        m.insert(self.middle_click.clone(), "middleClick".into());
        m
    }
}

#[derive(Debug)]
pub enum ParseOutcome {
    Help,
    Error(String),
}

pub fn apply_deadzone(value: f32, dz: f32) -> f32 {
    if value.abs() <= dz {
        return 0.0;
    }
    let sign = if value > 0.0 { 1.0 } else { -1.0 };
    sign * (value.abs() - dz) / (1.0 - dz)
}

pub fn format_value(value: f64, fmt: &str) -> String {
    match fmt {
        "int" => format!("{}", value as i64),
        "hz" => format!("{} Hz", value as i64),
        "f2" => format!("{:.2}", value),
        _ => format!("{}", value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_values() {
        let config = Config::parse(&[]).unwrap();
        assert_eq!(config.cursor_speed, 1500.0);
        assert_eq!(config.dpad_speed, 150.0);
        assert_eq!(config.scroll_speed, 8.0);
        assert_eq!(config.poll_hz, 120.0);
        assert_eq!(config.deadzone, 0.15);
        assert_eq!(config.left_click, "buttonA");
        assert_eq!(config.right_click, "buttonB");
        assert_eq!(config.middle_click, "buttonX");
        assert!(!config.natural_scroll);
        assert!(!config.debug);
        assert!(config.excluded_bundle_ids.is_empty());
    }

    #[test]
    fn cursor_speed_flag() {
        let config = Config::parse(&args(&["--cursor-speed", "2000"])).unwrap();
        assert_eq!(config.cursor_speed, 2000.0);
    }

    #[test]
    fn dpad_speed_flag() {
        let config = Config::parse(&args(&["--dpad-speed", "300"])).unwrap();
        assert_eq!(config.dpad_speed, 300.0);
    }

    #[test]
    fn scroll_speed_flag() {
        let config = Config::parse(&args(&["--scroll-speed", "15"])).unwrap();
        assert_eq!(config.scroll_speed, 15.0);
    }

    #[test]
    fn poll_hz_flag() {
        let config = Config::parse(&args(&["--poll-hz", "60"])).unwrap();
        assert_eq!(config.poll_hz, 60.0);
    }

    #[test]
    fn deadzone_flag() {
        let config = Config::parse(&args(&["--deadzone", "0.25"])).unwrap();
        assert_eq!(config.deadzone, 0.25);
    }

    #[test]
    fn button_assignment_flags() {
        let config = Config::parse(&args(&[
            "--left-click", "buttonY",
            "--right-click", "leftShoulder",
            "--middle-click", "rightShoulder",
        ])).unwrap();
        assert_eq!(config.left_click, "buttonY");
        assert_eq!(config.right_click, "leftShoulder");
        assert_eq!(config.middle_click, "rightShoulder");
    }

    #[test]
    fn boolean_flags() {
        let config = Config::parse(&args(&["--natural-scroll", "--debug"])).unwrap();
        assert!(config.natural_scroll);
        assert!(config.debug);
    }

    #[test]
    fn exclude_multiple() {
        let config = Config::parse(&args(&["--exclude", "com.foo,com.bar"])).unwrap();
        assert_eq!(config.excluded_bundle_ids, vec!["com.foo", "com.bar"]);
    }

    #[test]
    fn exclude_single() {
        let config = Config::parse(&args(&["--exclude", "com.example"])).unwrap();
        assert_eq!(config.excluded_bundle_ids, vec!["com.example"]);
    }

    #[test]
    fn invalid_numeric_falls_back() {
        let config = Config::parse(&args(&["--cursor-speed", "notanumber"])).unwrap();
        assert_eq!(config.cursor_speed, 1500.0);
    }

    #[test]
    fn multiple_flags_combined() {
        let config = Config::parse(&args(&[
            "--cursor-speed", "3000",
            "--poll-hz", "60",
            "--debug",
        ])).unwrap();
        assert_eq!(config.cursor_speed, 3000.0);
        assert_eq!(config.poll_hz, 60.0);
        assert!(config.debug);
    }

    #[test]
    fn help_flag() {
        let result = Config::parse(&args(&["--help"]));
        assert!(matches!(result, Err(ParseOutcome::Help)));
    }

    #[test]
    fn unknown_flag() {
        let result = Config::parse(&args(&["--bogus"]));
        assert!(matches!(result, Err(ParseOutcome::Error(_))));
    }

    #[test]
    fn all_buttons_no_duplicate_ids() {
        let mut seen = std::collections::HashSet::new();
        for (id, _) in ALL_BUTTONS {
            assert!(seen.insert(id), "duplicate button ID: {id}");
        }
    }

    #[test]
    fn all_actions_no_duplicate_ids() {
        let mut seen = std::collections::HashSet::new();
        for (id, _) in ALL_ACTIONS {
            assert!(seen.insert(id), "duplicate action ID: {id}");
        }
    }

    #[test]
    fn cli_button_map_defaults() {
        let config = Config::parse(&[]).unwrap();
        let map = config.cli_button_map();
        assert_eq!(map.get("buttonA").unwrap(), "leftClick");
        assert_eq!(map.get("buttonB").unwrap(), "rightClick");
        assert_eq!(map.get("buttonX").unwrap(), "middleClick");
    }

    #[test]
    fn cli_button_map_overridden() {
        let config = Config::parse(&args(&["--left-click", "buttonY"])).unwrap();
        let map = config.cli_button_map();
        assert_eq!(map.get("buttonY").unwrap(), "leftClick");
        assert!(!map.contains_key("buttonA"));
    }

    #[test]
    fn apply_deadzone_within_zone() {
        assert_eq!(apply_deadzone(0.1, 0.15), 0.0);
        assert_eq!(apply_deadzone(-0.1, 0.15), 0.0);
        assert_eq!(apply_deadzone(0.0, 0.15), 0.0);
    }

    #[test]
    fn apply_deadzone_at_boundary() {
        assert_eq!(apply_deadzone(0.15, 0.15), 0.0);
        assert_eq!(apply_deadzone(-0.15, 0.15), 0.0);
    }

    #[test]
    fn apply_deadzone_extremes() {
        let result = apply_deadzone(1.0, 0.15);
        assert!((result - 1.0).abs() < 1e-6);
        let result = apply_deadzone(-1.0, 0.15);
        assert!((result + 1.0).abs() < 1e-6);
    }

    #[test]
    fn apply_deadzone_zero_deadzone() {
        assert_eq!(apply_deadzone(0.5, 0.0), 0.5);
        assert_eq!(apply_deadzone(-0.5, 0.0), -0.5);
    }

    #[test]
    fn apply_deadzone_mid_range() {
        let result = apply_deadzone(0.5, 0.15);
        let expected = (0.5 - 0.15) / (1.0 - 0.15);
        assert!((result - expected).abs() < 1e-6);
    }

    #[test]
    fn format_value_int() {
        assert_eq!(format_value(1500.0, "int"), "1500");
        assert_eq!(format_value(1500.7, "int"), "1500");
    }

    #[test]
    fn format_value_hz() {
        assert_eq!(format_value(120.0, "hz"), "120 Hz");
    }

    #[test]
    fn format_value_f2() {
        assert_eq!(format_value(0.15, "f2"), "0.15");
        assert_eq!(format_value(1.0, "f2"), "1.00");
    }

    #[test]
    fn format_value_fallback() {
        let result = format_value(42.5, "unknown");
        assert_eq!(result, "42.5");
    }
}
