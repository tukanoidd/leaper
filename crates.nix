{...}: {
  perSystem = {pkgs, ...}: {
    nci = {
      projects."leaper" = {
        path = ./.;
        export = true;
      };
      crates = {
        "leaper" = {
          runtimeLibs = with pkgs; [
            vulkan-loader
            wayland
            xorg.libX11
            libxkbcommon
          ];
        };
        "leaper-core" = {};
      };
    };
  };
}
