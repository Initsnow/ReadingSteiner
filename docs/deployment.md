# 部署

## NixOS

仓库提供 NixOS flake 模块，可直接引用：

```nix
{
  inputs.reading-steiner.url = "github:you/ReadingSteiner";

  outputs = { nixpkgs, reading-steiner, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        reading-steiner.nixosModules.default
        {
          services.reading-steiner = {
            enable = true;
            configFile = ./config.yaml;
            settings = {
              stateDir = "/var/lib/reading-steiner";
              # 通知目标统一为 tgram:// 形式（含 bot token 与 chat id）
              telegram.url = "tgram://bottoken/ChatID";
              camofox = {
                enabled = false;
                base_url = "http://127.0.0.1:9377";
              };
            };
          };
        }
      ];
    };
  };
}
```

`nix flake check` 会运行 fmt/clippy/unit 检查；NixOS 集成测试见 [`nixos/tests/reading-steiner.nix`](../nixos/tests/reading-steiner.nix)。

## 手动部署（systemd 示例）

```ini
# /etc/systemd/system/reading-steiner.service
[Unit]
Description=ReadingSteiner daemon
After=network.target

[Service]
ExecStart=/opt/reading-steiner/reading-steiner serve --config /etc/reading-steiner/config.yaml
Restart=on-failure
User=reading-steiner

[Install]
WantedBy=multi-user.target
```
