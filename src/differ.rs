use crate::config::ChangeType;
use crate::models::{DiffResult, Item};

/// 对比新旧两轮提取结果，自动识别变化。不再需要「比较模式 / 稳定字段」配置：
/// - 整页文本（单条目）：指纹变化即视为内容更新。
/// - 结构化条目：按条目内容指纹自动配对，识别新增 / 更新 / 移除。
pub fn diff(
    old_fingerprint: &str,
    new_fingerprint: &str,
    old_items: &[Item],
    new_items: &[Item],
) -> DiffResult {
    // 单条目（整页文本）场景：直接比对指纹。
    if new_items.len() == 1 && old_items.len() <= 1 {
        let changed = old_fingerprint != new_fingerprint;
        return DiffResult {
            changed,
            change_type: if changed {
                Some(ChangeType::Updated)
            } else {
                None
            },
            diff_summary: if changed {
                "内容发生变化".to_string()
            } else {
                String::new()
            },
            fingerprint: new_fingerprint.to_string(),
            old_items: old_items.to_vec(),
            new_items: new_items.to_vec(),
            dedupe_key: new_fingerprint.to_string(),
        };
    }

    diff_items(old_items, new_items, new_fingerprint)
}

/// 结构化条目差异：按条目内容指纹配对，报告新增 / 更新 / 移除。
fn diff_items(old_items: &[Item], new_items: &[Item], fingerprint: &str) -> DiffResult {
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
    let mut has_added = false;
    let mut has_removed = false;
    let mut has_updated = false;

    for item in new_items {
        match old_map.get(item.stable_id.as_str()) {
            None => {
                changed = true;
                has_added = true;
                summary_parts.push(format!("+ {}", item.stable_id));
            }
            Some(old) => {
                if old.fingerprint(&[]) != item.fingerprint(&[]) {
                    changed = true;
                    has_updated = true;
                    summary_parts.push(format!("~ {}", item.stable_id));
                }
            }
        }
    }
    for item in old_items {
        if !new_map.contains_key(item.stable_id.as_str()) {
            changed = true;
            has_removed = true;
            summary_parts.push(format!("- {}", item.stable_id));
        }
    }

    let change_type = if !changed {
        None
    } else if has_added && !has_updated && !has_removed {
        Some(ChangeType::New)
    } else if has_removed && !has_added && !has_updated {
        Some(ChangeType::Removed)
    } else {
        Some(ChangeType::Updated)
    };

    let dedupe_key = format!(
        "{}:{}",
        change_type.map(|c| format!("{c:?}")).unwrap_or_default(),
        fingerprint
    );

    DiffResult {
        changed,
        change_type,
        diff_summary: summary_parts.join(", "),
        fingerprint: fingerprint.to_string(),
        old_items: old_items.to_vec(),
        new_items: new_items.to_vec(),
        dedupe_key,
    }
}
