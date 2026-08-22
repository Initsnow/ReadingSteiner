{ config, lib, pkgs, ... }:
let
  cfg = config.services.reading-steiner;
  package = cfg.package;
in
{
  options.services.reading-steiner = {
    enable = lib.mkEnableOption "ReadingSteiner daemon";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.reading-steiner or (import ../../flake.nix { }).packages.${pkgs.stdenv.system}.default;
      description = "ReadingSteiner package to use.";
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to ReadingSteiner config.yaml.";
    };

    settings = lib.mkOption {
      type = lib.types.submodule {
        options = {
          stateDir = lib.mkOption {
            type = lib.types.str;
            default = "/var/lib/reading-steiner";
            description = "State directory.";
          };
          socketPath = lib.mkOption {
            type = lib.types.str;
            default = "/run/reading-steiner/daemon.sock";
            description = "Control socket path.";
          };
          telegram = {
            url = lib.mkOption {
              type = lib.types.str;
              default = "";
              description = "Global Telegram notification target as tgram://bottoken/ChatID1/ChatID2.";
            };
          };
          camofox = {
            enabled = lib.mkOption {
              type = lib.types.bool;
              default = false;
            };
            baseUrl = lib.mkOption {
              type = lib.types.str;
              default = "http://127.0.0.1:9377";
            };
            accessKeyFile = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
            };
            apiKeyFile = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
            };
          };
        };
      };
      default = { };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.reading-steiner-daemon = {
      description = "ReadingSteiner change detection daemon";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${lib.getExe package} serve --config ${cfg.configFile}";
        DynamicUser = true;
        StateDirectory = "reading-steiner";
        RuntimeDirectory = "reading-steiner";
        RuntimeDirectoryMode = "0750";
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        # Secrets are passed as read-only files; not embedded in /nix/store.
        LoadCredential = lib.optional (cfg.settings.camofox.accessKeyFile != null) "camofox_access_key:${cfg.settings.camofox.accessKeyFile}"
          ++ lib.optional (cfg.settings.camofox.apiKeyFile != null) "camofox_api_key:${cfg.settings.camofox.apiKeyFile}";
      };

      preStart = ''
        install -d -m 0750 -o "''${USER}" -g "''${GROUP}" ${cfg.settings.stateDir}
      '';
    };

    environment.systemPackages = [ package ];
  };
}
