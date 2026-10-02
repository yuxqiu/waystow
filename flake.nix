{
  description = "WayStow: a transient file shelf and action surface for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };
            toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            craneLib = (crane.mkLib pkgs).overrideToolchain (_: toolchain);
          in
          f { inherit pkgs toolchain craneLib; }
        );
    in
    {
      packages = forAllSystems (
        { pkgs, craneLib, ... }:
        let
          waystow = pkgs.callPackage ./nix/package.nix { inherit craneLib; };
        in
        {
          default = waystow;
          inherit waystow;
          # The same program with every action's tools on PATH.
          waystow-full = waystow.override { withTools = true; };
        }
      );

      checks = forAllSystems (
        { pkgs, craneLib, ... }:
        let
          pkg = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          inherit (pkg.passthru) cargoArtifacts common;
        in
        {
          build = pkg;
          clippy = craneLib.cargoClippy (
            common
            // {
              inherit cargoArtifacts;
              pname = "waystow-clippy";
              cargoClippyExtraArgs = "--workspace --all-targets -- --deny warnings";
            }
          );
          test = craneLib.cargoTest (
            common
            // {
              inherit cargoArtifacts;
              pname = "waystow-test";
              cargoTestExtraArgs = "--workspace";
            }
          );
          fmt = craneLib.cargoFmt {
            inherit (common) src;
            pname = "waystow-fmt";
          };
        }
      );

      devShells = forAllSystems (
        { pkgs, toolchain, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              toolchain
            ]
            ++ (with pkgs; [
              pkg-config
              wayland-utils
              wl-clipboard
              # headless UI and DnD tests (scripts/)
              sway
              swaybg
              grim
              wtype
              dbus # dbus-run-session for scripts/headless.sh
              oxipng # scripts/screenshots.sh
              # action tools, so the action tests run
            ])
            ++ (import ./nix/tools.nix pkgs);
            buildInputs = with pkgs; [
              gtk4
              gtk4-layer-shell
              wayland
            ];
            shellHook = ''
              if [ ! -e /proc/driver/nvidia ]; then
                export GBM_BACKENDS_PATH=${pkgs.mesa}/lib/gbm
                export LIBGL_DRIVERS_PATH=${pkgs.mesa}/lib/dri
                export __EGL_VENDOR_LIBRARY_FILENAMES=${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json
                VK_DRIVER_FILES=
                for icd in ${pkgs.mesa}/share/vulkan/icd.d/*.json; do
                  case $icd in */lvp_icd.*) ;; *) VK_DRIVER_FILES=$VK_DRIVER_FILES$icd: ;; esac
                done
                export VK_DRIVER_FILES
                unset icd
              fi
            '';
          };
        }
      );

      overlays.default = final: _prev: {
        waystow = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      homeManagerModules.default = import ./nix/hm-module.nix self;
    };
}
