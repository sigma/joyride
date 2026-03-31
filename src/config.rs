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

        let mut i = 1;
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
                    eprintln!("Usage: joyride [options]");
                    eprintln!();
                    eprintln!("Options:");
                    eprintln!("  --exclude <ids>       Comma-separated bundle IDs");
                    eprintln!("  --cursor-speed <n>    Pixels/sec (default: 1500)");
                    eprintln!("  --dpad-speed <n>      D-pad pixels/sec (default: 150)");
                    eprintln!("  --scroll-speed <n>    Scroll multiplier (default: 8)");
                    eprintln!("  --poll-hz <n>         Polling rate (default: 120)");
                    eprintln!("  --deadzone <n>        Stick deadzone (default: 0.15)");
                    eprintln!("  --left-click <btn>    Left click button (default: buttonA)");
                    eprintln!("  --right-click <btn>   Right click button (default: buttonB)");
                    eprintln!("  --middle-click <btn>  Middle click button (default: buttonX)");
                    eprintln!("  --natural-scroll      Natural scroll direction");
                    eprintln!("  --debug               Debug logging");
                    std::process::exit(0);
                }
                other => {
                    eprintln!("Unknown option: {other}");
                    std::process::exit(1);
                }
            }
            i += 1;
        }
        config
    }
}
