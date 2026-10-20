{ config, lib, pkgs, self, ... }:
let
  cfg = config.services.reading-steiner;

  settingsFormat = (pkgs.formats.yaml { });

  # 生成权威 config.yaml。
  # 敏感 token 可通过 urlFile / authTokenFile 传入文件路径（如 /run/secrets/...），
  # 配置文件中仅记录安全的文件路径，密钥本身绝不写入 /nix/store。
  generatedConfigFile = settingsFormat.generate "reading-steiner-config.yaml" {
    state_dir = cfg.stateDir;
    media_dir = cfg.mediaDir;
    daemon = {
      log_level = cfg.logLevel;
      concurrency = cfg.daemon.concurrency;
      queue_capacity = cfg.daemon.queueCapacity;
      default_timeout_secs = cfg.daemon.defaultTimeoutSecs;
      default_cron = cfg.daemon.defaultCron;
      default_user_agent = cfg.daemon.defaultUserAgent;
      history_limit_per_source = cfg.daemon.historyLimitPerSource;
      failure_notify_threshold = cfg.daemon.failureNotifyThreshold;
      timezone = cfg.daemon.timezone;
    };
    web = {
      listen = "${cfg.web.listenAddress}:${toString cfg.web.port}";
      static_dir = cfg.web.staticDir;
    } // lib.optionalAttrs (cfg.web.authTokenFile != null) {
      auth_token_file = toString cfg.web.authTokenFile;
    };
    telegram = {
      url = cfg.telegram.url;
      api_base = cfg.telegram.apiBase;
      max_images_per_event = cfg.telegram.maxImagesPerEvent;
      image_bytes_budget = cfg.telegram.imageBytesBudget;
      digest_window_secs = cfg.telegram.digestWindowSecs;
      template = cfg.telegram.template;
    } // lib.optionalAttrs (cfg.telegram.urlFile != null) {
      url_file = toString cfg.telegram.urlFile;
    };
    browser = {
      enabled = cfg.browser.enabled;
      cdp_url = cfg.browser.cdpUrl;
      timeout_secs = cfg.browser.timeoutSecs;
    };
  };

  runtimeConfigFile = "/run/reading-steiner/config.yaml";
  runtimeConfigSource =
    if cfg.configFile != null then cfg.configFile else generatedConfigFile;

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

    logLevel = lib.mkOption {
      type = lib.types.enum [ "trace" "debug" "info" "warn" "error" ];
      default = "info";
      description = "Log level for the daemon.";
    };

    daemon = {
      concurrency = lib.mkOption {
        type = lib.types.ints.positive;
        default = 16;
        description = "Max concurrent fetch tasks executed in parallel.";
      };
      queueCapacity = lib.mkOption {
        type = lib.types.ints.positive;
        default = 1024;
        description = "Scheduler queue buffer capacity.";
      };
      defaultTimeoutSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 30;
        description = "Default fetch timeout in seconds.";
      };
      defaultCron = lib.mkOption {
        type = lib.types.str;
        default = "0 12 * * *";
        description = "Default cron schedule expression for sources.";
      };
      defaultUserAgent = lib.mkOption {
        type = lib.types.str;
        default = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36 Edg/151.0.0.0";
        description = "Default HTTP User-Agent.";
      };
      historyLimitPerSource = lib.mkOption {
        type = lib.types.ints.unsigned;
        default = 0;
        description = "Max snapshot records kept per source (0 for unlimited).";
      };
      failureNotifyThreshold = lib.mkOption {
        type = lib.types.ints.unsigned;
        default = 0;
        description = "Consecutive failures before sending failure alert (0 to disable).";
      };
      timezone = lib.mkOption {
        type = lib.types.str;
        default = "Asia/Shanghai";
        description = "IANA timezone name used by the scheduler.";
      };
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
          File containing the web console Bearer token (e.g. sops-nix secret).
        '';
      };
    };

    telegram = {
      url = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "Global Telegram notification target: tgram://bottoken/ChatID1/ChatID2.";
      };
      urlFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "Path to file containing the Telegram URL (ideal for sops-nix secrets).";
      };
      template = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "Custom message template for change events.";
      };
      maxImagesPerEvent = lib.mkOption {
        type = lib.types.ints.positive;
        default = 10;
        description = "Max images attached per change event.";
      };
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

    browser = {
      enabled = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable the external CDP browser engine (Chrome/Chromium) for dynamic pages.";
      };
      cdpUrl = lib.mkOption {
        type = lib.types.str;
        default = "http://127.0.0.1:9222";
        description = "CDP endpoint URL (e.g. http://127.0.0.1:9222 or ws://127.0.0.1:9222/...).";
      };
      timeoutSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 30;
        description = "Browser fetch and render timeout in seconds.";
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
          # 目录交给 systemd 创建并设好属主。
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
          # 声明式配置对 daemon 用户保持只读，避免 WebUI 修改后
          # 在服务重启时被 NixOS 配置覆盖。
          ExecStartPre = [
            "+${pkgs.coreutils}/bin/install -o root -g ${cfg.group} -m 0440 ${runtimeConfigSource} ${runtimeConfigFile}"
          ];
        };
      };

      networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.web.port ];

      environment.systemPackages = [ cfg.package ];
    }
  ]);
}
