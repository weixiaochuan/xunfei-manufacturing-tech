use crate::services::ppt_master::{
    resolve_generation_route, PptGenerationRoute, PptMasterCheckInput, PptMasterCheckResult,
    PptMasterExportInput, PptMasterExportResult, PptMasterGenerateInput, PptMasterGenerateResult,
    PptMasterService,
};
use crate::state::AppState;
use std::time::Instant;
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
    let route = resolve_generation_route(
        input.generation_engine.as_deref(),
        input.generation_mode.as_deref(),
    )
    .unwrap_or_else(|_| {
        if input.generation_engine.as_deref() == Some("ppt_master_native")
            || input.generation_mode.as_deref() == Some("agent")
        {
            PptGenerationRoute::PptMasterNative
        } else {
            PptGenerationRoute::LegacyFallback
        }
    });
    let started = Instant::now();
    match PptMasterService::generate_from_prompt(&state.db, input).await {
        Ok(result) => Ok(result),
        Err(error) => Ok(PptMasterGenerateResult::failure(
            error.to_string(),
            route.generation_mode().to_string(),
            route.generation_engine().to_string(),
            started.elapsed().as_millis(),
        )),
    }
}
