# The Menagerie agent as a per-user service: a launchd agent on macOS, a
# systemd user service on Linux. It runs as the user because the Home
# Manager revision lives in their home directory; the system revision files
# it also reads are world-readable.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.menagerie-agent;
  command = [
    (lib.getExe cfg.package)
    "--listen"
    cfg.listen
  ];
in
{
  options.services.menagerie-agent = {
    enable = lib.mkEnableOption "the Menagerie agent, which reports this machine's health and configuration revision over HTTP";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.menagerie-agent;
      defaultText = lib.literalExpression "menagerie.packages.\${system}.menagerie-agent";
      description = "The agent package to run.";
    };

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:9184";
      example = "100.64.0.12:9184";
      description = ''
        Address to serve `/status` and `/metrics` on. The default is reachable
        only from this machine; to reach it from another, listen on the
        machine's Tailscale address or publish it with `tailscale serve`.
      '';
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      (lib.mkIf pkgs.stdenv.hostPlatform.isDarwin {
        launchd.agents.menagerie-agent = {
          enable = true;
          config = {
            ProgramArguments = command;
            RunAtLoad = true;
            KeepAlive = true;
            ProcessType = "Background";
            StandardErrorPath = "${config.home.homeDirectory}/Library/Logs/menagerie-agent.log";
          };
        };
      })
      (lib.mkIf pkgs.stdenv.hostPlatform.isLinux {
        systemd.user.services.menagerie-agent = {
          Unit.Description = "Menagerie agent";
          Service = {
            ExecStart = lib.escapeShellArgs command;
            Restart = "always";
            RestartSec = 5;
          };
          Install.WantedBy = [ "default.target" ];
        };
      })
    ]
  );
}
