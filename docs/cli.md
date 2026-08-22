# CLI

```text
reading-steiner serve                 # 前台跑 daemon（systemd 用），并启动 Web API + 静态资源
reading-steiner web                   # 打印 Web 控制台地址
reading-steiner status [--json]       # 运行状态
reading-steiner sources add <file>    # 添加监控源（YAML 文件，需先启动 daemon）
reading-steiner sources list          # 列出监控源（需先启动 daemon）
reading-steiner check <id>            # 立即检测一次
reading-steiner diff <event-id>       # 查看变更 diff
reading-steiner notify test           # 发测试 Telegram 消息
reading-steiner history <id>          # 变更历史
reading-steiner settings             # 查看全局设置（SQLite）
reading-steiner backup               # 备份（db + media + config，并打包 zip）
reading-steiner backups              # 列出已有备份
reading-steiner restore <name>       # 从备份恢复（在线，无需停止 daemon）
reading-steiner backup-delete <name> # 删除一个备份（目录 + zip）
reading-steiner restore-from-zip --file x.zip  # 从上传/本地的 zip 备份恢复
```

> 所有命令默认读取 `config.yaml`，可用 `--config <path>` 指定。需与 daemon 通信的命令（`sources`、`check`、`status`、`settings` 等）需先启动 daemon。

## 监控源管理

```bash
# 添加（YAML 文件，格式见 docs/sources.md）
reading-steiner sources add source.yaml
# 列出
reading-steiner sources list
```

## 全局设置

```bash
# 查看全局设置（读取 SQLite settings 表）
reading-steiner settings
```

全局可编辑设置的完整列表见 [配置文档](./configuration.md#全局设置sqlite)。修改请使用 Web 控制台「设置」页。
