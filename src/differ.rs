use crate::config::{ChangeType, CompareConfig, CompareMode};
use crate::models::{DiffResult, Item};

pub fn diff(
    old_fingerprint: &str,
    new_fingerprint: &str,
    old_items: &[Item],
    new_items: &[Item],
    compare: &CompareConfig,
) -> DiffResult {
    match compare.mode {
        CompareMode::RawDigest => diff_raw(old_fingerprint, new_fingerprint, old_items, new_items),
        CompareMode::ItemSet => diff_item_set(old_items, new_items, compare),
        CompareMode::TextSim => {
            diff_text_sim(old_fingerprint, new_fingerprint, old_items, new_items)
        }
    }
}

fn diff_raw(
    old_fingerprint: &str,
    new_fingerprint: &str,
    old_items: &[Item],
    new_items: &[Item],
) -> DiffResult {
    let changed = old_fingerprint != new_fingerprint;
    DiffResult {
        changed,
        change_type: if changed {
            Some(ChangeType::Updated)
        } else {
            None
        },
        diff_summary: if changed {
            "raw content fingerprint changed".to_string()
        } else {
            String::new()
        },
        fingerprint: new_fingerprint.to_string(),
        old_items: old_items.to_vec(),
        new_items: new_items.to_vec(),
        dedupe_key: new_fingerprint.to_string(),
    }
}

fn diff_item_set(old_items: &[Item], new_items: &[Item], compare: &CompareConfig) -> DiffResult {
    let old_map: std::collections::HashMap<&str, &Item> = old_items
        .iter()
        .map(|i| (i.stable_id.as_str(), i))
        .collect();
    let new_map: std::collections::HashMap<&str, &Item> = new_items
        .iter()
        .map(|i| (i.stable_id.as_str(), i))
        .collect();

    let mut summary_parts = Vec::new();
    let mut changed = false;

    for item in new_items {
        match old_map.get(item.stable_id.as_str()) {
            None => {
                changed = true;
                summary_parts.push(format!("+ {}", item.stable_id));
            }
            Some(old) => {
                let old_fp = old.fingerprint(&compare.ignore_fields);
                let new_fp = item.fingerprint(&compare.ignore_fields);
                if old_fp != new_fp {
                    changed = true;
                    summary_parts.push(format!("~ {}", item.stable_id));
                }
            }
        }
    }
    for item in old_items {
        if !new_map.contains_key(item.stable_id.as_str()) {
            changed = true;
            summary_parts.push(format!("- {}", item.stable_id));
        }
    }

    let change_type = if !changed {
        None
    } else if new_items.len() > old_items.len() {
        Some(ChangeType::New)
    } else if new_items.len() < old_items.len() {
        Some(ChangeType::Removed)
    } else {
        Some(ChangeType::Updated)
    };

    let fingerprint = fingerprint_items(new_items);
    let dedupe_key = format!(
        "{}:{}",
        change_type.map(|c| format!("{c:?}")).unwrap_or_default(),
        fingerprint
    );

    DiffResult {
        changed,
        change_type,
        diff_summary: summary_parts.join(", "),
        fingerprint,
        old_items: old_items.to_vec(),
        new_items: new_items.to_vec(),
        dedupe_key,
    }
}

fn diff_text_sim(
    old_fingerprint: &str,
    new_fingerprint: &str,
    old_items: &[Item],
    new_items: &[Item],
) -> DiffResult {
    if old_fingerprint == new_fingerprint {
        return DiffResult {
            changed: false,
            change_type: None,
            diff_summary: String::new(),
            fingerprint: new_fingerprint.to_string(),
            old_items: old_items.to_vec(),
            new_items: new_items.to_vec(),
            dedupe_key: new_fingerprint.to_string(),
        };
    }
    // Simple similarity fallback: use item_set summary if items exist.
    let item_result = diff_item_set(
        old_items,
        new_items,
        &CompareConfig {
            mode: CompareMode::ItemSet,
            ..CompareConfig::default()
        },
    );
    DiffResult {
        changed: true,
        change_type: Some(ChangeType::Updated),
        diff_summary: item_result.diff_summary,
        fingerprint: new_fingerprint.to_string(),
        old_items: old_items.to_vec(),
        new_items: new_items.to_vec(),
        dedupe_key: new_fingerprint.to_string(),
    }
}

fn fingerprint_items(items: &[Item]) -> String {
    let mut parts: Vec<String> = items.iter().map(|i| i.fingerprint(&[])).collect();
    parts.sort();
    blake3::hash(parts.join("\n---\n").as_bytes())
        .to_hex()
        .to_string()
}
