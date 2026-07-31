use tauri::State;

use crate::account::AccountState;
use crate::models::{
    CredentialCreateInput, CredentialInfo, CredentialUpdateInput, CredentialUsage,
};
use crate::services::credentials::CredentialService;
use crate::services::resource_ownership::resolve_resource_owner;
use crate::state::AppState;

#[tauri::command]
pub async fn credential_list(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
) -> Result<Vec<CredentialInfo>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    CredentialService::list(&state.db, &owner).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn credential_create(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: CredentialCreateInput,
) -> Result<CredentialInfo, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    CredentialService::create(&state.db, &state.data_dir, &owner, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn credential_update(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
    input: CredentialUpdateInput,
) -> Result<CredentialInfo, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    CredentialService::update(&state.db, &state.data_dir, &owner, &id, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn credential_delete(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
    force: Option<bool>,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    CredentialService::delete(
        &state.db,
        &state.data_dir,
        &owner,
        &id,
        force.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn credential_get_usage(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
) -> Result<Vec<CredentialUsage>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    CredentialService::usage(&state.db, &owner, &id).map_err(|e| e.to_string())
}
