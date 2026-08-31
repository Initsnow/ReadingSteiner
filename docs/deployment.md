# 部署

## NixOS

仓库提供 NixOS flake 模块，模块会**自动生成 config.yaml**（渲染到 `/run/reading-steiner/config.yaml`），无需自备配置文件：

```nix
{
  inputs.reading-steiner.url = "github:you/ReadingSteiner";

  outputs = { nixpkgs, reading-steiner, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        reading-steiner.nixosModules.default
        {
          services.reading-steiner = {
            enable = true;
            web = {
              port = 8901;
              # 非回环地址必须配置 token 文件（systemd credential 注入，不落 nix store）
              listenAddress = "0.0.0.0";
              authTokenFile = "/etc/reading-steiner/token";
            };
            # Camofox 浏览器引擎（可选，动态页面抓取）
            camofox = {
              enabled = true;
              baseUrl = "http://127.0.0.1:9377";
            };
          };
        }
      ];
    };
  };
}
```

### 重要说明

- **`settings.telegram.url` 不是有效选项**。Telegram 通知目标（`tgram://bottoken/ChatID`）
  只存 SQLite `settings` 表，通过 Web 控制台「设置」页或 `reading-steiner settings` 命令读写，
  不在 config.yaml / NixOS 模块里配置。模块只提供 `telegram.apiBase` 等启动引导项。
- 配置里的可编辑运行参数（并发数、默认超时、cron、UA、通知模板等）同样只存 SQLite，
  见 [configuration.md](configuration.md)。
- 目录创建由 systemd 的 `StateDirectory` / `RuntimeDirectory` 完成，服务以静态
  `User`/`Group`（默认 `reading-steiner`）运行，CLI 通过 socket 与 daemon 通信。

### 主要选项

| 选项 | 默认值 | 说明 |
| --- | --- | --- |
| `package` | flake 包 | 使用的 ReadingSteiner 包 |
| `configFile` | `null` | 自备 config.yaml（进阶）；设置后跳过自动生成 |
| `stateDir` | `/var/lib/reading-steiner` | 状态目录（SQLite、监控源、历史） |
| `mediaDir` | `/var/lib/reading-steiner/media` | 图片媒体目录 |
| `socketPath` | `/run/reading-steiner/daemon.sock` | CLI 控制套接字 |
| `logLevel` | `info` | 日志级别（trace/debug/info/warn/error） |
| `user` / `group` | `reading-steiner` | 服务运行用户/组（自动创建） |
| `openFirewall` | `false` | 防火墙放行 web 端口 |
| `web.listenAddress` | `127.0.0.1` | Web 控制台监听地址 |
| `web.port` | `8901` | Web 控制台端口 |
| `web.staticDir` | flake `packages.web` | 前端静态资源目录（默认自动接线 flake 构建的前端产物；留空则回退 daemon 相对路径 `web/dist`） |
| `web.authTokenFile` | `null` | Web Bearer token 文件（credential 注入） |
| `telegram.apiBase` | `https://api.telegram.org` | Bot API 地址 |
| `telegram.imageBytesBudget` | `10485760` | 单事件图片字节预算 |
| `telegram.digestWindowSecs` | `30` | 通知聚合窗口秒数 |
| `camofox.enabled` | `false` | 启用 Camofox 引擎 |
| `camofox.baseUrl` | `http://127.0.0.1:9377` | Camofox 地址 |
| `camofox.accessKeyFile` / `apiKeyFile` | `null` | 密钥文件（credential 注入） |
| `camofox.poolSize` | `4` | 浏览器池大小 |

### 检查

- `nix flake check` 会运行 fmt / clippy / unit 测试与模块求值检查；
- NixOS 集成测试（[`nixos/tests/reading-steiner.nix`](../nixos/tests/reading-steiner.nix)）
  需要 KVM，无 KVM 的机器上自动跳过。

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
