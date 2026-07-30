use crate::services::document_sources::{
    DocumentSource, DocumentSourceListInput, DocumentSourceListResult, DocumentSourceService,
};
use crate::state::AppState;

#[tauri::command]
pub fn document_source_list(
    state: tauri::State<'_, AppState>,
    input: Option<DocumentSourceListInput>,
) -> Result<DocumentSourceListResult, String> {
    DocumentSourceService::list(&state.db, &state.data_dir, input.unwrap_or_default())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn document_source_import_learning(
    state: tauri::State<'_, AppState>,
    source_path: String,
) -> Result<DocumentSource, String> {
    DocumentSourceService::import_learning_file(
        &state.db,
        &state.data_dir,
        std::path::Path::new(&source_path),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn document_source_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    DocumentSourceService::delete(&state.db, &state.data_dir, id).map_err(|error| error.to_string())
}
