//! 插件运行时令牌注册表
//!
//! 设计要点：
//! - 内存存储（Mutex<HashMap>），进程退出自动清空
//! - 每次 acquire 生成新 UUID，覆盖旧令牌（轮换）
//! - 反查表 token → plugin_id 由 O(n) 遍历（插件总数 < 100）

use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
pub struct PluginTokenRegistry {
    /// pluginId → token
    tokens: Mutex<HashMap<String, String>>,
}

impl PluginTokenRegistry {
    /// 申领新令牌（覆盖旧令牌）
    pub fn acquire(&self, plugin_id: &str) -> Result<String, String> {
        let token = Uuid::new_v4().to_string();
        let mut guard = self.tokens.lock().map_err(|e| e.to_string())?;
        guard.insert(plugin_id.to_string(), token.clone());
        Ok(token)
    }

    /// 作废令牌（幂等）
    pub fn revoke(&self, plugin_id: &str) -> Result<(), String> {
        let mut guard = self.tokens.lock().map_err(|e| e.to_string())?;
        guard.remove(plugin_id);
        Ok(())
    }

    /// 通过令牌反查 plugin_id；找不到 → None
    pub fn lookup(&self, token: &str) -> Result<Option<String>, String> {
        let guard = self.tokens.lock().map_err(|e| e.to_string())?;
        Ok(guard
            .iter()
            .find(|(_, t)| t.as_str() == token)
            .map(|(pid, _)| pid.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn test_acquire_returns_uuid() {
        let registry = PluginTokenRegistry::default();
        let token = registry.acquire("cc").unwrap();
        assert_eq!(token.len(), 36);
        assert!(token.contains('-'));
    }

    #[test]
    fn test_acquire_overwrites_old() {
        let registry = PluginTokenRegistry::default();
        let t1 = registry.acquire("cc").unwrap();
        let t2 = registry.acquire("cc").unwrap();
        assert_ne!(t1, t2);
        assert_eq!(registry.lookup(&t1).unwrap(), None);
        assert_eq!(registry.lookup(&t2).unwrap(), Some("cc".to_string()));
    }

    #[test]
    fn test_revoke_idempotent() {
        let registry = PluginTokenRegistry::default();
        registry.acquire("cc").unwrap();
        registry.revoke("cc").unwrap();
        registry.revoke("cc").unwrap(); // 第二次不应报错
    }

    #[test]
    fn test_lookup_invalid_token() {
        let registry = PluginTokenRegistry::default();
        assert_eq!(registry.lookup("invalid").unwrap(), None);
    }

    // ─── §10.15 越权测试用例（直接针对 PluginTokenRegistry 层） ───

    /// 用例 1：无效令牌反查 → None（verify() 会据此返回 PluginPermissionDenied）
    #[test]
    fn case_1_invalid_token_returns_none() {
        let registry = PluginTokenRegistry::default();
        registry.acquire("cc").unwrap();
        assert_eq!(registry.lookup("random-junk").unwrap(), None);
    }

    /// 用例 2：空令牌 → None
    #[test]
    fn case_2_empty_token_returns_none() {
        let registry = PluginTokenRegistry::default();
        registry.acquire("cc").unwrap();
        assert_eq!(registry.lookup("").unwrap(), None);
    }

    /// 用例 3：撤销后的令牌反查 → None
    #[test]
    fn case_3_revoked_token_returns_none() {
        let registry = PluginTokenRegistry::default();
        let token = registry.acquire("cc").unwrap();
        registry.revoke("cc").unwrap();
        assert_eq!(registry.lookup(&token).unwrap(), None);
    }

    /// 用例 4：轮换后旧令牌失效，新令牌生效
    #[test]
    fn case_4_rotated_token_old_invalid() {
        let registry = PluginTokenRegistry::default();
        let old = registry.acquire("cc").unwrap();
        let new = registry.acquire("cc").unwrap();
        assert_ne!(old, new);
        assert_eq!(registry.lookup(&old).unwrap(), None, "旧令牌应失效");
        assert_eq!(
            registry.lookup(&new).unwrap(),
            Some("cc".to_string()),
            "新令牌应生效"
        );
    }

    /// 用例 10：令牌不应包含可识别的明文（仅 UUID v4 格式）
    /// 通过验证格式间接保证：插件 ID 不会泄漏到令牌中
    #[test]
    fn case_10_token_format_no_plugin_id_leak() {
        let registry = PluginTokenRegistry::default();
        let token = registry.acquire("secret-plugin-id").unwrap();
        assert!(
            !token.contains("secret-plugin-id"),
            "令牌不应包含插件 ID 明文"
        );
        // UUID v4 格式：8-4-4-4-12
        let parts: Vec<&str> = token.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
    }

    /// 用例 11：并发申领同一 plugin_id，后到者覆盖
    #[test]
    fn case_11_concurrent_acquire_last_wins() {
        let registry = Arc::new(PluginTokenRegistry::default());
        let mut handles = vec![];

        // 10 个线程并发对同一 plugin_id 申领
        for _ in 0..10 {
            let r = Arc::clone(&registry);
            handles.push(thread::spawn(move || r.acquire("cc").unwrap()));
        }

        let tokens: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        // 全部唯一
        let unique: std::collections::HashSet<_> = tokens.iter().collect();
        assert_eq!(unique.len(), 10, "10 次申领应产生 10 个唯一令牌");

        // 最终注册表中只有一个有效令牌（最后写入的那个）
        let mut valid_count = 0;
        for t in &tokens {
            if registry.lookup(t).unwrap().is_some() {
                valid_count += 1;
            }
        }
        assert_eq!(valid_count, 1, "并发申领后只应有 1 个令牌有效");
    }

    /// 用例 12：大量插件场景下 lookup 性能（O(n) 实现，1000 个插件应 < 10ms）
    #[test]
    fn case_12_large_scale_lookup_performance() {
        let registry = PluginTokenRegistry::default();
        let mut last_token = String::new();
        for i in 0..1000 {
            last_token = registry.acquire(&format!("plugin-{}", i)).unwrap();
        }

        // 测最坏情况：查找最后一个插入的（HashMap 顺序不保证，但 1000 量级延迟均极小）
        let start = Instant::now();
        for _ in 0..100 {
            let _ = registry.lookup(&last_token).unwrap();
        }
        let elapsed = start.elapsed();
        let per_call = elapsed / 100;
        assert!(
            per_call.as_millis() < 10,
            "lookup 平均耗时 {:?} 超过 10ms",
            per_call
        );
    }
}
