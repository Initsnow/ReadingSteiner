//! 全局设置领域服务：读取、校验并保存（保存即热更新）。

use std::str::FromStr;

use crate::config::EditableSettings;
use crate::error::{Error, Result};
use crate::scheduler::AppState;

/// 读取全局设置。数据库迁移时已 seed 默认值，恒有值。
pub async fn settings_get(state: &AppState) -> Result<EditableSettings> {
    state
        .db
        .lock()
        .await
        .get_settings()?
        .ok_or_else(|| Error::other("settings not seeded"))
}

/// 保存全局设置并热更新到 runtime / notifier。
///
/// 先校验后落库：非法值直接拒绝，避免坏值入库后反复影响运行。
pub async fn settings_update(state: &AppState, settings: EditableSettings) -> Result<()> {
    validate(&settings)?;
    state.db.lock().await.set_settings(&settings)?;
    state.reload_settings(&settings);
    Ok(())
}

/// 校验全局设置的合法值，返回首个错误。
fn validate(s: &EditableSettings) -> Result<()> {
    if s.concurrency == 0 {
        return Err(Error::config("concurrency 必须大于 0"));
    }
    if s.queue_capacity == 0 {
        return Err(Error::config("queue_capacity 必须大于 0"));
    }
    if s.default_timeout_secs == 0 {
        return Err(Error::config("default_timeout_secs 必须大于 0"));
    }
    // 用标准 5 段解析器校验：直接交给 cron crate 会把 5 段表达式按 7 段解释
    // （`*/10 * * * *` 被当成「秒=*、分=*/10」而误判为非法）。
    if !s.default_cron.trim().is_empty()
        && let Err(e) = crate::cron_expr::validate(&s.default_cron)
    {
        return Err(Error::config(format!(
            "default_cron 不是合法的 cron 表达式: {e}"
        )));
    }
    // 非空 timezone 必须是合法 IANA 时区名，避免坏值入库后调度/通知渲染静默回退 UTC。
    // 系统本地时区字符串作为例外放行（Windows 上 iana_time_zone 可能返回非 IANA 名称），
    // 否则默认种子值在「取回再保存」时会被误判为非法。
    if !s.timezone.trim().is_empty()
        && chrono_tz::Tz::from_str(&s.timezone).is_err()
        && s.timezone != crate::config::system_local_timezone()
    {
        return Err(Error::config(format!(
            "timezone 不是合法的 IANA 时区: {}",
            s.timezone
        )));
    }
    // 非空 telegram_url 必须可解析，避免非法 URL 在热更新重建 notifier 时静默关掉通知。
    if !s.telegram_url.trim().is_empty()
        && crate::config::parse_telegram_url(&s.telegram_url).is_err()
    {
        return Err(Error::config(format!(
            "telegram_url 不是合法的 tgram:// 通知目标: {}",
            s.telegram_url
        )));
    }
    Ok(())
}
