//! cron 表达式解析与调度时间计算。
//!
//! 对外只暴露两个能力：
//! - [`next_due`]：按标准 5 段表达式求下一次触发时刻（配置时区的本地时间）。
//! - [`format_local`]：把 UTC 时间按 IANA 时区渲染成可读字符串。
//!
//! 表达式采用标准 5 段 cron（`分 时 日 月 周`，周日可为 0 或 7），内部转换为
//! `cron` crate 的 7 段格式（追加秒 `0` 与年 `*`）后计算。

use std::str::FromStr;

use chrono::{DateTime, Local, Utc};
use cron::Schedule as CronSchedule;

use crate::error::{Error, Result};

/// 把标准 cron 的星期值（0-7，0/7=周日）转为 cron crate 的星期值（1-7，1=周日）。
fn map_dow(v: i32) -> i32 {
    match v {
        0 | 7 => 1,                         // 周日 → 1
        n if (1..=6).contains(&n) => n + 1, // 周一~周六 → 2~7
        n => n,
    }
}

/// 在标准星期环（0-6，7 归一为 0）上展开范围 `a-b`，支持跨周（`5-7` → [5,6,0]）。
fn expand_dow_range(a: i32, b: i32) -> Vec<i32> {
    let norm = |v: i32| if v == 7 { 0 } else { v };
    let (na, nb) = (norm(a), norm(b));
    if na <= nb {
        return (na..=nb).collect();
    }
    // 跨周：a..6 再接 0..b
    (na..=6).chain(0..=nb).collect()
}

/// 把标准 5 段表达式的第 5 段（星期）转为 cron crate 的星期字段。
/// 支持 `*`、`n`、`a-b`、`a-b/n`、`*/n`、`n/n` 与逗号列表；命名（SUN/MON）原样透传。
fn convert_dow(field: &str) -> Result<String> {
    let field = field.trim();
    if field == "*" || field.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(field.to_string());
    }

    let mut out = Vec::new();
    for item in field.split(',') {
        let item = item.trim();
        if let Some((range, step)) = item.split_once('/') {
            let step: i32 = step
                .parse()
                .map_err(|_| Error::other(format!("cron 步进值无效: '{item}'")))?;
            if step < 1 {
                return Err(Error::other(format!("cron 步进值必须为正: '{item}'")));
            }
            if range == "*" {
                // 标准环 0-6 到 cron crate 1-7 是线性偏移，`*/n` 结构不变。
                out.push(format!("*/{step}"));
                continue;
            }
            let vals = if let Some((a, b)) = range.split_once('-') {
                // 范围 + 步进：在展开后的序列上按 step 取样（如 1-5/2 → 1,3,5）。
                expand_dow_range(parse_dow(a, item)?, parse_dow(b, item)?)
                    .into_iter()
                    .step_by(step as usize)
                    .collect::<Vec<i32>>()
            } else {
                // 单值 + 步进（Vixie 语义，如 1/2 = 周一/三/五）：从起点在标准环上取样。
                let start = parse_dow(range, item)?;
                let start = if start == 7 { 0 } else { start };
                (start..=6).step_by(step as usize).collect()
            };
            out.push(join_dow(&vals));
        } else if let Some((a, b)) = item.split_once('-') {
            let (a, b) = (parse_dow(a, item)?, parse_dow(b, item)?);
            out.push(if a == b {
                map_dow(a).to_string()
            } else {
                join_dow(&expand_dow_range(a, b))
            });
        } else {
            out.push(map_dow(parse_dow(item, item)?).to_string());
        }
    }
    Ok(out.join(","))
}

fn parse_dow(v: &str, item: &str) -> Result<i32> {
    v.trim()
        .parse()
        .map_err(|_| Error::other(format!("cron 星期值无效: '{item}'")))
}

fn join_dow(values: &[i32]) -> String {
    values
        .iter()
        .map(|&v| map_dow(v).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 标准 5 段 → cron crate 7 段（`秒 分 时 日 月 周 年`）。
fn to_7field(expr: &str) -> Result<String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(Error::other(format!(
            "cron 表达式需要 5 段（分 时 日 月 周），实际得到 {} 段: '{expr}'",
            parts.len()
        )));
    }
    Ok(format!(
        "0 {} {} {} {} {} *",
        parts[0],
        parts[1],
        parts[2],
        parts[3],
        convert_dow(parts[4])?
    ))
}

/// 校验标准 5 段 cron 表达式是否合法，返回首个错误描述。
///
/// 必须走 5 段转换：直接把 5 段表达式交给 cron crate 会按 7 段解释
/// （`*/10 * * * *` 被当成「秒=*、分=*/10」），导致合法表达式被误判为非法。
pub fn validate(expr: &str) -> Result<()> {
    let schedule = CronSchedule::from_str(&to_7field(expr.trim())?)
        .map_err(|e| Error::other(format!("{e}")))?;
    if schedule.upcoming(Utc).next().is_none() {
        return Err(Error::other(format!(
            "cron 表达式没有可用的触发时间: '{expr}'"
        )));
    }
    Ok(())
}

/// 求表达式在 `after` 之后的下一个触发时刻（严格晚于 `after`）。
///
/// `tz` 为 IANA 时区名；解析失败时回退到系统本地时区。
pub fn next_due(expr: &str, tz: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let schedule = CronSchedule::from_str(&to_7field(expr.trim())?)
        .map_err(|e| Error::other(format!("cron 表达式解析失败 '{expr}': {e}")))?;
    let next = match tz.parse::<chrono_tz::Tz>() {
        Ok(zone) => schedule
            .after(&after.with_timezone(&zone))
            .next()
            .map(|t| t.with_timezone(&Utc)),
        Err(_) => schedule
            .after(&after.with_timezone(&Local))
            .next()
            .map(|t| t.with_timezone(&Utc)),
    };
    next.ok_or_else(|| Error::other(format!("cron 表达式没有可用的下一次触发时间: '{expr}'")))
}

/// 把 UTC 时间按指定 IANA 时区格式化为 `%Y-%m-%d %H:%M:%S`。时区无法解析时退回 UTC。
pub fn format_local(t: DateTime<Utc>, tz: &str) -> String {
    match tz.parse::<chrono_tz::Tz>() {
        Ok(zone) => t
            .with_timezone(&zone)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => t.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn converts_5field_to_7field() {
        assert_eq!(to_7field("0 9 * * 1-5").unwrap(), "0 0 9 * * 2,3,4,5,6 *");
        assert_eq!(to_7field("*/15 * * * *").unwrap(), "0 */15 * * * * *");
        assert_eq!(to_7field("30 8,20 * * 0,6").unwrap(), "0 30 8,20 * * 1,7 *");
        assert_eq!(to_7field("0 9 * * 7").unwrap(), "0 0 9 * * 1 *");
    }

    #[test]
    fn converts_cross_week_ranges() {
        assert_eq!(to_7field("0 9 * * 5-7").unwrap(), "0 0 9 * * 6,7,1 *");
        assert_eq!(to_7field("0 9 * * 6-1").unwrap(), "0 0 9 * * 7,1,2 *");
        assert_eq!(
            to_7field("0 9 * * 0-6").unwrap(),
            "0 0 9 * * 1,2,3,4,5,6,7 *"
        );
    }

    #[test]
    fn converts_steps() {
        assert_eq!(to_7field("0 9 * * 1/2").unwrap(), "0 0 9 * * 2,4,6 *");
        assert_eq!(to_7field("0 9 * * 1-5/2").unwrap(), "0 0 9 * * 2,4,6 *");
        assert_eq!(to_7field("0 9 * * 5-1/2").unwrap(), "0 0 9 * * 6,1 *");
    }

    #[test]
    fn rejects_bad_arity() {
        assert!(to_7field("0 9 * *").is_err());
        assert!(to_7field("0 9 * * 1 2 3").is_err());
        assert!(to_7field("").is_err());
    }

    #[test]
    fn next_due_daily() {
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(
            next_due("30 9 * * *", "UTC", after).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 1, 9, 30, 0).unwrap()
        );
    }

    #[test]
    fn next_due_rolls_to_next_day() {
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(
            next_due("30 9 * * *", "UTC", after).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 2, 9, 30, 0).unwrap()
        );
    }

    #[test]
    fn next_due_weekdays_only() {
        // 2026-01-01 是周四；周五 10:00 之后应跳到下周一。
        let thu = Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        assert_eq!(
            next_due("0 9 * * 1-5", "UTC", thu).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap()
        );
        let fri = Utc.with_ymd_and_hms(2026, 1, 2, 10, 0, 0).unwrap();
        assert_eq!(
            next_due("0 9 * * 1-5", "UTC", fri).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap()
        );
    }

    #[test]
    fn next_due_every_15_min() {
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 10, 7, 0).unwrap();
        assert_eq!(
            next_due("*/15 * * * *", "UTC", after).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 1, 10, 15, 0).unwrap()
        );
    }

    #[test]
    fn next_due_invalid_expr() {
        assert!(next_due("61 * * * *", "UTC", Utc::now()).is_err());
        assert!(next_due("bad", "UTC", Utc::now()).is_err());
    }
}
