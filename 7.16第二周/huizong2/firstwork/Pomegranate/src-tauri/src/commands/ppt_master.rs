use crate::services::ppt_master::{
    PptMasterCheckInput, PptMasterCheckResult, PptMasterExportInput, PptMasterExportResult,
    PptMasterGenerateInput, PptMasterGenerateResult, PptMasterService,
};
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn ppt_master_check(input: PptMasterCheckInput) -> Result<PptMasterCheckResult, String> {
    PptMasterService::check(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ppt_master_export(input: PptMasterExportInput) -> Result<PptMasterExportResult, String> {
    PptMasterService::export(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ppt_master_generate_from_prompt(
    state: State<'_, AppState>,
    input: PptMasterGenerateInput,
) -> Result<PptMasterGenerateResult, String> {
    println!("[PPT Pipeline] command entered");
    PptMasterService::generate_from_prompt(&state.db, input)
        .await
        .map_err(|e| e.to_string())
}
