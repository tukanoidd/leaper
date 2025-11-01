{
  description = "Leaper";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    flake-utils,
    ...
  }:
    (flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain (p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);

        libs = with pkgs;
        with xorg; [
          vulkan-loader
          libGL

          wayland
          xorg.libX11

          libxkbcommon
        ];
        libsPath = pkgs.lib.makeLibraryPath libs;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;

          buildInputs = libs;
          passthru.runtimeLibsPath = libsPath;
        };

        leaper = craneLib.buildPackage (
          commonArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          }
        );
      in {
        checks = {
          inherit leaper;
        };

        packages.default = leaper;

        apps.default = flake-utils.lib.mkApp {
          drv = leaper;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          inputsFrom = [leaper];

          packages = with pkgs; [
            cargo-edit
            cargo-expand
            cargo-machete
            cargo-audit
            cargo-bloat
            cargo-features-manager
            cargo-modules

            surrealdb
          ];

          shellHook = ''
            export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${libsPath}";
          '';
        };
      }
    ))
    // {
      homeModules = {
        default = {
          config,
          pkgs,
          lib,
          ...
        }: let
          leaper-program = config.programs.leaper;
        in
          with lib; {
            options = {
              programs.leaper = {
                enable = mkEnableOption "leaper";
                package = mkOption {
                  description = "Package for Leaper";
                  example = false;
                  type = types.package;
                };
              };
            };
            config = {
              home = {
                packages = (
                  if leaper-program.enable
                  then [leaper-program.package]
                  else []
                );
              };
            };
          };
      };
    };
}
