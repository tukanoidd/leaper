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

    surrealdb = {
      url = "github:surrealdb/surrealdb?tag=v3.0.0-alpha.17";
      inputs.nixpkgs.follows = "nixpkgs";
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
          inherit src;
          strictDeps = true;

          buildInputs = with pkgs; [
            rustPlatform.bindgenHook
          ];

          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        fileSet = lib.fileset.toSource {
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
            (craneLib.fileset.commonCargoSources ./leaper-lock)
            (craneLib.fileset.commonCargoSources ./leaper-executor)
            (craneLib.fileset.commonCargoSources ./leaper-style)
            (craneLib.fileset.commonCargoSources ./leaper-tracing)
            (craneLib.fileset.commonCargoSources ./leaper)
            (craneLib.fileset.commonCargoSources ./leaper-daemon)
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
            src = fileSet;

            buildInputs =
              libs
              ++ (with pkgs; [
                linux-pam
              ]);
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
            src = fileSet;
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

          # Later...
          # leaper-deny = craneLib.cargoDeny {
          #   inherit src;
          # };
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
        };

        apps = {
          leaper = flake-utils.lib.mkApp {
            drv = leaper;
          };
          leaper-daemon = flake-utils.lib.mkApp {
            drv = leaper-daemon;
          };
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

            inputs.surrealdb.packages.${system}.default

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
      nixosModules = {
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
                daemon-package = mkOption {
                  description = "Package for Leaper Daemon";
                  example = false;
                  type = types.package;
                };
                db = {
                  host = mkOption {
                    description = "SurrealDB host";
                    example = "127.0.0.1";
                    default = "127.0.0.1";
                    type = types.str;
                  };
                  port = mkOption {
                    description = "SurrealDB instance port";
                    example = 8000;
                    default = 8000;
                    type = types.port;
                  };
                  path = mkOption {
                    description = "SurrealDB path";
                    example = "memory";
                    default = "rocksdb:/var/lib/surrealdb";
                    type = types.str;
                  };
                  extraFlags = mkOption {
                    description = "SurrealDB extra flags to pass";
                    example = [
                      "--allow-all"
                      "--user"
                      "root"
                      "--pass"
                      "root"
                    ];
                    default = ["--unauthenticated"];
                    type = types.listOf types.str;
                  };
                };
              };
            };

            config = mkIf leaper-program.enable {
              environment.systemPackages = [leaper-program.package leaper-program.daemon-package];
              services.surrealdb = {
                enable = true;
                package = inputs.surrealdb.packages.${pkgs.system}.default;
                host = leaper-program.db.host;
                port = leaper-program.db.port;
                dbPath = leaper-program.db.path;
                extraFlags = leaper-program.db.extraFlags;
              };
              systemd = {
                services.surrealdb.serviceConfig.ProcSubset = lib.mkForce "all";
                user.services.leaper-daemon = {
                  enable = true;
                  after = ["surrealdb.service"];
                  wantedBy = ["graphical-session.target"];
                  description = "Leaper Daemon";
                  serviceConfig = {
                    Type = "simple";
                    ExecStart = "${leaper-program.daemon-package}/bin/leaper-daemon";
                    Restart = "on-failure";
                  };
                };
              };
              security.pam.services.leaper-lock = {};
            };
          };
      };
    };
}
