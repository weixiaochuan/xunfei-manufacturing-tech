use crate::services::source_writeback::{WriteBackResult, WriteBackService};
use crate::state::AppState;

/// 手动触发把笔记写回原 .md 文件
///
/// - 默认 `force=false`：检测到外部 mtime 变化会返回 `WriteBackResult::Conflict`，
///   前端据此弹冲突 Modal。
/// - `force=true`：强制覆盖，配合冲突 Modal 中"覆盖外部"按钮使用。
///
/// 笔记在保存时（`update_note`）会自动调用一次（force=false），所以前端通常只在
/// 冲突解决时才主动调用 `force=true`，或在设置页提供"立即同步"按钮。
#[tauri::command]
pub fn write_back_source_md(
    state: tauri::State<'_, AppState>,
    note_id: i64,
    force: bool,
) -> Result<WriteBackResult, String> {
    WriteBackService::write_back(&state.db, note_id, force).map_err(|e| e.to_string())
}
