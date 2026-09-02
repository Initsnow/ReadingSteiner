{ pkgs, lib, reading-steiner, ... }:
# ReadingSteiner NixOS 集成测试。需要 KVM；由 flake checks 里的
# `builtins.pathExists "/dev/kvm"` 守卫，无 KVM 环境自动跳过。
let
  # Telegram Bot API mock 端口与页面服务端口
  pagePort = 8080;
  tgPort = 8443;

  sourceYaml = pkgs.writeText "source.yaml" ''
    id: test-source
    name: Test Source
    enabled: true
    tags: []
    fetch:
      engine: http
      url: http://127.0.0.1:${toString pagePort}/page
      timeout_secs: 5
    schedule:
      cron: "* * * * *"
  '';
in
pkgs.testers.runNixOSTest {
  name = "reading-steiner";

  nodes.machine = { pkgs, config, ... }: {
    imports = [ ../../module.nix ];
    # module 期望 self 参数用于默认包回退，直接传入 flake 自身
    _module.args.self = reading-steiner;

    services.reading-steiner = {
      enable = true;
      package = reading-steiner.packages.x86_64-linux.default;
      web = {
        listenAddress = "127.0.0.1";
        port = 8901;
        # 集成测试仅验证 API，不把前端 web 包拖入构建
        staticDir = lib.mkForce "";
      };
      telegram.apiBase = "http://127.0.0.1:${toString tgPort}";
    };

    systemd.services.telegram-mock = {
      description = "Telegram Bot API mock + page server";
      wantedBy = [ "multi-user.target" ];
      before = [ "reading-steiner-daemon.service" ];
      serviceConfig = {
        ExecStart = "${pkgs.python3}/bin/python3 ${./mock_servers.py}";
        DynamicUser = true;
      };
    };

    # curl 探测 API；CLI 包入 PATH 以便 sources add / status 子命令
    environment.systemPackages = [ pkgs.jq pkgs.curl config.services.reading-steiner.package ];
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("telegram-mock.service")
    machine.wait_for_open_port(${toString pagePort})
    machine.wait_for_open_port(${toString tgPort})

    with subtest("daemon starts (preStart works under declared User/Group)"):
        machine.wait_for_unit("reading-steiner-daemon.service")
        machine.wait_for_open_port(8901)

    with subtest("state dir owned by service user"):
        machine.succeed("test -d /var/lib/reading-steiner")
        machine.succeed(
            "test \"$(stat -c %U /var/lib/reading-steiner)\" = reading-steiner"
        )

    with subtest("control socket exists"):
        machine.wait_for_file("/run/reading-steiner/daemon.sock")
        machine.succeed("test -S /run/reading-steiner/daemon.sock")

    with subtest("web console responds without token (loopback)"):
        machine.succeed("curl -sf http://127.0.0.1:8901/api/status")

    with subtest("CLI talks to daemon over socket"):
        machine.succeed("reading-steiner status --config /run/reading-steiner/config.yaml")

    with subtest("add a source and scheduler detects change"):
        machine.succeed(
            "reading-steiner sources add ${sourceYaml} --config /run/reading-steiner/config.yaml"
        )
        machine.wait_until_succeeds(
            "journalctl -u reading-steiner-daemon -n 100 --no-pager | grep -q 'change detected'",
            timeout=60,
        )

    with subtest("sqlite db created in state dir"):
        machine.succeed("test -f /var/lib/reading-steiner/reading-steiner.db")
  '';
}
