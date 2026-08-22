{ pkgs, lib, ... }:
let
  testConfig = pkgs.writeText "config.yaml" ''
    state_dir: /var/lib/reading-steiner
    media_dir: /var/lib/reading-steiner/media
    daemon:
      socket_path: /run/reading-steiner/daemon.sock
      concurrency: 2
      queue_capacity: 16
    telegram:
      url: "tgram://test:token/12345"
      api_base: http://127.0.0.1:8443
    camofox:
      enabled: false
  '';
in
pkgs.testers.runNixOSTest {
  name = "reading-steiner";
  nodes.machine = { ... }: {
    services.reading-steiner = {
      enable = true;
      configFile = testConfig;
      settings = {
        stateDir = "/var/lib/reading-steiner";
        socketPath = "/run/reading-steiner/daemon.sock";
      };
    };
    systemd.services.telegram-mock = {
      description = "Telegram Bot API mock";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${pkgs.python3}/bin/python3 ${./mock_servers.py}";
        DynamicUser = true;
      };
    };
    environment.systemPackages = [ pkgs.jq ];
  };

  testScript = ''
    machine.wait_for_unit("reading-steiner-daemon.service")
    machine.wait_for_unit("telegram-mock.service")
    machine.wait_for_open_port(8080)
    machine.wait_for_open_port(8443)

    # Add a monitoring source via CLI (sources are stored in SQLite, not config.yaml)
    machine.succeed('''
      cat > /tmp/source.yaml <<'EOF'
      id: test-source
      name: Test Source
      enabled: true
      tags: []
      fetch:
        engine: http
        url: http://127.0.0.1:8080/page
        timeout_secs: 5
      schedule:
        interval_secs: 1
        jitter_secs: 0
      priority: 0
      pipeline: default
      compare:
        mode: raw_digest
        stable_id: id
        notify_on: [new, updated, removed]
    EOF
    reading-steiner sources add /tmp/source.yaml --config ${testConfig}
    ''')

    machine.wait_until_succeeds(
      "journalctl -u reading-steiner-daemon -n 100 --no-pager | grep -q 'change detected'"
    )
    machine.succeed("test -f /var/lib/reading-steiner/reading-steiner.db")
  '';
}
