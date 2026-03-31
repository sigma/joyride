{
  description = "Joyride - gamepad-to-mouse daemon for macOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "joyride";
            version = "0.1.0";

            src = ./.;

            cargoLock.lockFile = ./Cargo.lock;

            buildInputs = [
              pkgs.apple-sdk_15
            ];

            nativeBuildInputs = [
              pkgs.pkg-config
            ];

            postInstall = ''
              mkdir -p $out/Applications/joyride.app/Contents/MacOS
              mkdir -p $out/Applications/joyride.app/Contents/Resources
              cp $out/bin/joyride $out/Applications/joyride.app/Contents/MacOS/
              cp ${./Info.plist} $out/Applications/joyride.app/Contents/Info.plist
              cp ${./AppIcon.icns} $out/Applications/joyride.app/Contents/Resources/AppIcon.icns
            '';

            meta = {
              description = "Gamepad-to-mouse daemon for macOS using Game Controller framework";
              license = pkgs.lib.licenses.mit;
              platforms = pkgs.lib.platforms.darwin;
              mainProgram = "joyride";
            };
          };
        }
      );

      overlays.default = final: prev: {
        joyride = self.packages.${final.stdenv.system}.default;
      };

      darwinModules.default = import ./nix/darwin-module.nix { joyridePackages = self.packages; };
    };
}
