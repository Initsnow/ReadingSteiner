# camofox 接入

[camofox](https://github.com/your/camofox) 是一个可选的**反检测浏览器引擎**，用于应对需要真实浏览器指纹 / 渲染的站点。它通过 HTTP API 调用，不内置、不捆绑到 ReadingSteiner。

## 接入步骤

1. 单独部署 camofox-browser（Docker / Node / 远程实例）。
2. 将 `config.yaml` 的 `camofox.enabled` 设为 `true`，填写 `base_url`、`access_key_file` / `api_key_file`。
3. 在 source 的 `fetch.engine: camofox`，按需配置 `wait`、`tab_policy`、`evaluate`、`screenshot`。

```yaml
# config.yaml
camofox:
  enabled: true
  base_url: http://127.0.0.1:9377
  access_key_file: state/camofox_access_key
  api_key_file: state/camofox_api_key
  user_id: readingsteiner
  session_key: readingsteiner
  health_check_interval_secs: 30
  pool_size: 4
```

```yaml
# source.yaml
fetch:
  engine: camofox
  url: https://example.com
  wait:
    selector: ".content"
    timeout: 10
  screenshot: true
```

## 契约测试

契约测试基于仓库内 [`camofox-openapi.json`](../camofox-openapi.json) 与 mock server，见 `tests/integration.rs::test_camofox_contract_with_mock`。
