//! 插件运行时 Command：令牌生命周期

use sha2::{Digest, Sha256};
use std::fs;

use crate::state::AppState;
use tauri::State;

/// 申领插件令牌
///
/// 校验：
/// 1. plugin_id 在 plugins 表存在
/// 2. status = "installed"
/// 3. enabled = true
/// 4. T26: main.js 未被篡改（content_hash 非空时对比 SHA-256）
#[tauri::command]
pub fn plugin_acquire_token(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<String, String> {
    let plugin = state
        .db
        .get_plugin(&plugin_id)
        .map_err(|e| e.to_string())?
        ;

    if plugin.status != "installed" {
        return Err(format!("插件 {} 状态非 installed", plugin_id));
    }
    if !plugin.enabled {
        return Err(format!("插件 {} 已禁用", plugin_id));
    }

    // T26: 完整性校验 — content_hash 非空时对比当前 main.js 的 SHA-256
    let stored_hash: Option<String> = state
        .db
        .get_plugin_content_hash(&plugin_id)
        .map_err(|e| e.to_string())?;
    if let Some(ref expected) = stored_hash {
        if !expected.is_empty() {
            let main_path = std::path::Path::new(&plugin.path).join(&plugin.main);
            let bytes = fs::read(&main_path).map_err(|e| e.to_string())?;
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if actual != *expected {
                return Err(format!(
                    "插件 {} 的 main.js 已被人篡改，拒绝激活。期望 hash={}，实际={}",
                    plugin_id,
                    &expected[..8], // 只显示前 8 位，不泄露完整 hash
                    &actual[..8],
                ));
            }
        }
    }

    state.plugin_tokens.acquire(&plugin_id)
}

/// 作废插件令牌
#[tauri::command]
pub fn plugin_revoke_token(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<(), String> {
    state.plugin_tokens.revoke(&plugin_id)
}
