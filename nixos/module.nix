{ config, lib, pkgs, self, ... }:
let
  cfg = config.services.reading-steiner;

  settingsFormat = (pkgs.formats.yaml { });

  # 生成 config.yaml。authTokenFile 不落 /nix/store，
  # 由 preStart 用 replace-secret 从 credential 渲染到运行时配置。
  generatedConfigFile = settingsFormat.generate "reading-steiner-config.yaml" {
    state_dir = cfg.stateDir;
    media_dir = cfg.mediaDir;
    daemon = {
      socket_path = cfg.socketPath;
      log_level = cfg.logLevel;
    };
    web = {
      listen = "${cfg.web.listenAddress}:${toString cfg.web.port}";
      static_dir = cfg.web.staticDir;
    } // lib.optionalAttrs (cfg.web.authTokenFile != null) {
      auth_token = "@auth-token@";
    };
    telegram = {
      api_base = cfg.telegram.apiBase;
      image_bytes_budget = cfg.telegram.imageBytesBudget;
      digest_window_secs = cfg.telegram.digestWindowSecs;
    };
    camofox = {
      enabled = cfg.camofox.enabled;
      base_url = cfg.camofox.baseUrl;
      access_key_file =
        if cfg.camofox.accessKeyFile != null then "/run/credentials/reading-steiner-daemon.service/camofox_access_key" else "";
      api_key_file =
        if cfg.camofox.apiKeyFile != null then "/run/credentials/reading-steiner-daemon.service/camofox_api_key" else "";
      user_id = cfg.camofox.userId;
      session_key = cfg.camofox.sessionKey;
      health_check_interval_secs = cfg.camofox.healthCheckIntervalSecs;
      pool_size = cfg.camofox.poolSize;
    };
  };

  runtimeConfigFile = "/run/reading-steiner/config.yaml";

  hasToken = cfg.web.authTokenFile != null;

  execStart = "${lib.getExe cfg.package} serve --config ${runtimeConfigFile}";
in
{
  options.services.reading-steiner = {
    enable = lib.mkEnableOption "ReadingSteiner web/data change detection service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "pkgs.reading-steiner";
      description = "ReadingSteiner package to use.";
    };

    # 声明式配置。configFile 提供时跳过自动生成（进阶用法，此时
    # 下方各选项仅用于设置 systemd 目录/用户，须自行保持一致）。
    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Optional custom config.yaml. When set, it is used as-is (except that
        `@auth-token@` is still replaced when `web.authTokenFile` is set);
        otherwise the module generates one from the options below.
      '';
    };

    stateDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/reading-steiner";
      description = "State directory (SQLite database, sources, history).";
    };

    mediaDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/reading-steiner/media";
      description = "Media directory for downloaded images.";
    };

    socketPath = lib.mkOption {
      type = lib.types.str;
      default = "/run/reading-steiner/daemon.sock";
      description = "Control socket path used by the CLI to talk to the daemon.";
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [ "trace" "debug" "info" "warn" "error" ];
      default = "info";
      description = "Log level for the daemon.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "reading-steiner";
      description = "System user the daemon runs as (created automatically).";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "reading-steiner";
      description = "System group the daemon runs as (created automatically).";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the web console port in the firewall.";
    };

    environment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Extra environment variables (e.g. RUST_LOG overrides logLevel).";
    };

    web = {
      listenAddress = lib.mkOption {
        type = lib.types.str;
        default = "127.0.0.1";
        description = "Web console listen address.";
      };
      port = lib.mkOption {
        type = lib.types.port;
        default = 8901;
        description = "Web console port.";
      };
      staticDir = lib.mkOption {
        type = lib.types.str;
        # 默认接线 flake 的 packages.web（前端构建产物）；无该包时留空 = 仅 API。
        # 注意留空时 Rust 端会回退到相对路径 web/dist，而非「仅 API」，
        # 因此显式设为指向 flake web 包的绝对路径。
        default = lib.mkDefault (
          if (self.packages.${pkgs.system} or { }) ? web
          then "${self.packages.${pkgs.system}.web}"
          else ""
        );
        defaultText = lib.literalExpression "flake `packages.web` outPath, or \"\"";
        description = ''
          Web console static assets directory. Empty means falling back to the
          daemon's built-in relative `web/dist` lookup; prefer the flake's
          `packages.web` for a fully declarative deployment.
        '';
      };
      authTokenFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = ''
          File containing the web console Bearer token. Read via systemd
          credentials and rendered into the runtime config; never copied to
          the nix store. Required when listening on a non-loopback address.
        '';
      };
    };

    telegram = {
      apiBase = lib.mkOption {
        type = lib.types.str;
        default = "https://api.telegram.org";
        description = "Telegram Bot API base URL.";
      };
      imageBytesBudget = lib.mkOption {
        type = lib.types.ints.positive;
        default = 10485760;
        description = "Per-event image download byte budget.";
      };
      digestWindowSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 30;
        description = "Digest window for batching notifications, in seconds.";
      };
    };

    camofox = {
      enabled = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable the Camofox browser engine for dynamic pages.";
      };
      baseUrl = lib.mkOption {
        type = lib.types.str;
        default = "http://127.0.0.1:9377";
        description = "Camofox engine base URL.";
      };
      accessKeyFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "File containing the Camofox access key (passed as a credential).";
      };
      apiKeyFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "File containing the Camofox API key (passed as a credential).";
      };
      userId = lib.mkOption {
        type = lib.types.str;
        default = "readingsteiner";
        description = "Camofox user id.";
      };
      sessionKey = lib.mkOption {
        type = lib.types.str;
        default = "readingsteiner";
        description = "Camofox session key.";
      };
      healthCheckIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 30;
        description = "Camofox health check interval, in seconds.";
      };
      poolSize = lib.mkOption {
        type = lib.types.ints.positive;
        default = 4;
        description = "Camofox browser pool size.";
      };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      assertions = [
        {
          assertion = cfg.web.authTokenFile != null
            || lib.hasPrefix "127.0.0.1" cfg.web.listenAddress
            || lib.hasPrefix "::1" cfg.web.listenAddress
            || cfg.web.listenAddress == "localhost";
          message = ''
            services.reading-steiner: the web console listens on a non-loopback
            address (${cfg.web.listenAddress}) without authentication. Set
            `services.reading-steiner.web.authTokenFile` or bind to a loopback address.
          '';
        }
      ];

      # 优先 pkgs.reading-steiner（可用 overlay 提供），否则用 flake 自带包
      services.reading-steiner.package = lib.mkDefault (
        if pkgs ? reading-steiner then pkgs.reading-steiner
        else if (self.packages.${pkgs.system} or { }) ? default then self.packages.${pkgs.system}.default
        else throw "services.reading-steiner: no package found. Set `services.reading-steiner.package` or add this flake's overlay."
      );

      users.users.${cfg.user} = {
        isSystemUser = true;
        group = cfg.group;
        description = "ReadingSteiner daemon user";
      };
      users.groups.${cfg.group} = { };

      systemd.services.reading-steiner-daemon = {
        description = "ReadingSteiner change detection daemon";
        wantedBy = [ "multi-user.target" ];
        after = [ "network-online.target" ];
        wants = [ "network-online.target" ];

        serviceConfig = {
          Type = "simple";
          ExecStart = execStart;
          User = cfg.user;
          Group = cfg.group;
          # 目录交给 systemd 创建并设好属主，preStart 不再依赖 $USER/$GROUP
          StateDirectory = lib.removePrefix "/var/lib/" cfg.stateDir;
          RuntimeDirectory = "reading-steiner";
          RuntimeDirectoryMode = "0750";
          ProtectSystem = "strict";
          ReadWritePaths = [ cfg.stateDir ];
          ProtectHome = true;
          PrivateTmp = true;
          PrivateDevices = true;
          NoNewPrivileges = true;
          RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
          LockPersonality = true;
          MemoryDenyWriteExecute = true;
          RestrictRealtime = true;
          RestrictSUIDSGID = true;
          LoadCredential = lib.optional hasToken "auth_token:${cfg.web.authTokenFile}"
            ++ lib.optional (cfg.camofox.accessKeyFile != null) "camofox_access_key:${cfg.camofox.accessKeyFile}"
            ++ lib.optional (cfg.camofox.apiKeyFile != null) "camofox_api_key:${cfg.camofox.apiKeyFile}";
        };

        # 把配置（含 token 占位符）渲染到 RuntimeDirectory。
        # 注意：不使用 $USER/$GROUP —— 服务无 DynamicUser，目录由 systemd 创建。
        preStart = ''
          tmp=$(mktemp)
          trap 'rm -f "$tmp"' EXIT
          cp ${if cfg.configFile != null then cfg.configFile else generatedConfigFile} "$tmp"
          ${lib.optionalString hasToken ''
            ${lib.getExe pkgs.replace-secret} '@auth-token@' "$CREDENTIALS_DIRECTORY/auth_token" "$tmp"
          ''}
          # ExecStartPre 默认以 User=/Group= 身份运行（非 root），无需也不能 chown：
          # install 创建的文件自然归服务用户所有，服务进程 0640 可读。
          install -m 0640 "$tmp" ${runtimeConfigFile}
        '';
      };

      networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.web.port ];

      environment.systemPackages = [ cfg.package ];
    }
  ]);
}
