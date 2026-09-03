use std::collections::HashMap;

use chrono::Utc;
use reading_steiner::config::{
    CamofoxConfig, ChangeType, Config, EditableSettings, ExtractConfig, FetchConfig, ItemField,
    ItemSelector, SourceConfig, parse_telegram_url, resolve_effective_extract,
    resolve_effective_source, resolve_notify_target,
};
use reading_steiner::db::Db;
use reading_steiner::differ;
use reading_steiner::fetcher::FetchSpec;
use reading_steiner::models::{Item, SnapshotRecord, TagConfig};
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
fn test_differ_items_new_updated_removed() {
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
    // a 更新、c 新增、b 移除
    let result = differ::diff("", "", &old_items, &new_items);
    assert!(result.changed);
    assert!(result.diff_summary.contains('+'));
    assert!(result.diff_summary.contains('~'));
    assert!(result.diff_summary.contains('-'));
}

#[test]
fn test_differ_text_changed() {
    let old_item = Item {
        stable_id: "page".into(),
        fields: HashMap::new(),
        image_urls: vec![],
        text: "hello".into(),
        meta: HashMap::new(),
    };
    let new_item = Item {
        stable_id: "page".into(),
        fields: HashMap::new(),
        image_urls: vec![],
        text: "hello world".into(),
        meta: HashMap::new(),
    };
    let result = differ::diff(
        "fp-old",
        "fp-new",
        std::slice::from_ref(&old_item),
        std::slice::from_ref(&new_item),
    );
    assert!(result.changed);
    assert_eq!(result.change_type, Some(ChangeType::Updated));
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

    let cfg = Config::default();
    let fetcher =
        reading_steiner::fetcher::create_fetcher("http", &cfg, &EditableSettings::default())
            .unwrap();
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

    let extract = ExtractConfig::Items {
        selector: ItemSelector::Css {
            selector: ".product".into(),
        },
        fields: vec![
            ItemField {
                name: "id".into(),
                attr: Some("data-id".into()),
                ..Default::default()
            },
            ItemField {
                name: "title".into(),
                selector: Some(".title".into()),
                ..Default::default()
            },
        ],
        dedupe_key: None,
        images: None,
    };
    let out = pipeline::run_pipeline(&doc, &extract).unwrap();
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
    let fetcher =
        reading_steiner::fetcher::create_fetcher("camofox", &cfg, &EditableSettings::default())
            .unwrap();
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
fn test_item_fingerprint_ignore_fields_exact_match() {
    // ignore_fields 只做精确匹配，避免 `price` 误伤 `price2`。
    let item = Item {
        stable_id: "i1".into(),
        fields: HashMap::from([
            ("price".into(), "10".into()),
            ("price2".into(), "20".into()),
            ("title".into(), "x".into()),
        ]),
        image_urls: vec![],
        text: String::new(),
        meta: HashMap::new(),
    };
    let fp_ignored = item.fingerprint(&["price".into()]);
    // 忽略 price 后，price2 仍在指纹中。
    assert!(fp_ignored.contains("price2=20"));
    assert!(!fp_ignored.contains("price=10"));
    // 忽略 price2 与忽略 price 的结果不同。
    let fp_ignored2 = item.fingerprint(&["price2".into()]);
    assert_ne!(fp_ignored, fp_ignored2);
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
  cron: "*/30 * * * *"
extract:
  type: text
"#,
    )
    .unwrap();
    assert_eq!(src.id, "s1");
    assert_eq!(src.schedule.cron.as_deref(), Some("*/30 * * * *"));
    assert_eq!(src.extract, ExtractConfig::Text { images: None });
}

#[test]
fn test_extract_items_config_roundtrip() {
    let src: SourceConfig = serde_yaml::from_str(
        r#"
id: s2
name: S2
fetch:
  engine: http
  url: https://example.com
schedule:
  cron: "0 * * * *"
extract:
  type: items
  selector:
    kind: css
    selector: ".product"
  fields:
    - { name: id, attr: data-id }
"#,
    )
    .unwrap();
    match &src.extract {
        ExtractConfig::Items {
            selector, fields, ..
        } => {
            assert!(matches!(
                selector,
                ItemSelector::Css { selector } if selector == ".product"
            ));
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].name, "id");
        }
        _ => panic!("expected items extract"),
    }
}

#[test]
fn test_pending_notifications_respects_retry_backoff() {
    // 修复：失败后设置了 next_retry_at 的通知，在到期前不应被取出，
    // 否则会每 500ms 疯狂重试，退避时间完全无效。
    let db = Db::open_in_memory().unwrap();
    let now = Utc::now();

    // 立即可发：next_retry_at 为空。
    db.insert_notification(&reading_steiner::models::NotificationRecord {
        id: 0,
        event_id: 1,
        chat_id: "c".into(),
        target_json: "".into(),
        message_ids_json: "[]".into(),
        status: "pending".into(),
        attempts: 0,
        next_retry_at: None,
    })
    .unwrap();
    // 退避中：next_retry_at 在未来（+30s），不应被取出。
    db.insert_notification(&reading_steiner::models::NotificationRecord {
        id: 0,
        event_id: 2,
        chat_id: "c".into(),
        target_json: "".into(),
        message_ids_json: "[]".into(),
        status: "pending".into(),
        attempts: 1,
        next_retry_at: Some(now + chrono::Duration::seconds(30)),
    })
    .unwrap();
    // 已到期的退避：next_retry_at 在过去，应被取出。
    db.insert_notification(&reading_steiner::models::NotificationRecord {
        id: 0,
        event_id: 3,
        chat_id: "c".into(),
        target_json: "".into(),
        message_ids_json: "[]".into(),
        status: "pending".into(),
        attempts: 2,
        next_retry_at: Some(now - chrono::Duration::seconds(1)),
    })
    .unwrap();

    let pending = db.pending_notifications(50).unwrap();
    // 只有 event 1（无退避）和 event 3（已到期）应被取出，退避中的 event 2 被跳过。
    let event_ids: Vec<i64> = pending.iter().map(|n| n.event_id).collect();
    assert!(event_ids.contains(&1));
    assert!(!event_ids.contains(&2), "退避中的通知不应被取出");
    assert!(event_ids.contains(&3));
}

#[test]
fn test_css_image_selector_ignores_non_html() {
    // 修复：CSS 图片选择器对 JSON/纯文本等非 HTML 内容不应误解析。
    use reading_steiner::config::ImageSelector;
    use reading_steiner::models::FetchedDocument;

    let make_doc = |content_type: Option<&str>, text: &str| FetchedDocument {
        final_url: "https://example.com".into(),
        status: 200,
        text: text.into(),
        html: None,
        images: vec![],
        screenshot: None,
        etag: None,
        last_modified: None,
        content_sha256: "x".into(),
        normalized_fingerprint: "x".into(),
        duration_ms: 0,
        engine: "http".into(),
        content_type: content_type.map(|s| s.to_string()),
        not_modified: false,
    };

    // JSON 内容，即使包含 <img> 字样也不应被当作 HTML 解析。
    let json_doc = make_doc(
        Some("application/json"),
        r#"{"html":"<img src=\"/x.jpg\">"}"#,
    );
    let extract = ExtractConfig::Text {
        images: Some(ImageSelector::Css {
            selector: "img".into(),
        }),
    };
    let out = pipeline::run_pipeline(&json_doc, &extract).unwrap();
    assert!(
        out.image_urls.is_empty(),
        "非 HTML 内容不应被 CSS 图片选择器匹配"
    );

    // HTML 内容应正常匹配。
    let html_doc = make_doc(
        Some("text/html; charset=utf-8"),
        r#"<html><body><img src="/a.jpg"></body></html>"#,
    );
    let out = pipeline::run_pipeline(&html_doc, &extract).unwrap();
    assert_eq!(out.image_urls, vec!["/a.jpg"]);
}

#[test]
fn test_prune_history_per_source() {
    let db = Db::open_in_memory().unwrap();
    // 给两个监控源各插入 5 条事件。
    for wp in ["a", "b"] {
        for i in 0..5 {
            db.insert_change_event(&reading_steiner::models::ChangeEvent {
                id: 0,
                watchpoint_id: wp.into(),
                change_type: ChangeType::Updated,
                old_items_json: "[]".into(),
                new_items_json: "[]".into(),
                diff_summary: format!("{wp}-{i}"),
                fingerprint: "f".into(),
                dedupe_key: "d".into(),
                image_urls_json: "[]".into(),
                detected_at: Utc::now(),
                read: false,
                screenshot_path: None,
            })
            .unwrap();
        }
    }
    // 每个保留 2 条。
    db.prune_history(2).unwrap();
    let events = db.list_change_events(None, 100).unwrap();
    assert_eq!(events.len(), 4, "每个源应只保留 2 条，共 4 条");
    // 校验每个源都保留最新 2 条（diff_summary 含 -3 / -4）。
    for wp in ["a", "b"] {
        let list = db.list_change_events(Some(wp), 10).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|e| e.diff_summary == format!("{wp}-3")));
        assert!(list.iter().any(|e| e.diff_summary == format!("{wp}-4")));
    }
}

#[test]
fn test_failure_notified_field_roundtrip() {
    let db = Db::open_in_memory().unwrap();
    let sched = reading_steiner::models::ScheduleState {
        source_id: "s".into(),
        next_due_at: Utc::now(),
        consecutive_failures: 3,
        consecutive_changes: 0,
        backoff_until: None,
        last_success_at: None,
        last_error: Some("connection refused".into()),
        last_notified_fingerprint: None,
        last_notified_at: None,
        failure_notified: true,
    };
    db.upsert_schedule_state(&sched).unwrap();
    let got = db.get_schedule_state("s").unwrap().unwrap();
    assert!(got.failure_notified);
    assert_eq!(got.consecutive_failures, 3);
    assert_eq!(got.last_error.as_deref(), Some("connection refused"));
}

#[test]
fn test_event_template_rendering() {
    let event = reading_steiner::models::ChangeEvent {
        id: 1,
        watchpoint_id: "watch1".into(),
        change_type: ChangeType::New,
        old_items_json: "[]".into(),
        new_items_json: "[]".into(),
        diff_summary: "added 2 items".into(),
        fingerprint: "f".into(),
        dedupe_key: "d".into(),
        image_urls_json: "[]".into(),
        detected_at: Utc::now(),
        read: false,
        screenshot_path: None,
    };
    let text = reading_steiner::notifier::render_event_message(
        &event,
        &[],
        "<b>{label}</b> {watch} @ {time} {tz}\n{summary}",
        "Asia/Shanghai",
    );
    assert!(text.contains("NEW"));
    assert!(text.contains("watch1"));
    assert!(text.contains("added 2 items"));
    assert!(text.contains("Asia/Shanghai"));
}

#[test]
fn test_backup_delete_and_zip_restore_roundtrip() {
    use std::io::Cursor;

    let dir = tempfile::tempdir().unwrap();
    let state_dir = dir.path().join("state");
    let media_dir = dir.path().join("media");
    let cfg = Config {
        state_dir: state_dir.clone(),
        media_dir: media_dir.clone(),
        ..Config::default()
    };

    // 准备一个可备份的数据库。
    let db_path = state_dir.join("reading-steiner.db");
    std::fs::create_dir_all(&state_dir).unwrap();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);")
            .unwrap();
    }

    // 备份。
    let backup_dir = reading_steiner::backup::backup_from_path(&cfg, None).unwrap();
    let name = backup_dir
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(backup_dir.join("reading-steiner.db").exists());

    // 列表应包含它。
    let list = reading_steiner::backup::list_backups(&state_dir).unwrap();
    assert!(list.iter().any(|b| b.name == name && b.has_zip));

    // 删除它。
    assert!(reading_steiner::backup::delete_backup(&state_dir, &name).unwrap());
    assert!(!backup_dir.exists());
    let list = reading_steiner::backup::list_backups(&state_dir).unwrap();
    assert!(!list.iter().any(|b| b.name == name));

    // 再备份一次并导出为 zip 字节，模拟“上传 zip 恢复”。
    let backup_dir = reading_steiner::backup::backup_from_path(&cfg, None).unwrap();
    let list = reading_steiner::backup::list_backups(&state_dir).unwrap();
    let new_name = &list[0].name;
    let zip_path = reading_steiner::backup::backup_zip_path(&state_dir, new_name).unwrap();
    let zip_bytes = std::fs::read(&zip_path).unwrap();

    // 清空状态目录并重新从 zip 恢复。
    let restore_state_dir = dir.path().join("state2");
    let restore_cfg = Config {
        state_dir: restore_state_dir.clone(),
        media_dir: dir.path().join("media2"),
        ..Config::default()
    };
    let restored_dir =
        reading_steiner::backup::restore_from_zip(Cursor::new(zip_bytes), &restore_cfg, None)
            .unwrap();
    assert!(restored_dir.join("reading-steiner.db").exists());
    // 恢复后应把数据库复制到目标 state 目录。
    assert!(restore_state_dir.join("reading-steiner.db").exists());
    // 离线恢复后调用方应补打包 zip（与 CLI 离线路径一致），便于后续下载/管理。
    let restored_name = restored_dir
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let restored_zip = reading_steiner::backup::pack_backup_zip(&restored_dir).unwrap();
    assert!(restored_zip.exists());
    assert!(reading_steiner::backup::backup_zip_path(&restore_state_dir, &restored_name).is_some());
    let _ = backup_dir; // zip 已生成，目录保留
}

#[test]
fn test_resolve_effective_source_with_groups() {
    // 基础源：跟随分组，带标签 news。
    let mut source = SourceConfig {
        id: "s1".into(),
        name: "s1".into(),
        enabled: true,
        notify_enabled: true,
        follow_group: true,
        tags: vec!["news".into()],
        ..SourceConfig::default()
    };

    // 未配置分组时，使用自身设置与全局历史保留。
    let tags = vec![];
    assert_eq!(
        resolve_effective_source(&source, &tags, 100),
        (true, true, 100)
    );

    // 分组配置历史保留条数，则历史取分组值；开关不受分组影响。
    let group_tags = vec![TagConfig {
        name: "news".into(),
        history_limit: 50,
        extract: None,
        notify_url: String::new(),
    }];
    assert_eq!(
        resolve_effective_source(&source, &group_tags, 100),
        (true, true, 50)
    );

    // 分组历史保留为 0（不限制）时使用全局。
    let group_tags_zero = vec![TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: None,
        notify_url: String::new(),
    }];
    assert_eq!(
        resolve_effective_source(&source, &group_tags_zero, 100),
        (true, true, 100)
    );

    // 关闭“跟随分组”后历史使用全局设置。
    source.follow_group = false;
    assert_eq!(
        resolve_effective_source(&source, &group_tags, 100),
        (true, true, 100)
    );
    source.follow_group = true;

    // 多个分组取历史最小值（最严格的保留策略）。
    source.tags = vec!["news".into(), "alert".into()];
    let multi = vec![
        TagConfig {
            name: "news".into(),
            history_limit: 50,
            extract: None,
            notify_url: String::new(),
        },
        TagConfig {
            name: "alert".into(),
            history_limit: 20,
            extract: None,
            notify_url: String::new(),
        },
    ];
    assert_eq!(
        resolve_effective_source(&source, &multi, 100),
        (true, true, 20)
    );

    // 开关始终由监控源自身控制，不受分组影响。
    let mut src_off = source.clone();
    src_off.enabled = false;
    src_off.notify_enabled = false;
    assert_eq!(
        resolve_effective_source(&src_off, &group_tags, 100),
        (false, false, 50)
    );
}

#[test]
fn test_parse_telegram_url() {
    // 单 chat id。
    let t = parse_telegram_url("tgram://123:ABC/1000").unwrap();
    assert_eq!(t.token, "123:ABC");
    assert_eq!(t.chat_ids, vec!["1000".to_string()]);

    // 多 chat id。
    let t = parse_telegram_url("tgram://123:ABC/1000/2000/3000").unwrap();
    assert_eq!(t.token, "123:ABC");
    assert_eq!(
        t.chat_ids,
        vec!["1000".to_string(), "2000".to_string(), "3000".to_string()]
    );

    // 非法：非 tgram:// 前缀。
    assert!(parse_telegram_url("https://api.telegram.org").is_err());
    // 非法：缺 token。
    assert!(parse_telegram_url("tgram:///1000").is_err());
    // 非法：缺 chat id。
    assert!(parse_telegram_url("tgram://123:ABC").is_err());
}

#[test]
fn test_resolve_notify_target() {
    let source = SourceConfig {
        tags: vec!["news".into()],
        ..SourceConfig::default()
    };

    // 分组未配置通知 URL 时回退到全局。
    let tags = vec![TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: None,
        notify_url: String::new(),
    }];
    let t = resolve_notify_target(&source, &tags, "tgram://G/C9").unwrap();
    assert_eq!(t.token, "G");
    assert_eq!(t.chat_ids, vec!["C9".to_string()]);

    // 分组配置了通知 URL 时优先使用分组的。
    let tags = vec![TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: None,
        notify_url: "tgram://P/1/2".into(),
    }];
    let t = resolve_notify_target(&source, &tags, "tgram://G/C9").unwrap();
    assert_eq!(t.token, "P");
    assert_eq!(t.chat_ids, vec!["1".to_string(), "2".to_string()]);

    // 关闭跟随分组时用全局。
    let mut src_no_follow = source.clone();
    src_no_follow.follow_group = false;
    let t = resolve_notify_target(&src_no_follow, &tags, "tgram://G/C9").unwrap();
    assert_eq!(t.token, "G");

    // 分组与全局均未配置时返回 None。
    let no_url_tags = vec![TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: None,
        notify_url: String::new(),
    }];
    assert!(resolve_notify_target(&source, &no_url_tags, "").is_none());
}

#[test]
fn test_resolve_effective_extract() {
    let source = SourceConfig {
        tags: vec!["news".into()],
        extract: ExtractConfig::Text { images: None },
        ..SourceConfig::default()
    };

    // 分组未配置提取时使用源自身。
    let tags = vec![TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: None,
        notify_url: String::new(),
    }];
    assert!(matches!(
        resolve_effective_extract(&source, &tags),
        ExtractConfig::Text { .. }
    ));

    // 分组配置了提取时，源「跟随分组」则用分组的提取。
    let tags = vec![TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: Some(ExtractConfig::Items {
            selector: ItemSelector::Css {
                selector: ".item".into(),
            },
            fields: vec![],
            dedupe_key: None,
            images: None,
        }),
        notify_url: String::new(),
    }];
    assert!(matches!(
        resolve_effective_extract(&source, &tags),
        ExtractConfig::Items { .. }
    ));

    // 关闭「跟随分组」时使用源自身，即使分组配置了提取。
    let mut src_no_follow = source.clone();
    src_no_follow.follow_group = false;
    assert!(matches!(
        resolve_effective_extract(&src_no_follow, &tags),
        ExtractConfig::Text { .. }
    ));
}

#[test]
fn test_tag_db_roundtrip() {
    let db = Db::open_in_memory().unwrap();
    // 初始为空。
    assert!(db.list_tags().unwrap().is_empty());
    assert!(find_tag(&db, "news").is_none());

    // 新增。
    let tag = TagConfig {
        name: "news".into(),
        history_limit: 30,
        extract: None,
        notify_url: String::new(),
    };
    db.upsert_tag(&tag).unwrap();
    assert_eq!(db.list_tags().unwrap().len(), 1);
    let loaded = find_tag(&db, "news").unwrap();
    assert_eq!(loaded.history_limit, 30);

    // 更新。
    db.upsert_tag(&TagConfig {
        name: "news".into(),
        history_limit: 0,
        extract: None,
        notify_url: "tgram://tok/C1".into(),
    })
    .unwrap();
    let loaded = find_tag(&db, "news").unwrap();
    assert_eq!(loaded.history_limit, 0);
    assert_eq!(loaded.notify_url, "tgram://tok/C1");

    // 删除。
    assert_eq!(db.delete_tag("news").unwrap(), 1);
    assert!(find_tag(&db, "news").is_none());
}

#[test]
fn test_settings_db_roundtrip() {
    let db = Db::open_in_memory().unwrap();
    // 数据库迁移时会 seed 一条全局默认设置，未手动配置前返回默认值。
    let seeded = db.get_settings().unwrap().unwrap();
    assert_eq!(seeded.concurrency, 16);
    assert_eq!(seeded.queue_capacity, 1024);
    assert_eq!(seeded.default_timeout_secs, 30);

    let s = EditableSettings {
        concurrency: 8,
        queue_capacity: 512,
        default_timeout_secs: 15,
        default_cron: "*/5 * * * *".into(),
        default_user_agent: "test-agent".into(),
        history_limit_per_source: 100,
        failure_notify_threshold: 3,
        timezone: "Asia/Shanghai".into(),
        template: "{label}".into(),
        telegram_url: "tgram://token/C1".into(),
        max_images_per_event: 5,
    };
    db.set_settings(&s).unwrap();
    let loaded = db.get_settings().unwrap().unwrap();
    assert_eq!(loaded.concurrency, 8);
    assert_eq!(loaded.queue_capacity, 512);
    assert_eq!(loaded.default_cron, "*/5 * * * *");
    assert_eq!(loaded.timezone, "Asia/Shanghai");
    assert_eq!(loaded.telegram_url, "tgram://token/C1");

    // 覆盖更新。
    let mut s2 = s;
    s2.concurrency = 16;
    db.set_settings(&s2).unwrap();
    assert_eq!(db.get_settings().unwrap().unwrap().concurrency, 16);
}

#[test]
fn test_reload_settings_hot_update() {
    // reload_settings 应把全部字段（含并发 / 队列容量）刷新到 runtime / settings，
    // 全部即时生效。
    use reading_steiner::scheduler::AppState;

    let dir = std::env::temp_dir().join(format!("rst-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config {
        state_dir: dir.clone(),
        ..Default::default()
    };
    let state = AppState::with_config_path(cfg, None).unwrap();

    // 初始值（默认）。
    assert_eq!(state.runtime.read().unwrap().concurrency, 16);
    assert_eq!(state.runtime.read().unwrap().failure_notify_threshold, 0);

    // 热更新：全部字段（含并发 / 队列容量）都应即时刷新。
    let s = EditableSettings {
        concurrency: 999,
        queue_capacity: 999,
        default_timeout_secs: 30,
        default_cron: "*/10 * * * *".into(),
        default_user_agent: "hot-agent".into(),
        history_limit_per_source: 200,
        failure_notify_threshold: 5,
        timezone: "Asia/Shanghai".into(),
        template: "{label}".into(),
        telegram_url: "tgram://hot/C1".into(),
        max_images_per_event: 7,
    };
    state.reload_settings(&s);

    // 并发 / 队列容量也即时生效。
    assert_eq!(state.runtime.read().unwrap().concurrency, 999);
    assert_eq!(state.runtime.read().unwrap().queue_capacity, 999);
    // 其余热更新字段已刷新。
    assert_eq!(state.runtime.read().unwrap().failure_notify_threshold, 5);
    assert_eq!(state.runtime.read().unwrap().history_limit_per_source, 200);
    assert_eq!(state.runtime.read().unwrap().timezone, "Asia/Shanghai");
    assert_eq!(state.runtime.read().unwrap().default_cron, "*/10 * * * *");
    // settings 内存视图已更新（fetcher 的 UA / 超时来源）。
    assert_eq!(
        state.settings.read().unwrap().default_user_agent,
        "hot-agent"
    );
    assert_eq!(state.settings.read().unwrap().default_timeout_secs, 30);
    // notifier 重建后全局目标指向热更新的 url。
    let n = state.notifier.read().unwrap().clone().unwrap();
    assert!(n.global_target().is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_ensure_tags_registers_new_tags() {
    let db = Db::open_in_memory().unwrap();
    // 自动登记：新标签以默认值插入，已存在标签不覆盖其配置。
    db.ensure_tags(&["news".into(), "alert".into()]).unwrap();
    assert_eq!(db.list_tags().unwrap().len(), 2);

    let news = find_tag(&db, "news").unwrap();
    assert_eq!(news.history_limit, 0);

    // 更新 news 的分组配置后再次登记，不应覆盖。
    db.upsert_tag(&TagConfig {
        name: "news".into(),
        history_limit: 10,
        extract: None,
        notify_url: String::new(),
    })
    .unwrap();
    db.ensure_tags(&["news".into()]).unwrap();
    let news = find_tag(&db, "news").unwrap();
    assert_eq!(news.history_limit, 10);

    // 空标签 / 空白标签被忽略。
    db.ensure_tags(&["".into(), "   ".into()]).unwrap();
    assert_eq!(db.list_tags().unwrap().len(), 2);
}

/// 按名称取分组设置（走 `list_tags`，与生产代码同一路径）。
fn find_tag(db: &Db, name: &str) -> Option<TagConfig> {
    db.list_tags()
        .unwrap_or_default()
        .into_iter()
        .find(|t| t.name == name)
}
