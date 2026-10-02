# WayStow, built with crane. `withTools = true` puts every built-in action's
# command-line tools on the wrapper's PATH (the `waystow-full` package);
# otherwise actions use whatever is on the user's PATH and hide when missing.
{
  lib,
  pkgs,
  craneLib,
  pkg-config,
  wrapGAppsHook4,
  gtk4,
  gtk4-layer-shell,
  glib,
  librsvg,
  withTools ? false,
}:
let
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../dev
      ../rustfmt.toml
      ../clippy.toml
      ../dist
    ];
  };
  common = {
    inherit src;
    strictDeps = true;
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [
      gtk4
      gtk4-layer-shell
      glib
    ];
  };
  cargoArtifacts = craneLib.buildDepsOnly (common // { pname = "waystow-deps"; });
  tools = import ./tools.nix pkgs;
in
craneLib.buildPackage (
  common
  // {
    pname = "waystow";
    inherit cargoArtifacts;
    cargoExtraArgs = "-p waystow";
    nativeBuildInputs = common.nativeBuildInputs ++ [ wrapGAppsHook4 ];
    # SVG thumbnails and icons.
    buildInputs = common.buildInputs ++ [ librsvg ];
    # Workspace tests run in the flake's `checks`.
    doCheck = false;
    preFixup = lib.optionalString withTools ''
      gappsWrapperArgs+=(--suffix PATH : ${lib.makeBinPath tools})
    '';
    postInstall = ''
      install -Dm644 ${../dist/waystow.service} $out/lib/systemd/user/waystow.service
      substituteInPlace $out/lib/systemd/user/waystow.service --replace-fail "ExecStart=waystow" "ExecStart=$out/bin/waystow"
    '';
    passthru = { inherit cargoArtifacts common; };
    meta = {
      description = "A transient file shelf and action surface for Wayland (layer-shell)";
      license = with lib.licenses; [
        mit
        asl20
      ];
      mainProgram = "waystow";
      platforms = lib.platforms.linux;
    };
  }
)
