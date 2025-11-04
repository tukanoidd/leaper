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

    tracy = {
      url = "github:tukanoidd/tracy.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
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

          nativeBuildInputs = with pkgs; [
            rustPlatform.bindgenHook
          ];

          buildInputs =
            libs;
          passthru.runtimeLibsPath = libsPath;
        };

        leaper = craneLib.buildPackage (
          commonArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;

            postFixup = ''
              patchelf $out/bin/leaper --add-rpath ${libsPath}
            '';

            NIX_OUTPATH_USED_AS_RANDOM_SEED = "__leaper__";
          }
        );
      in {
        checks = {
          inherit leaper;
        };

        packages = {
          inherit leaper;
          default = leaper;
        };

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

            inputs.tracy.packages.${system}.default
          ];

          hardeningDisable = ["fortify"];

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
