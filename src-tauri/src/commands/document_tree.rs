use crate::services::document_tree::{DocumentTreeResult, DocumentTreeService};
use crate::state::AppState;

#[tauri::command]
pub fn document_tree_list(
    state: tauri::State<'_, AppState>,
    force_refresh: Option<bool>,
) -> Result<DocumentTreeResult, String> {
    DocumentTreeService::list(&state.db, &state.data_dir, force_refresh.unwrap_or(false))
        .map_err(|error| error.to_string())
}
