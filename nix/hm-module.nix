self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.waystow;
  toml = pkgs.formats.toml { };
  configFile = toml.generate "waystow-config.toml" cfg.settings;
in
{
  options.services.waystow = {
    enable = lib.mkEnableOption "WayStow, a transient file shelf and action surface for Wayland";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.waystow-full;
      defaultText = lib.literalExpression "waystow.packages.\${system}.waystow-full";
      description = ''
        The WayStow package. The default, `waystow-full`, brings the tools behind
        every built-in action (ImageMagick, ffmpeg, qpdf, Ghostscript, OCRmyPDF…).
        Use `waystow` to rely on what is already on your PATH.
      '';
    };

    settings = lib.mkOption {
      inherit (toml) type;
      default = { };
      example = lib.literalExpression ''
        {
          catcher.edges = [ "right" "left" ];
          shelf.expiry = "2h";
          ui = { theme = "system"; client_shadow = false; };
        }
      '';
      description = ''
        Written to {file}`$XDG_CONFIG_HOME/waystow/config.toml`. See the README for every key.
        Unknown keys are rejected so typos surface at daemon start.
      '';
    };

    actions = lib.mkOption {
      type = lib.types.attrsOf lib.types.lines;
      default = { };
      example = lib.literalExpression ''
        {
          "upload" = '''
            [[action]]
            id = "upload.0x0"
            title = "Upload to 0x0.st"
            command = ["curl", "-sF", "file=@{in}", "https://0x0.st"]
            stdout = "clipboard"
          ''';
        }
      '';
      description = "Extra action files, written to {file}`$XDG_CONFIG_HOME/waystow/actions/<name>.toml`.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile = {
      "waystow/config.toml" = lib.mkIf (cfg.settings != { }) { source = configFile; };
    }
    // lib.mapAttrs' (
      name: text: lib.nameValuePair "waystow/actions/${name}.toml" { inherit text; }
    ) cfg.actions;

    systemd.user.services.waystow = {
      Unit = {
        Description = "WayStow file shelf";
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
        X-Restart-Triggers = lib.optional (cfg.settings != { }) "${configFile}";
      };
      Service = {
        ExecStart = "${lib.getExe cfg.package} daemon";
        Restart = "on-failure";
        RestartSec = 2;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
