# Joyride

A macOS daemon that turns game controllers into mouse input. Use your Xbox, PlayStation, or other standard gamepad to move the cursor, click, and scroll.

Joyride runs as a status bar app (no dock icon) and automatically pauses when excluded apps are in the foreground, so it stays out of the way during actual gaming.

## Features

- **Stick-to-cursor**: left stick for fast movement, D-pad for precise movement
- **Right stick scrolling** with optional natural scroll direction
- **Configurable button mapping**: assign any button to left/right/middle click, back, or forward
- **Per-app exclusions**: pause automatically for games or other apps that handle the controller directly
- **Native settings GUI**: adjust speeds, deadzones, and mappings live from the status bar
- **Settings persistence**: preferences stored in NSUserDefaults across restarts
- **Multi-monitor support**: cursor stays within screen bounds

## Requirements

- macOS (aarch64 or x86_64)
- Accessibility permission (prompted on first run)
- A game controller supported by Apple's Game Controller framework

## Installation

### With Nix (recommended)

Add the flake as an input and use the nix-darwin module:

```nix
# In your flake inputs:
joyride.url = "github:sigma/joyride";

# In your nix-darwin configuration:
{ joyride, ... }: {
  imports = [ joyride.darwinModules.default ];

  programs.joyride = {
    enable = true;
    user = "yourname";
    # Optional overrides:
    # cursorSpeed = 1500;
    # dpadSpeed = 150;
    # scrollSpeed = 8;
    # naturalScroll = false;
    # excludeApps = [ "com.example.game" ];
  };
}
```

### From source

```sh
cargo build --release
cp target/release/joyride /usr/local/bin/
```

The `Info.plist` must be available for the Game Controller framework to recognize the app. For a proper app bundle, the Nix build handles this automatically.

## Usage

```
joyride [OPTIONS]

Options:
  --cursor-speed <N>      Pixels/sec at full stick deflection (default: 1500)
  --dpad-speed <N>        D-pad pixels/sec (default: 150)
  --scroll-speed <N>      Scroll multiplier (default: 8)
  --poll-hz <N>           Polling rate, 30-240 Hz (default: 120)
  --deadzone <N>          Stick deadzone, 0.0-1.0 (default: 0.15)
  --left-click <BTN>      Button for left click (default: buttonA)
  --right-click <BTN>     Button for right click (default: buttonB)
  --middle-click <BTN>    Button for middle click (default: buttonX)
  --natural-scroll        Reverse scroll direction
  --exclude <IDS>         Comma-separated bundle IDs to exclude
  --debug                 Log to stderr
  -h, --help              Show help
```

Once running, use the status bar menu to toggle the daemon on/off or open the settings window.

## Default button mapping

| Button | Action      |
|--------|-------------|
| A      | Left click  |
| B      | Right click |
| X      | Middle click|

All buttons (A, B, X, Y, LB, RB, LT, RT, Menu, Options) can be remapped via CLI flags or the settings window.

## License

MIT
