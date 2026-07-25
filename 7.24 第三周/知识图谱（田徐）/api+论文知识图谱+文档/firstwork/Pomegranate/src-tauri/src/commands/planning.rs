use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::models::{
    PlanningApplyUpdateInput, PlanningClearInput, PlanningExportInput, PlanningSaveFileInput,
    PlanningSessionInput, PlanningSetEnabledInput, PlanningWorkspace,
};
use crate::services::planning::PlanningService;
use crate::state::AppState;

#[tauri::command]
pub fn planning_get_workspace(
    state: State<'_, AppState>,
    input: PlanningSessionInput,
) -> Result<PlanningWorkspace, String> {
    PlanningService::get_workspace(
        &state.db,
        &state.data_dir,
        input.session_kind,
        &input.session_id,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn planning_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PlanningSetEnabledInput,
) -> Result<PlanningWorkspace, String> {
    let workspace = PlanningService::set_enabled(
        &state.db,
        &state.data_dir,
        input.session_kind,
        &input.session_id,
        input.enabled,
    )
    .map_err(|e| e.to_string())?;
    PlanningService::emit_workspace_updated(&app, &workspace, vec!["state".into()]);
    Ok(workspace)
}

#[tauri::command]
pub fn planning_save_file(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PlanningSaveFileInput,
) -> Result<PlanningWorkspace, String> {
    let workspace = PlanningService::save_file(
        &state.db,
        &state.data_dir,
        input.session_kind,
        &input.session_id,
        &input.file_name,
        &input.content,
    )
    .map_err(|e| e.to_string())?;
    PlanningService::emit_workspace_updated(&app, &workspace, vec![input.file_name]);
    Ok(workspace)
}

#[tauri::command]
pub fn planning_apply_update(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PlanningApplyUpdateInput,
) -> Result<PlanningWorkspace, String> {
    let workspace = PlanningService::apply_update(
        &state.db,
        &state.data_dir,
        input.session_kind,
        &input.session_id,
        input.accept,
    )
    .map_err(|e| e.to_string())?;
    PlanningService::emit_workspace_updated(
        &app,
        &workspace,
        vec!["plan".into(), "findings".into(), "progress".into()],
    );
    Ok(workspace)
}

#[tauri::command]
pub fn planning_clear(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PlanningClearInput,
) -> Result<PlanningWorkspace, String> {
    let workspace = PlanningService::clear(
        &state.db,
        &state.data_dir,
        input.session_kind,
        &input.session_id,
        input.confirm,
    )
    .map_err(|e| e.to_string())?;
    PlanningService::emit_workspace_updated(
        &app,
        &workspace,
        vec!["plan".into(), "findings".into(), "progress".into()],
    );
    Ok(workspace)
}

#[tauri::command]
pub fn planning_export(
    state: State<'_, AppState>,
    input: PlanningExportInput,
) -> Result<(), String> {
    PlanningService::export(
        &state.db,
        &state.data_dir,
        input.session_kind,
        &input.session_id,
        &PathBuf::from(input.target_dir),
    )
    .map_err(|e| e.to_string())
}
