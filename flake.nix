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

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };

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
    advisory-db,
    ...
  }:
    (flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        inherit (pkgs) lib;

        craneLib = (crane.mkLib pkgs).overrideToolchain (p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);
        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;

          buildInputs = with pkgs; [
            rustPlatform.bindgenHook
          ];
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        fileSetForCrate = crate:
          lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              (craneLib.fileset.commonCargoSources ./leaper-macros)
              (craneLib.fileset.commonCargoSources ./leaper-db)
              (craneLib.fileset.commonCargoSources ./leaper-mode)
              (craneLib.fileset.commonCargoSources ./leaper-launcher)
              (craneLib.fileset.commonCargoSources ./leaper-power)
              (craneLib.fileset.commonCargoSources ./leaper-runner)
              (craneLib.fileset.commonCargoSources ./leaper-executor)
              (craneLib.fileset.commonCargoSources ./leaper-style)
              (craneLib.fileset.commonCargoSources crate)
            ];
          };

        individualCrateArgs =
          commonArgs
          // {
            inherit cargoArtifacts;
            inherit (craneLib.crateNameFromCargoToml {inherit src;}) version;
            # doCheck = false;
          };

        libs = with pkgs;
        with xorg; [
          vulkan-loader
          libGL

          wayland
          xorg.libX11

          libxkbcommon
        ];
        libsPath = pkgs.lib.makeLibraryPath libs;

        leaper = craneLib.buildPackage (
          individualCrateArgs
          // {
            pname = "leaper";
            cargoExtraArgs = "-p leaper";
            src = fileSetForCrate ./leaper;

            buildInputs =
              libs;
            passthru.runtimeLibsPath = libsPath;

            postFixup = ''
              patchelf $out/bin/leaper --add-rpath ${libsPath}
            '';

            NIX_OUTPATH_USED_AS_RANDOM_SEED = "__leaper__";
          }
        );
        leaper-daemon = craneLib.buildPackage (individualCrateArgs
          // {
            pname = "leaper-daemon";
            cargoExtraArgs = "-p leaper-daemon";
            src = fileSetForCrate ./leaper-daemon;
          });
      in {
        checks = {
          inherit leaper;
          inherit leaper-daemon;

          leaper-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });
          leaper-fmt = craneLib.cargoFmt (commonArgs
            // {
              cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            });
          leaper-toml-fmt = craneLib.taploFmt {
            src = pkgs.lib.sources.sourceFilesBySuffices src [".toml"];
          };
          leaper-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };
          leaper-deny = craneLib.cargoDeny {
            inherit src;
          };

          # Later...
          # leaper-doc = craneLib.cargoDoc (
          #   commonArgs
          #   // {
          #     inherit cargoArtifacts;
          #     env.RUSTDOCFLAGS = "--deny warnings";
          #   }
          # );
          # leaper-nextest = craneLib.cargoNextest (
          #   commonArgs
          #   // {
          #     inherit cargoArtifacts;
          #     partitions = 1;
          #     partitionType = "count";
          #     cargoNextestPartitionsExtraArgs = "--no-tests=pass";
          #   }
          # );
        };

        packages = {
          inherit leaper;
          inherit leaper-daemon;
          default = leaper;
        };

        apps = rec {
          leaper = flake-utils.lib.mkApp {
            drv = leaper;
          };
          leaper-daemon = flake-utils.lib.mkApp {
            drv = leaper-daemon;
          };
          default = leaper;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          inputsFrom = [leaper leaper-daemon];

          packages = with pkgs; [
            # Workspace Management
            cargo-edit
            cargo-features-manager

            # Audit
            cargo-audit

            # Misc
            cargo-modules
            cargo-expand

            # Clean up unused stuff
            cargo-machete
            cargo-udeps
            cargo-bloat

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
