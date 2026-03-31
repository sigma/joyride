# Joyride

macOS gamepad-to-mouse daemon written in Rust. Converts game controller input into cursor movement, clicks, and scrolling via macOS frameworks (GameController, CoreGraphics, AppKit).

## Build & Run

```sh
cargo build           # debug build
cargo build --release # release build
cargo run             # run directly
```

Platform: macOS only (uses Objective-C bindings via objc2).

## Architecture

- `main.rs` — app lifecycle, polling loop via libdispatch timer
- `gamepad.rs` — GCController input handling
- `mouse.rs` — CGEvent-based mouse emission
- `config.rs` — CLI argument parsing
- `settings.rs` — runtime settings + NSUserDefaults persistence
- `settings_window.rs` — native macOS settings GUI (AppKit via objc2)
- `statusbar.rs` — status bar menu
- `appwatcher.rs` — active app monitoring for exclusion list

All UI must run on the main thread (MainThreadMarker).

## Nix

- `flake.nix` — builds the app bundle for aarch64-darwin and x86_64-darwin
- `nix/darwin-module.nix` — nix-darwin module with launchd agent

## Version Control

This project uses [jj](https://martinvonz.github.io/jj/) for version control.

## Work Management

This project tracks work with `bw` (beadwork), which persists to git — plans, progress, and decisions survive compaction, session boundaries, and context loss.

ALWAYS run `bw prime` before starting work. Without it, you're missing workflow context, current state, and repo hygiene warnings. Work done without priming often conflicts with in-progress changes.

Committing, closing issues, and syncing are part of completing a task — not separate actions requiring additional permission.
