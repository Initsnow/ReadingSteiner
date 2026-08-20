use std::collections::HashMap;

use chrono::Utc;
use reading_steiner::config::{
    CamofoxConfig, ChangeType, CompareConfig, CompareMode, Config, FetchConfig, PipelineConfig,
    SourceConfig,
};
use reading_steiner::db::Db;
use reading_steiner::differ;
use reading_steiner::fetcher::FetchSpec;
use reading_steiner::models::{Item, SnapshotRecord};
use reading_steiner::pipeline;

#[test]
fn test_db_schema_and_snapshot_roundtrip() {
    let db = Db::open_in_memory().unwrap();
    let now = Utc::now();
    let snap = SnapshotRecord {
        id: 0,
        watchpoint_id: "wp".into(),
        fetched_at: now,
        status: 200,
        etag: Some("etag-1".into()),
        last_modified: None,
        content_sha256: "abc".into(),
        normalized_fingerprint: "fp".into(),
        items_json: "[]".into(),
        duration_ms: 10,
        engine: "http".into(),
    };
    let id = db.save_snapshot(&snap).unwrap();
    assert!(id > 0);
    let latest = db.latest_snapshot("wp").unwrap().unwrap();
    assert_eq!(latest.content_sha256, "abc");
}

#[test]
fn test_differ_item_set_new_updated_removed() {
    let old_items = vec![
        Item {
            stable_id: "a".into(),
            fields: HashMap::from([("title".into(), "A".into())]),
            image_urls: vec![],
            text: String::new(),
            meta: HashMap::new(),
        },
        Item {
            stable_id: "b".into(),
            fields: HashMap::from([("title".into(), "B".into())]),
            image_urls: vec![],
            text: String::new(),
            meta: HashMap::new(),
        },
    ];
    let new_items = vec![
        Item {
            stable_id: "a".into(),
            fields: HashMap::from([("title".into(), "A2".into())]),
            image_urls: vec![],
            text: String::new(),
            meta: HashMap::new(),
        },
        Item {
            stable_id: "c".into(),
            fields: HashMap::from([("title".into(), "C".into())]),
            image_urls: vec![],
            text: String::new(),
            meta: HashMap::new(),
        },
    ];
    let cmp = CompareConfig {
        mode: CompareMode::ItemSet,
        stable_id: "id".into(),
        ignore_fields: vec![],
        notify_on: vec![ChangeType::New, ChangeType::Updated, ChangeType::Removed],
        confirm_count: 1,
        cooldown_secs: 0,
    };
    let result = differ::diff("", "", &old_items, &new_items, &cmp);
    assert!(result.changed);
    assert!(result.diff_summary.contains('+'));
    assert!(result.diff_summary.contains('~'));
    assert!(result.diff_summary.contains('-'));
}

#[tokio::test]
async fn test_http_fetcher_and_pipeline() {
    let server = wiremock::MockServer::start().await;
    use wiremock::{Match, Mock, ResponseTemplate};
    struct PathMatcher(String);
    impl Match for PathMatcher {
        fn matches(&self, request: &wiremock::Request) -> bool {
            request.url.path() == self.0
        }
    }
    Mock::given(PathMatcher("/page".into()))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<html><body><div class="product" data-id="1"><span class="title">Shoe</span><span class="price">99</span></div></body></html>"#,
        ))
        .mount(&server)
        .await;

    let cfg = Config {
        state_dir: "".into(),
        media_dir: "".into(),
        daemon: Default::default(),
        web: Default::default(),
        telegram: Default::default(),
        camofox: Default::default(),
        sources: vec![],
        pipelines: HashMap::new(),
    };
    let fetcher = reading_steiner::fetcher::create_fetcher("http", &cfg).unwrap();
    let doc = fetcher
        .fetch(&FetchSpec {
            fetch: FetchConfig {
                url: format!("{}/page", server.uri()),
                ..FetchConfig::default()
            },
            etag: None,
            last_modified: None,
            source_id: "test".into(),
        })
        .await
        .unwrap();
    assert_eq!(doc.status, 200);
    assert!(!doc.text.is_empty());

    let pipeline = PipelineConfig {
        extract: vec![reading_steiner::config::ExtractConfig::CssItems {
            selector: ".product".into(),
            fields: HashMap::from([
                (
                    "id".into(),
                    reading_steiner::config::FieldSelector {
                        selector: None,
                        attr: Some("data-id".into()),
                        path: None,
                        regex: None,
                        group: None,
                    },
                ),
                (
                    "title".into(),
                    reading_steiner::config::FieldSelector {
                        selector: Some(".title".into()),
                        attr: None,
                        path: None,
                        regex: None,
                        group: None,
                    },
                ),
            ]),
        }],
        normalize: vec![],
        filter: Default::default(),
    };
    let out = pipeline::run_pipeline(&doc, &pipeline).unwrap();
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].stable_id, "1");
}

#[tokio::test]
async fn test_camofox_contract_with_mock() {
    let server = wiremock::MockServer::start().await;
    use wiremock::{Match, Mock, ResponseTemplate};
    struct ExactPath(String);
    impl Match for ExactPath {
        fn matches(&self, request: &wiremock::Request) -> bool {
            request.url.path() == self.0
        }
    }

    Mock::given(ExactPath("/health".into()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "browserConnected": true,
            "browserRunning": true,
            "activeTabs": 0,
            "activeSessions": 0,
            "consecutiveFailures": 0
        })))
        .mount(&server)
        .await;
    Mock::given(ExactPath("/tabs".into()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tabId": "tab-1",
            "url": "https://example.com"
        })))
        .mount(&server)
        .await;
    Mock::given(ExactPath("/tabs/tab-1/navigate".into()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    Mock::given(ExactPath("/tabs/tab-1/snapshot".into()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "url": "https://example.com",
            "snapshot": "Hello snapshot",
            "hasMore": false,
            "nextOffset": null,
            "totalChars": 14
        })))
        .mount(&server)
        .await;
    Mock::given(ExactPath("/tabs/tab-1/images".into()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "images": [{"src": "https://example.com/a.jpg", "alt": "A", "width": 10, "height": 10}]
        })))
        .mount(&server)
        .await;
    Mock::given(ExactPath("/tabs/tab-1".into()))
        .and(wiremock::matchers::method("DELETE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let key_file = dir.path().join("access.key");
    std::fs::write(&key_file, "secret").unwrap();
    let camofox = CamofoxConfig {
        enabled: true,
        base_url: server.uri(),
        access_key_file: key_file,
        api_key_file: Default::default(),
        user_id: "user".into(),
        session_key: "session".into(),
        health_check_interval_secs: 1,
        pool_size: 2,
    };
    let cfg = Config {
        camofox: camofox.clone(),
        ..Config::default()
    };
    let fetcher = reading_steiner::fetcher::create_fetcher("camofox", &cfg).unwrap();
    let doc = fetcher
        .fetch(&FetchSpec {
            fetch: FetchConfig {
                engine: "camofox".into(),
                url: "https://example.com".into(),
                tab_policy: "per_check".into(),
                ..FetchConfig::default()
            },
            etag: None,
            last_modified: None,
            source_id: "cam".into(),
        })
        .await
        .unwrap();
    assert!(doc.text.contains("Hello snapshot"));
    assert_eq!(doc.images.len(), 1);
}

#[test]
fn test_config_roundtrip() {
    let src: SourceConfig = serde_yaml::from_str(
        r#"
id: s1
name: S1
enabled: true
fetch:
  engine: http
  url: https://example.com
schedule:
  interval_secs: 30
priority: 0
pipeline: default
compare:
  mode: item_set
"#,
    )
    .unwrap();
    assert_eq!(src.id, "s1");
    assert_eq!(src.schedule.interval_secs, 30);
}
