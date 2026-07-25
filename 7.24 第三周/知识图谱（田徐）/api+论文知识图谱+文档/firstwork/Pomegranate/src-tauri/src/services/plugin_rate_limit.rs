//! 插件写操作速率限制器
//!
//! 防止插件爆库：每个插件每秒最多 10 次写操作。
//! 阈值可通过环境变量 PLUGIN_WRITE_RATE_LIMIT 调整。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::error::AppError;

const DEFAULT_MAX_WRITES_PER_SECOND: u32 = 10;

struct RateLimitState {
    window_start: Instant,
    count: u32,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            window_start: Instant::now(),
            count: 0,
        }
    }
}

pub struct PluginRateLimiter {
    inner: Mutex<HashMap<String, RateLimitState>>,
    max_per_second: u32,
}

impl PluginRateLimiter {
    pub fn new() -> Self {
        let max_per_second = std::env::var("PLUGIN_WRITE_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_WRITES_PER_SECOND);
        log::info!("[rate_limit] 插件写入速率上限: {} 次/秒", max_per_second);
        Self {
            inner: Mutex::new(HashMap::new()),
            max_per_second,
        }
    }

    /// 检查插件写操作是否超限。未超限 → Ok(()); 超限 → Err(AppError::Custom(...))
    pub fn check_write(&self, plugin_id: &str) -> Result<(), AppError> {
        self.check_window(
            plugin_id,
            "write",
            self.max_per_second,
            1,
            "插件写入速率超限",
        )
    }

    /// 检查插件 AI 调用是否超限：每插件每分钟最多 10 次。
    pub fn check_ai(&self, plugin_id: &str) -> Result<(), AppError> {
        self.check_window(plugin_id, "ai", 10, 60, "插件 AI 调用速率超限")
    }

    fn check_window(
        &self,
        plugin_id: &str,
        scope: &str,
        max_count: u32,
        window_secs: u64,
        label: &str,
    ) -> Result<(), AppError> {
        let mut map = self
            .inner
            .lock()
            .map_err(|e| AppError::Custom(format!("速率限制器锁错误: {}", e)))?;
        let key = format!("{}:{}", scope, plugin_id);
        let state = map.entry(key).or_insert_with(|| RateLimitState {
            window_start: Instant::now(),
            count: 0,
        });
        if state.window_start.elapsed().as_secs() >= window_secs {
            state.window_start = Instant::now();
            state.count = 0;
        }
        if state.count >= max_count {
            return Err(AppError::Custom(format!(
                "{}（每 {} 秒最多 {} 次）",
                label, window_secs, max_count
            )));
        }
        state.count += 1;
        Ok(())
    }
}

impl Default for PluginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 单元测试 ────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter_with_limit(max_per_second: u32) -> PluginRateLimiter {
        PluginRateLimiter {
            inner: Mutex::new(HashMap::new()),
            max_per_second,
        }
    }

    #[test]
    fn test_allow_writes_within_limit() {
        let limiter = limiter_with_limit(3);
        let pid = "plugin-a";
        for _ in 0..3 {
            assert!(limiter.check_write(pid).is_ok());
        }
    }

    #[test]
    fn test_exceed_rate_limit() {
        let limiter = limiter_with_limit(2);
        let pid = "plugin-b";
        assert!(limiter.check_write(pid).is_ok());
        assert!(limiter.check_write(pid).is_ok());
        // 第 3 次超限
        let err = limiter.check_write(pid).unwrap_err();
        assert!(err.to_string().contains("速率超限"));
    }

    #[test]
    fn test_different_plugins_independent() {
        let limiter = limiter_with_limit(1);
        assert!(limiter.check_write("plugin-x").is_ok());
        // 不同插件不受对方限制影响
        assert!(limiter.check_write("plugin-y").is_ok());
        // plugin-x 第 2 次应超限
        assert!(limiter.check_write("plugin-x").is_err());
    }

    #[test]
    fn test_allow_after_window_reset() {
        let limiter = limiter_with_limit(1);
        let pid = "plugin-c";
        assert!(limiter.check_write(pid).is_ok());
        assert!(limiter.check_write(pid).is_err());
        // 等 1 秒让时间窗口重置
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert!(limiter.check_write(pid).is_ok());
    }
}
