# 备份与恢复

备份包含 SQLite 数据库（含监控源、事件、**全局设置**、媒体缓存索引）与 media 目录，并打包为 zip，可用于整机迁移或灾难恢复。

## 命令行

```bash
# 备份（在线一致性快照，含 db + media + config，并打包 zip）
reading-steiner backup --config config.local.yaml
# 列出备份
reading-steiner backups
# 恢复（在线，无需停止 daemon；daemon 未运行时自动回退到离线恢复）
reading-steiner restore <备份名> --config config.local.yaml
# 删除备份
reading-steiner backup-delete <备份名> --config config.local.yaml
# 从本地/上传的 zip 备份包恢复（daemon 在线时自动走在线恢复）
reading-steiner restore-from-zip --file backup.zip --config config.local.yaml
```

备份保存在 `state/backups/<时间戳>/`，并打包为同名 `<时间戳>.zip` 供下载。

## Web 控制台

「设置 → 备份与恢复」提供一键备份、备份列表、**zip 下载**、**删除备份**与**在线恢复**（无需停止 daemon）。

## 从 zip 备份包恢复 / 跨机器迁移

备份 zip 可下载后带到另一台机器：

- **Web 控制台**：「设置 → 备份与恢复」点击「上传 zip 恢复」，选择 `.zip` 备份包即在线恢复。
- **CLI**：`reading-steiner restore-from-zip --file backup.zip`。

上传的 zip 会被解压到 `state/backups/<新时间戳>/`（仅接受安全相对路径，拒绝路径穿越），然后在线恢复数据库与 media。
