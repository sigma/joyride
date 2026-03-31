{ joyridePackages }:
{
  config,
  lib,
  pkgs,
  ...
}:
with lib;
let
  cfg = config.programs.joyride;
  appName = "joyride";
  appDir = "/Users/${cfg.user}/Applications/${appName}.app";
  storePkg = joyridePackages.${pkgs.stdenv.system}.default;
in
{
  options.programs.joyride = {
    enable = mkEnableOption "joyride gamepad-to-mouse daemon";

    user = mkOption {
      type = types.str;
      description = "Username for installing the app bundle to ~/Applications";
    };

    excludeApps = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Bundle IDs where joyride is disabled (gamepad passthrough)";
    };

    cursorSpeed = mkOption {
      type = types.int;
      default = 1500;
      description = "Cursor speed in pixels/sec at full stick deflection";
    };

    dpadSpeed = mkOption {
      type = types.int;
      default = 150;
      description = "D-pad cursor speed in pixels/sec (precise movement)";
    };

    scrollSpeed = mkOption {
      type = types.int;
      default = 8;
      description = "Scroll speed multiplier";
    };

    naturalScroll = mkOption {
      type = types.bool;
      default = false;
      description = "Use natural scrolling direction";
    };
  };

  config = mkIf cfg.enable {
    # Copy app bundle to ~/Applications and sign for stable TCC permissions.
    system.activationScripts.postActivation.text = ''
      echo >&2 "installing ${appName}.app..."

      # Stop the running instance before replacing the binary
      launchctl bootout "gui/$(id -u ${cfg.user})/org.nixos.${appName}" 2>/dev/null || true

      mkdir -p "${appDir}/Contents/MacOS"
      mkdir -p "${appDir}/Contents/Resources"
      cp "${storePkg}/Applications/${appName}.app/Contents/MacOS/${appName}" "${appDir}/Contents/MacOS/"
      cp "${storePkg}/Applications/${appName}.app/Contents/Info.plist" "${appDir}/Contents/"
      cp "${storePkg}/Applications/${appName}.app/Contents/Resources/AppIcon.icns" "${appDir}/Contents/Resources/"
      /usr/bin/codesign --force --sign - --identifier dev.${appName} "${appDir}"

      # Reset accessibility TCC entry so the new binary is recognized.
      # The user will get a one-time prompt on next launch.
      tccutil reset Accessibility dev.${appName} 2>/dev/null || true

      # Restart the service with the new binary
      launchctl bootstrap "gui/$(id -u ${cfg.user})" /Library/LaunchAgents/org.nixos.${appName}.plist 2>/dev/null || true
    '';

    launchd.user.agents.joyride = {
      serviceConfig = {
        ProgramArguments =
          [
            "${appDir}/Contents/MacOS/${appName}"
          ]
          ++ optionals (cfg.excludeApps != [ ]) [
            "--exclude"
            (concatStringsSep "," cfg.excludeApps)
          ]
          ++ [
            "--cursor-speed"
            (toString cfg.cursorSpeed)
            "--dpad-speed"
            (toString cfg.dpadSpeed)
            "--scroll-speed"
            (toString cfg.scrollSpeed)
          ]
          ++ optionals cfg.naturalScroll [
            "--natural-scroll"
          ];
        KeepAlive = {
          SuccessfulExit = false;
        };
        RunAtLoad = true;
        StandardOutPath = "/tmp/joyride.log";
        StandardErrorPath = "/tmp/joyride.log";
      };
    };
  };
}
