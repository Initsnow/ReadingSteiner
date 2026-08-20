use std::collections::HashMap;

use scraper::{Html, Selector};
use serde_json::Value;

use crate::config::{ExtractConfig, ItemField, ItemSelector};
use crate::error::{Error, Result};
use crate::models::{FetchedDocument, Item};

#[derive(Debug, Clone, Default)]
pub struct PipelineOutput {
    pub items: Vec<Item>,
    pub fingerprint: String,
    pub text: String,
}

/// 从抓取到的文档中提取监控内容，返回条目列表与内容指纹。
///
/// - `Text`：把整页文本作为单一条目，指纹跟随文本变化。
/// - `Items`：按选择器提取若干条目，自动对比条目的增 / 改 / 删。
pub fn run_pipeline(doc: &FetchedDocument, extract: &ExtractConfig) -> Result<PipelineOutput> {
    let items = match extract {
        ExtractConfig::Text => vec![whole_page_item(doc)],
        ExtractConfig::Items {
            selector,
            fields,
            dedupe_key,
        } => {
            let mut items = extract_items(doc, selector, fields)?;
            if let Some(key) = dedupe_key {
                items = dedupe_items(items, key);
            }
            items
        }
    };

    let fingerprint = compute_fingerprint(&items, &doc.text);
    Ok(PipelineOutput {
        items,
        fingerprint,
        text: doc.text.clone(),
    })
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
    // Supports simple JSONPath: $.items[*], $.items, $['key'], and JSON pointer subset.
    let trimmed = path.trim();
    if trimmed == "$" {
        return Ok(vec![value]);
    }
    if let Some(rest) = trimmed.strip_prefix("$.") {
        let mut cur = value;
        for part in rest.split('.') {
            let part = part
                .trim_end_matches("[*]")
                .trim_start_matches('[')
                .trim_end_matches(']');
            if part.is_empty() {
                continue;
            }
            cur = cur
                .get(part)
                .ok_or_else(|| Error::other(format!("json path not found: {path}")))?;
        }
        return Ok(flatten_array(cur));
    }
    if let Some(rest) = trimmed.strip_prefix("$[") {
        let key = rest.trim_end_matches(']').trim_matches('\'');
        if let Some(arr) = value.as_array() {
            if key == "*" {
                return Ok(arr.iter().collect());
            }
            if let Ok(i) = key.parse::<usize>() {
                return arr
                    .get(i)
                    .map(|v| vec![v])
                    .ok_or_else(|| Error::other(format!("json path index out of bounds: {path}")));
            }
        }
        return value
            .get(key)
            .map(|v| flatten_array(v))
            .ok_or_else(|| Error::other(format!("json path not found: {path}")));
    }
    Err(Error::other(format!("unsupported json path: {path}")))
}

fn flatten_array(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(arr) => arr.iter().collect(),
        _ => vec![v],
    }
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
            not_modified: false,
        }
    }

    #[test]
    fn test_text_extract() {
        let out = run_pipeline(&doc("hello world"), &ExtractConfig::Text).unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].text, "hello world");
    }

    #[test]
    fn test_text_fingerprint_tracks_changes() {
        let a = run_pipeline(&doc("hello world"), &ExtractConfig::Text).unwrap();
        let b = run_pipeline(&doc("hello world!"), &ExtractConfig::Text).unwrap();
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
}
