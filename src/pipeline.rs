use std::collections::HashMap;

use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::config::{
    Condition, ExtractConfig, FieldSelector, FilterConfig, NormalizeConfig, PipelineConfig,
};
use crate::error::{Error, Result};
use crate::models::{FetchedDocument, Item};

#[derive(Debug, Clone, Default)]
pub struct PipelineOutput {
    pub items: Vec<Item>,
    pub fingerprint: String,
    pub text: String,
}

pub fn run_pipeline(doc: &FetchedDocument, pipeline: &PipelineConfig) -> Result<PipelineOutput> {
    let mut items = Vec::new();
    for extract in &pipeline.extract {
        let mut extracted = extract_items(doc, extract)?;
        items.append(&mut extracted);
    }
    if pipeline.extract.is_empty() {
        items.push(Item {
            stable_id: "page".to_string(),
            fields: HashMap::new(),
            image_urls: doc.images.iter().map(|i| i.canonical_url.clone()).collect(),
            text: doc.text.clone(),
            meta: HashMap::new(),
        });
    }

    apply_pipeline_stages(&mut items, pipeline, &doc.final_url)?;

    let fingerprint = compute_fingerprint(&items, &doc.text);
    Ok(PipelineOutput {
        items,
        fingerprint,
        text: doc.text.clone(),
    })
}

/// Re-run the normalize / filter stages of a pipeline on an already-extracted
/// set of items. Used by "test-pipeline": validation runs the content selector
/// against the latest snapshot's items without re-fetching the page.
pub fn rerun_on_items(items: &[Item], pipeline: &PipelineConfig) -> Result<PipelineOutput> {
    let mut items = items.to_vec();
    apply_pipeline_stages(&mut items, pipeline, "")?;
    let fingerprint = compute_fingerprint(&items, "");
    Ok(PipelineOutput {
        items,
        fingerprint,
        text: String::new(),
    })
}

fn apply_pipeline_stages(
    items: &mut Vec<Item>,
    pipeline: &PipelineConfig,
    final_url: &str,
) -> Result<()> {
    for norm in &pipeline.normalize {
        apply_normalize(items, norm, final_url)?;
    }
    *items = filter_items(std::mem::take(items), &pipeline.filter)?;
    Ok(())
}

fn extract_items(doc: &FetchedDocument, extract: &ExtractConfig) -> Result<Vec<Item>> {
    match extract {
        ExtractConfig::CssItems { selector, fields } => extract_css(doc, selector, fields),
        ExtractConfig::Xpath { selector, fields } => extract_xpath(doc, selector, fields),
        ExtractConfig::JsonPath { path, fields } => extract_json_path(doc, path, fields),
        ExtractConfig::Regex { pattern, fields } => extract_regex(doc, pattern, fields),
        ExtractConfig::AutoText => Ok(vec![Item {
            stable_id: "page".to_string(),
            fields: HashMap::new(),
            image_urls: doc.images.iter().map(|i| i.canonical_url.clone()).collect(),
            text: doc.text.clone(),
            meta: HashMap::new(),
        }]),
        ExtractConfig::AutoImages => Ok(extract_images_from_html(doc)),
        ExtractConfig::CamofoxImages => Ok(doc
            .images
            .iter()
            .enumerate()
            .map(|(i, img)| Item {
                stable_id: format!("image-{i}"),
                fields: HashMap::from([
                    ("src".to_string(), img.canonical_url.clone()),
                    ("alt".to_string(), img.alt.clone()),
                ]),
                image_urls: vec![img.canonical_url.clone()],
                text: img.alt.clone(),
                meta: HashMap::new(),
            })
            .collect()),
    }
}

fn extract_css(
    doc: &FetchedDocument,
    selector_str: &str,
    fields: &HashMap<String, FieldSelector>,
) -> Result<Vec<Item>> {
    let html = Html::parse_document(&doc.text);
    let selector = Selector::parse(selector_str)
        .map_err(|e| Error::config(format!("invalid css selector '{selector_str}': {e}")))?;
    let mut items = Vec::new();
    for (idx, el) in html.select(&selector).enumerate() {
        let mut item_fields = HashMap::new();
        for (name, fs) in fields {
            let value = extract_field_from_element(&el, fs);
            item_fields.insert(name.clone(), value);
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

fn extract_field_from_element(el: &scraper::ElementRef<'_>, fs: &FieldSelector) -> String {
    if let Some(attr) = &fs.attr
        && let Some(v) = el.value().attr(attr)
    {
        return v.to_string();
    }
    if let Some(sel) = &fs.selector
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

fn extract_xpath(
    doc: &FetchedDocument,
    selector_str: &str,
    fields: &HashMap<String, FieldSelector>,
) -> Result<Vec<Item>> {
    let package = sxd_document::parser::parse(&doc.text)
        .map_err(|e| Error::config(format!("invalid HTML for xpath: {e}")))?;
    let document = package.as_document();
    let context = sxd_xpath::Context::new();
    let factory = sxd_xpath::Factory::new();
    let xpath = factory
        .build(selector_str)
        .map_err(|e| Error::config(format!("invalid xpath '{selector_str}': {e}")))?
        .ok_or_else(|| Error::config(format!("xpath '{selector_str}' compiled to nothing")))?;
    let value = xpath
        .evaluate(&context, document.root())
        .map_err(|e| Error::config(format!("xpath evaluate error: {e}")))?;
    let mut items = Vec::new();
    if let sxd_xpath::Value::Nodeset(nodes) = value {
        for (idx, node) in nodes.document_order().into_iter().enumerate() {
            let mut item_fields = HashMap::new();
            for (name, fs) in fields {
                let val = xpath_field(&node, fs);
                item_fields.insert(name.clone(), val);
            }
            let stable_id = item_fields
                .get("id")
                .cloned()
                .unwrap_or_else(|| format!("xpath-{idx}"));
            items.push(Item {
                stable_id,
                fields: item_fields,
                image_urls: Vec::new(),
                text: String::new(),
                meta: HashMap::new(),
            });
        }
    }
    Ok(items)
}

fn xpath_field(node: &sxd_xpath::nodeset::Node, fs: &FieldSelector) -> String {
    if let Some(attr) = &fs.attr
        && let sxd_xpath::nodeset::Node::Element(el) = node
        && let Some(v) = el.attribute_value(attr.as_str())
    {
        return v.to_string();
    }
    node.string_value().trim().to_string()
}

fn extract_json_path(
    doc: &FetchedDocument,
    path: &str,
    fields: &HashMap<String, FieldSelector>,
) -> Result<Vec<Item>> {
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
            for (name, fs) in fields {
                let val = if let Some(p) = &fs.path {
                    eval_json_path(v, p)
                        .ok()
                        .and_then(|v| v.first().cloned())
                        .map(scalar_to_string)
                        .unwrap_or_default()
                } else if let Some(obj) = v.as_object() {
                    obj.get(name).map(scalar_to_string).unwrap_or_default()
                } else {
                    String::new()
                };
                item_fields.insert(name.clone(), val);
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

fn extract_regex(
    doc: &FetchedDocument,
    pattern: &str,
    fields: &HashMap<String, FieldSelector>,
) -> Result<Vec<Item>> {
    let re = Regex::new(pattern)
        .map_err(|e| Error::config(format!("invalid regex '{pattern}': {e}")))?;
    let mut items = Vec::new();
    for (idx, caps) in re.captures_iter(&doc.text).enumerate() {
        let mut item_fields = HashMap::new();
        for (name, fs) in fields {
            let value = if let Some(grp) = fs.group {
                caps.get(grp)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            } else if let Some(regex) = &fs.regex {
                Regex::new(regex)
                    .ok()
                    .and_then(|r| r.captures(caps.get(0).map(|m| m.as_str()).unwrap_or_default()))
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            } else {
                caps.get(0)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            };
            item_fields.insert(name.clone(), value);
        }
        let stable_id = item_fields
            .get("id")
            .cloned()
            .unwrap_or_else(|| format!("regex-{idx}"));
        items.push(Item {
            stable_id,
            fields: item_fields,
            image_urls: Vec::new(),
            text: caps
                .get(0)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            meta: HashMap::new(),
        });
    }
    Ok(items)
}

fn extract_images_from_html(doc: &FetchedDocument) -> Vec<Item> {
    let html = Html::parse_document(&doc.text);
    let mut items = Vec::new();
    if let Ok(sel) = Selector::parse("img") {
        for (idx, el) in html.select(&sel).enumerate() {
            let mut src = String::new();
            for attr in ["src", "data-src", "data-lazy-src"] {
                if let Some(v) = el.value().attr(attr) {
                    src = v.to_string();
                    break;
                }
            }
            if src.is_empty() {
                continue;
            }
            let alt = el.value().attr("alt").unwrap_or("").to_string();
            items.push(Item {
                stable_id: format!("img-{idx}"),
                fields: HashMap::from([
                    ("src".to_string(), src.clone()),
                    ("alt".to_string(), alt.clone()),
                ]),
                image_urls: vec![src],
                text: alt,
                meta: HashMap::new(),
            });
        }
    }
    items
}

fn apply_normalize(items: &mut [Item], norm: &NormalizeConfig, final_url: &str) -> Result<()> {
    match norm {
        NormalizeConfig::Strip { field, chars } => {
            for item in items {
                if let Some(v) = item.fields.get_mut(field) {
                    *v = v.trim_matches(|c| chars.contains(c)).to_string();
                }
            }
        }
        NormalizeConfig::Trim { field } => {
            for item in items {
                if let Some(v) = item.fields.get_mut(field) {
                    *v = v.trim().to_string();
                }
            }
        }
        NormalizeConfig::AbsUrl { field, base } => {
            let base = base.replace("{{final_url}}", final_url);
            for item in items {
                if let Some(v) = item.fields.get_mut(field)
                    && let Ok(resolved) = resolve_url(&base, v)
                {
                    *v = resolved;
                }
                for url in &mut item.image_urls {
                    if let Ok(resolved) = resolve_url(&base, url) {
                        *url = resolved;
                    }
                }
            }
        }
        NormalizeConfig::Lowercase { field } => {
            for item in items {
                if let Some(v) = item.fields.get_mut(field) {
                    *v = v.to_lowercase();
                }
            }
        }
        NormalizeConfig::Replace {
            field,
            pattern,
            with,
        } => {
            let re = Regex::new(pattern)
                .map_err(|e| Error::config(format!("invalid replace regex '{pattern}': {e}")))?;
            for item in items {
                if let Some(v) = item.fields.get_mut(field) {
                    *v = re.replace_all(v, with.as_str()).into_owned();
                }
            }
        }
    }
    Ok(())
}

fn resolve_url(base: &str, url: &str) -> Result<String> {
    let base_url = url::Url::parse(base)
        .map_err(|e| Error::config(format!("invalid base url '{base}': {e}")))?;
    Ok(base_url.join(url)?.to_string())
}

fn filter_items(items: Vec<Item>, filter: &FilterConfig) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    for item in items {
        if !filter.include.is_empty() && !filter.include.iter().all(|c| condition_matches(c, &item))
        {
            continue;
        }
        if filter.exclude.iter().any(|c| condition_matches(c, &item)) {
            continue;
        }
        out.push(item);
    }

    if let Some(dd) = &filter.drop_duplicate {
        let mut seen = std::collections::HashSet::new();
        out.retain(|item| {
            let key = render_key(&dd.key, item);
            seen.insert(key)
        });
    }

    if let Some(min) = filter.min_items
        && out.len() < min
    {
        out.clear();
    }
    Ok(out)
}

fn condition_matches(cond: &Condition, item: &Item) -> bool {
    match cond {
        Condition::Eq { field, value } => {
            item.fields.get(field).map(|v| v == value).unwrap_or(false)
        }
        Condition::Ne { field, value } => {
            item.fields.get(field).map(|v| v != value).unwrap_or(true)
        }
        Condition::Gt { field, value } => item
            .fields
            .get(field)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|v| v > *value)
            .unwrap_or(false),
        Condition::Lt { field, value } => item
            .fields
            .get(field)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|v| v < *value)
            .unwrap_or(false),
        Condition::Regex { field, pattern } => Regex::new(pattern)
            .map(|re| {
                item.fields
                    .get(field)
                    .map(|v| re.is_match(v))
                    .unwrap_or(false)
            })
            .unwrap_or(false),
        Condition::Glob { field, pattern } => item
            .fields
            .get(field)
            .map(|v| glob_match(pattern, v))
            .unwrap_or(false),
        Condition::Contains { field, value } => item
            .fields
            .get(field)
            .map(|v| v.contains(value))
            .unwrap_or(false),
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let regex = pattern
        .replace('.', "\\.")
        .replace('*', ".*")
        .replace('?', ".");
    Regex::new(&format!("^{regex}$"))
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

fn render_key(template: &str, item: &Item) -> String {
    let mut out = template.to_string();
    for (k, v) in &item.fields {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
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

    #[test]
    fn test_css_items() {
        let doc = FetchedDocument {
            final_url: "https://example.com".into(),
            status: 200,
            text: r#"<html><body><div class="item" data-id="a"><span class="title">A</span></div><div class="item" data-id="b"><span class="title">B</span></div></body></html>"#.into(),
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
        };
        let pipeline = PipelineConfig {
            extract: vec![ExtractConfig::CssItems {
                selector: ".item".into(),
                fields: HashMap::from([
                    (
                        "id".into(),
                        FieldSelector {
                            selector: None,
                            attr: Some("data-id".into()),
                            path: None,
                            regex: None,
                            group: None,
                        },
                    ),
                    (
                        "title".into(),
                        FieldSelector {
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
            filter: FilterConfig::default(),
        };
        let out = run_pipeline(&doc, &pipeline).unwrap();
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.items[0].stable_id, "a");
        assert_eq!(out.items[0].fields["title"], "A");
    }

    #[test]
    fn test_auto_text_fingerprint_tracks_text_changes() {
        let pipeline = PipelineConfig {
            extract: vec![ExtractConfig::AutoText],
            normalize: vec![],
            filter: FilterConfig::default(),
        };
        let make_doc = |text: &str| FetchedDocument {
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
        };
        let first = run_pipeline(&make_doc("hello world"), &pipeline).unwrap();
        let second = run_pipeline(&make_doc("hello world!"), &pipeline).unwrap();
        assert_ne!(first.fingerprint, second.fingerprint);
        assert_ne!(
            first.items[0].fingerprint(&[]),
            second.items[0].fingerprint(&[])
        );
    }
}
