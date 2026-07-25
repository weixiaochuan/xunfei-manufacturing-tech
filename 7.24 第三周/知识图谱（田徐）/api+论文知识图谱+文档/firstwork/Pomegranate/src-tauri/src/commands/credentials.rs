use tauri::State;

use crate::models::{
    CredentialCreateInput, CredentialInfo, CredentialUpdateInput, CredentialUsage,
};
use crate::services::credentials::CredentialService;
use crate::state::AppState;

#[tauri::command]
pub fn credential_list(state: State<'_, AppState>) -> Result<Vec<CredentialInfo>, String> {
    CredentialService::list(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn credential_create(
    state: State<'_, AppState>,
    input: CredentialCreateInput,
) -> Result<CredentialInfo, String> {
    CredentialService::create(&state.db, &state.data_dir, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn credential_update(
    state: State<'_, AppState>,
    id: String,
    input: CredentialUpdateInput,
) -> Result<CredentialInfo, String> {
    CredentialService::update(&state.db, &state.data_dir, &id, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn credential_delete(
    state: State<'_, AppState>,
    id: String,
    force: Option<bool>,
) -> Result<(), String> {
    CredentialService::delete(&state.db, &state.data_dir, &id, force.unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn credential_get_usage(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<CredentialUsage>, String> {
    CredentialService::usage(&state.db, &id).map_err(|e| e.to_string())
}
