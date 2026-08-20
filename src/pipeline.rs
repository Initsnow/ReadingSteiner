use std::collections::HashMap;

use scraper::{Html, Selector};
use serde_json::Value;

use crate::config::{ExtractConfig, ImageSelector, ItemField, ItemSelector};
use crate::error::{Error, Result};
use crate::models::{FetchedDocument, Item};

#[derive(Debug, Clone, Default)]
pub struct PipelineOutput {
    pub items: Vec<Item>,
    pub fingerprint: String,
    pub text: String,
    /// 本次变更要随通知附带的图片 URL（按图片选择器挑选，去重后保留顺序）。
    pub image_urls: Vec<String>,
}

/// 从抓取到的文档中提取监控内容，返回条目列表与内容指纹。
///
/// - `Text`：把整页文本作为单一条目，指纹跟随文本变化。
/// - `Items`：按选择器提取若干条目，自动对比条目的增 / 改 / 删。
pub fn run_pipeline(doc: &FetchedDocument, extract: &ExtractConfig) -> Result<PipelineOutput> {
    let (items, image_selector) = match extract {
        ExtractConfig::Text { images } => (vec![whole_page_item(doc)], images.as_ref()),
        ExtractConfig::Items {
            selector,
            fields,
            dedupe_key,
            images,
        } => {
            let mut items = extract_items(doc, selector, fields)?;
            if let Some(key) = dedupe_key {
                items = dedupe_items(items, key);
            }
            (items, images.as_ref())
        }
    };

    let fingerprint = compute_fingerprint(&items, &doc.text);
    let image_urls = collect_image_urls(doc, &items, image_selector);
    Ok(PipelineOutput {
        items,
        fingerprint,
        text: doc.text.clone(),
        image_urls,
    })
}

/// 按图片选择器挑选本次要随通知附带的图片 URL。
///
/// - `None` / `None` 选择器：不收集图片。
/// - `Items`：收集条目提取时自动带出的图片。
/// - `Css { selector }`：用 CSS 选择器从整页匹配图片元素。
fn collect_image_urls(
    doc: &FetchedDocument,
    items: &[Item],
    selector: Option<&ImageSelector>,
) -> Vec<String> {
    match selector {
        None | Some(ImageSelector::None) => Vec::new(),
        Some(ImageSelector::Items) => {
            let mut urls = Vec::new();
            for item in items {
                urls.extend(item.image_urls.iter().cloned());
            }
            dedupe_urls(urls)
        }
        Some(ImageSelector::Css { selector }) => {
            // CSS 图片选择器仅适用于 HTML 内容。对 JSON/纯文本等非 HTML 内容
            // 直接跳过，避免把文本当 HTML 解析产生无意义的匹配。
            if !is_html_doc(doc) {
                return Vec::new();
            }
            let urls = collect_img_urls_from_doc(doc, selector);
            dedupe_urls(urls)
        }
    }
}

/// 判断抓取到的文档是否为 HTML 内容。
/// 优先依据响应头 Content-Type；缺失时退化为对内容开头做启发式判断。
fn is_html_doc(doc: &FetchedDocument) -> bool {
    if let Some(ct) = doc.content_type.as_deref() {
        let media = ct.split(';').next().unwrap_or("").trim().to_lowercase();
        return media == "text/html" || media.ends_with("/xhtml+xml");
    }
    let text = doc.text.trim_start();
    text.starts_with('<') || text.to_lowercase().starts_with("<!doctype html")
}

/// 从整页文档中按 CSS 选择器匹配图片元素，取其 `src`/`data-src` 等属性。
fn collect_img_urls_from_doc(doc: &FetchedDocument, selector_str: &str) -> Vec<String> {
    let html = Html::parse_document(&doc.text);
    let Ok(selector) = Selector::parse(selector_str) else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    for el in html.select(&selector) {
        // 元素本身是 <img>：取 src/data-src；否则取其内部的 <img>。
        if el.value().name() == "img" {
            for attr in ["src", "data-src", "data-lazy-src"] {
                if let Some(v) = el.value().attr(attr) {
                    urls.push(v.to_string());
                    break;
                }
            }
        } else {
            urls.extend(collect_img_urls_from_element(&el));
        }
    }
    urls
}

fn dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    urls.into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

fn whole_page_item(doc: &FetchedDocument) -> Item {
    Item {
        stable_id: "page".to_string(),
        fields: HashMap::new(),
        image_urls: Vec::new(),
        text: doc.text.clone(),
        meta: HashMap::new(),
    }
}

fn extract_items(
    doc: &FetchedDocument,
    selector: &ItemSelector,
    fields: &[ItemField],
) -> Result<Vec<Item>> {
    match selector {
        ItemSelector::Css { selector } => extract_css(doc, selector, fields),
        ItemSelector::JsonPath { path } => extract_json_path(doc, path, fields),
    }
}

fn extract_css(
    doc: &FetchedDocument,
    selector_str: &str,
    fields: &[ItemField],
) -> Result<Vec<Item>> {
    let html = Html::parse_document(&doc.text);
    let selector = Selector::parse(selector_str)
        .map_err(|e| Error::config(format!("invalid css selector '{selector_str}': {e}")))?;
    let mut items = Vec::new();
    for (idx, el) in html.select(&selector).enumerate() {
        let mut item_fields = HashMap::new();
        if fields.is_empty() {
            // 未配置字段时捕获元素完整文本，确保内容变化可被检测
            item_fields.insert(
                "text".to_string(),
                el.text().collect::<Vec<_>>().join(" ").trim().to_string(),
            );
        }
        for f in fields {
            let value = extract_css_field(&el, f);
            item_fields.insert(f.name.clone(), value);
        }
        let stable_id = item_fields
            .get("id")
            .cloned()
            .unwrap_or_else(|| format!("item-{idx}"));
        let image_urls = collect_img_urls_from_element(&el);
        let text = item_fields
            .get("title")
            .or_else(|| item_fields.get("text"))
            .cloned()
            .unwrap_or_default();
        items.push(Item {
            stable_id,
            fields: item_fields,
            image_urls,
            text,
            meta: HashMap::new(),
        });
    }
    Ok(items)
}

fn extract_css_field(el: &scraper::ElementRef<'_>, f: &ItemField) -> String {
    if let Some(attr) = &f.attr
        && let Some(v) = el.value().attr(attr)
    {
        return v.to_string();
    }
    if let Some(sel) = &f.selector
        && let Ok(selector) = Selector::parse(sel)
        && let Some(inner) = el.select(&selector).next()
    {
        return inner
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
    }
    el.text().collect::<Vec<_>>().join(" ").trim().to_string()
}

fn collect_img_urls_from_element(el: &scraper::ElementRef<'_>) -> Vec<String> {
    let mut urls = Vec::new();
    if let Ok(sel) = Selector::parse("img") {
        for img in el.select(&sel) {
            for attr in ["src", "data-src", "data-lazy-src"] {
                if let Some(v) = img.value().attr(attr) {
                    urls.push(v.to_string());
                }
            }
        }
    }
    urls
}

fn extract_json_path(doc: &FetchedDocument, path: &str, fields: &[ItemField]) -> Result<Vec<Item>> {
    let value: Value = serde_json::from_str(&doc.text)?;
    let matches = eval_json_path(&value, path)?;
    let mut items = Vec::new();
    for (idx, v) in matches.iter().enumerate() {
        let mut item_fields = HashMap::new();
        if fields.is_empty() {
            if let Some(obj) = v.as_object() {
                for (k, val) in obj {
                    item_fields.insert(k.clone(), scalar_to_string(val));
                }
            }
        } else {
            for f in fields {
                let val = if let Some(p) = &f.path {
                    eval_json_path(v, p)
                        .ok()
                        .and_then(|v| v.first().cloned())
                        .map(scalar_to_string)
                        .unwrap_or_default()
                } else if let Some(obj) = v.as_object() {
                    obj.get(&f.name).map(scalar_to_string).unwrap_or_default()
                } else {
                    String::new()
                };
                item_fields.insert(f.name.clone(), val);
            }
        }
        let stable_id = item_fields
            .get("id")
            .cloned()
            .unwrap_or_else(|| format!("json-{idx}"));
        items.push(Item {
            stable_id,
            fields: item_fields,
            image_urls: extract_image_urls_from_json(v),
            text: v.to_string(),
            meta: HashMap::new(),
        });
    }
    Ok(items)
}

fn eval_json_path<'a>(value: &'a Value, path: &str) -> Result<Vec<&'a Value>> {
    // 增强版 JSONPath：支持 `$.items[*].id`、`$.items[0].name`、`$.a.b` 等链式导航，
    // 以及 `$['key']`、JSON Pointer 子集。`[*]` / `[n]` 可出现在任意层级。
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(vec![value]);
    }
    let rest = trimmed
        .strip_prefix("$.")
        .or_else(|| trimmed.strip_prefix('$'))
        .unwrap_or(trimmed);
    if rest.is_empty() {
        return Ok(vec![value]);
    }
    // 把 `a.b[0].c[*].d` 拆成若干步骤：字段访问（`a`、`b`）与下标（`[0]`、`[*]`）。
    let steps = tokenize_path_steps(rest);
    let mut cur: Vec<&'a Value> = vec![value];
    for step in steps {
        match step {
            PathStep::Field(name) => {
                cur = cur
                    .into_iter()
                    .filter_map(|v| v.get(&name))
                    .collect::<Vec<_>>();
                if cur.is_empty() {
                    return Err(Error::other(format!("json path not found: {path}")));
                }
            }
            PathStep::Index(i) => {
                cur = cur.into_iter().filter_map(|v| v.get(i)).collect::<Vec<_>>();
                if cur.is_empty() {
                    return Err(Error::other(format!(
                        "json path index out of bounds: {path}"
                    )));
                }
            }
            PathStep::Wildcard => {
                cur = cur
                    .into_iter()
                    .flat_map(|v| match v {
                        Value::Array(arr) => arr.iter().collect::<Vec<_>>(),
                        Value::Object(map) => map.values().collect::<Vec<_>>(),
                        _ => vec![v],
                    })
                    .collect::<Vec<_>>();
            }
        }
    }
    Ok(cur)
}

enum PathStep {
    Field(String),
    Index(usize),
    Wildcard,
}

/// 把 `a.b[0].c[*].d` 拆成 `Field(a), Field(b), Index(0), Field(c), Wildcard, Field(d)`。
fn tokenize_path_steps(rest: &str) -> Vec<PathStep> {
    let mut steps = Vec::new();
    // 用 `[`、`]`、`.` 作为分隔符，保留括号内的内容以识别下标/通配。
    let mut cur = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !cur.is_empty() {
                    steps.push(PathStep::Field(std::mem::take(&mut cur)));
                }
            }
            '[' => {
                if !cur.is_empty() {
                    steps.push(PathStep::Field(std::mem::take(&mut cur)));
                }
                // 收集到 `]` 为止的括号内容。
                let mut inner = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    inner.push(c);
                }
                let inner = inner.trim().trim_matches('\'');
                if inner == "*" {
                    steps.push(PathStep::Wildcard);
                } else if let Ok(i) = inner.parse::<usize>() {
                    steps.push(PathStep::Index(i));
                } else if !inner.is_empty() {
                    // `['key']` 形式的字段访问。
                    steps.push(PathStep::Field(inner.to_string()));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        steps.push(PathStep::Field(cur));
    }
    steps
}

fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn extract_image_urls_from_json(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        Value::Array(items) => {
            for item in items {
                out.extend(extract_image_urls_from_json(item));
            }
        }
        Value::Object(map) => {
            for (k, val) in map {
                let kl = k.to_lowercase();
                if kl.contains("image")
                    || kl.contains("img")
                    || kl.contains("avatar")
                    || kl.contains("cover")
                    || kl.contains("photo")
                {
                    if let Value::String(s) = val {
                        out.push(s.clone());
                    } else if let Value::Array(arr) = val {
                        for x in arr {
                            if let Value::String(s) = x {
                                out.push(s.clone());
                            }
                        }
                    }
                }
                out.extend(extract_image_urls_from_json(val));
            }
        }
        _ => {}
    }
    out
}

fn dedupe_items(items: Vec<Item>, key_template: &str) -> Vec<Item> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let mut key = key_template.to_string();
        for (k, v) in &item.fields {
            key = key.replace(&format!("{{{{{k}}}}}"), v);
        }
        if seen.insert(key) {
            out.push(item);
        }
    }
    out
}

fn compute_fingerprint(items: &[Item], text: &str) -> String {
    if items.is_empty() {
        return blake3::hash(text.as_bytes()).to_hex().to_string();
    }
    let mut parts: Vec<String> = items.iter().map(|i| i.fingerprint(&[])).collect();
    parts.sort();
    blake3::hash(parts.join("\n---\n").as_bytes())
        .to_hex()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> FetchedDocument {
        FetchedDocument {
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
            content_type: Some("text/html; charset=utf-8".into()),
            not_modified: false,
        }
    }

    #[test]
    fn test_text_extract() {
        let out = run_pipeline(&doc("hello world"), &ExtractConfig::Text { images: None }).unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].text, "hello world");
    }

    #[test]
    fn test_text_fingerprint_tracks_changes() {
        let a = run_pipeline(&doc("hello world"), &ExtractConfig::Text { images: None }).unwrap();
        let b = run_pipeline(&doc("hello world!"), &ExtractConfig::Text { images: None }).unwrap();
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn test_css_items() {
        let extract = ExtractConfig::Items {
            selector: ItemSelector::Css {
                selector: ".item".into(),
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
        let out = run_pipeline(
            &doc(
                r#"<html><body><div class="item" data-id="a"><span class="title">A</span></div><div class="item" data-id="b"><span class="title">B</span></div></body></html>"#,
            ),
            &extract,
        )
        .unwrap();
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.items[0].stable_id, "a");
        assert_eq!(out.items[0].fields["title"], "A");
    }

    #[test]
    fn test_text_with_css_image_selector() {
        // 整页文本模式下，可通过图片选择器挑选要附带的图片。
        let html = r#"<html><body><div class="cover"><img src="/a.jpg"></div>
            <p>content</p><img src="/b.png" data-src="/b2.png"></body></html>"#;
        let extract = ExtractConfig::Text {
            images: Some(ImageSelector::Css {
                selector: ".cover img".into(),
            }),
        };
        let out = run_pipeline(&doc(html), &extract).unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.image_urls, vec!["/a.jpg"]);
    }

    #[test]
    fn test_text_without_image_selector_has_no_images() {
        let out = run_pipeline(&doc("plain"), &ExtractConfig::Text { images: None }).unwrap();
        assert!(out.image_urls.is_empty());
    }

    #[test]
    fn test_items_image_selector_collects_item_images() {
        // Items 模式下用 `images: items` 收集条目自动带出的图片。
        let extract = ExtractConfig::Items {
            selector: ItemSelector::Css {
                selector: ".item".into(),
            },
            fields: vec![],
            dedupe_key: None,
            images: Some(ImageSelector::Items),
        };
        let html = r#"<html><body>
            <div class="item"><img src="/1.jpg"></div>
            <div class="item"><img src="/2.jpg"><img src="/1.jpg"></div>
        </body></html>"#;
        let out = run_pipeline(&doc(html), &extract).unwrap();
        assert_eq!(out.items.len(), 2);
        // 去重后保留顺序。
        assert_eq!(out.image_urls, vec!["/1.jpg", "/2.jpg"]);
    }

    #[test]
    fn test_css_items_without_fields_captures_text() {
        // 未配置字段时，应捕获每个条目的完整文本，确保内容变化可被检测。
        let extract = ExtractConfig::Items {
            selector: ItemSelector::Css {
                selector: ".item".into(),
            },
            fields: vec![],
            dedupe_key: None,
            images: None,
        };
        let html = r#"<html><body>
            <div class="item"><h2>First</h2><p>alpha</p></div>
            <div class="item"><h2>Second</h2><p>beta</p></div>
        </body></html>"#;
        let out = run_pipeline(&doc(html), &extract).unwrap();
        assert_eq!(out.items.len(), 2);
        // 条目正文应被捕获，且两个条目的指纹不同。
        assert!(out.items[0].fields.contains_key("text"));
        assert!(out.items[0].fields["text"].contains("First"));
        assert_ne!(out.items[0].fingerprint(&[]), out.items[1].fingerprint(&[]));
    }

    #[test]
    fn test_json_path_chain_wildcard_field() {
        // 增强后的 JSONPath 支持 `$.items[*].id` 链式导航。
        let json = r#"{"items":[{"id":"a","name":"A"},{"id":"b","name":"B"}]}"#;
        let binding = serde_json::from_str(json).unwrap();
        let matches = eval_json_path(&binding, "$.items[*].id").unwrap();
        let ids: Vec<String> = matches
            .iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn test_json_path_index_then_field() {
        let json = r#"{"items":[{"name":"first"},{"name":"second"}]}"#;
        let binding = serde_json::from_str(json).unwrap();
        let matches = eval_json_path(&binding, "$.items[1].name").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].as_str(), Some("second"));
    }
}
