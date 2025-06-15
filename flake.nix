{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nci = {
      url = "github:yusdacra/nix-cargo-integration";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    home-manager.url = "github:nix-community/home-manager";

    tracy = {
      url = "github:wolfpld/tracy";
      flake = false;
    };
  };

  outputs = inputs @ {
    self,
    parts,
    nci,
    ...
  }:
    parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux"];
      imports = [
        inputs.home-manager.flakeModules.home-manager
        nci.flakeModule
        ./crates.nix
      ];
      perSystem = {
        config,
        pkgs,
        ...
      }: let
        outputs = config.nci.outputs;
      in {
        devShells.default = outputs."leaper".devShell.overrideAttrs (old: {
          packages =
            (old.packages or [])
            ++ (with pkgs; [
              cargo-edit
              cargo-expand
              cargo-machete
              cargo-audit
              cargo-bloat
              cargo-features-manager

              tracy
            ]);
        });

        packages = {
          default = outputs."leaper".packages.release;
          leaper = config.packages.default;
        };
      };

      flake = {
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
    };
}
