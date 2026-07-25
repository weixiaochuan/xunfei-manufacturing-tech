use tauri::State;

use crate::models::{
    AdminProductModerationInput, AdminReviewInput, AdminVersionModerationInput, DeveloperDashboard,
    DeveloperEarning, DeveloperProduct, DeveloperProductInput, DeveloperProductVersion,
    DeveloperSubmitInput, DeveloperUploadPackageInput, DeveloperVersionInput, LocalAccountProfile,
    LocalAccountUpdateInput, MarketplaceActionResult, MarketplaceMockRole, MarketplaceMockSession,
    MarketplacePackageReport, MarketplaceReviewStatus, MarketplaceSubmission,
};
use crate::services::marketplace_supply::MarketplaceSupplyService;
use crate::state::AppState;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn current_session(state: &State<'_, AppState>) -> Result<MarketplaceMockSession, String> {
    MarketplaceSupplyService::current_session(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_list_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<LocalAccountProfile>, String> {
    MarketplaceSupplyService::list_accounts(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_get_mock_session(
    state: State<'_, AppState>,
) -> Result<MarketplaceMockSession, String> {
    MarketplaceSupplyService::ensure_local_users(&state.db).map_err(|e| e.to_string())?;
    current_session(&state)
}

#[tauri::command]
pub fn marketplace_switch_mock_role(
    state: State<'_, AppState>,
    role: MarketplaceMockRole,
) -> Result<MarketplaceMockSession, String> {
    if !cfg!(debug_assertions) {
        return Err("模拟角色切换仅开发构建可用".to_string());
    }
    MarketplaceSupplyService::ensure_local_users(&state.db).map_err(|e| e.to_string())?;
    let next = MarketplaceSupplyService::session_for_role(role);
    let next = MarketplaceSupplyService::set_current_user(&state.db, &next.user_id)
        .map_err(|e| e.to_string())?;
    let mut guard = state
        .marketplace_session
        .lock()
        .map_err(|_| "marketplace session lock poisoned".to_string())?;
    *guard = next.clone();
    Ok(next)
}

#[tauri::command]
pub fn marketplace_switch_account(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<MarketplaceMockSession, String> {
    if !cfg!(debug_assertions) {
        return Err("本地演示账号切换仅开发构建可用".to_string());
    }
    let next = MarketplaceSupplyService::set_current_user(&state.db, &user_id)
        .map_err(|e| e.to_string())?;
    let mut guard = state
        .marketplace_session
        .lock()
        .map_err(|_| "marketplace session lock poisoned".to_string())?;
    *guard = next.clone();
    Ok(next)
}

#[tauri::command]
pub fn marketplace_update_account(
    state: State<'_, AppState>,
    input: LocalAccountUpdateInput,
) -> Result<MarketplaceMockSession, String> {
    let session = current_session(&state)?;
    let next = MarketplaceSupplyService::update_account(&state.db, &session, input)
        .map_err(|e| e.to_string())?;
    let mut guard = state
        .marketplace_session
        .lock()
        .map_err(|_| "marketplace session lock poisoned".to_string())?;
    *guard = next.clone();
    Ok(next)
}

#[tauri::command]
pub fn marketplace_apply_developer(
    state: State<'_, AppState>,
) -> Result<MarketplaceMockSession, String> {
    let session = current_session(&state)?;
    let next = MarketplaceSupplyService::apply_developer(&state.db, &session)
        .map_err(|e| e.to_string())?;
    let mut guard = state
        .marketplace_session
        .lock()
        .map_err(|_| "marketplace session lock poisoned".to_string())?;
    *guard = next.clone();
    Ok(next)
}

#[tauri::command]
pub fn developer_list_products(
    state: State<'_, AppState>,
) -> Result<Vec<DeveloperProduct>, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_list_products(&state.db, &session)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_create_product(
    state: State<'_, AppState>,
    input: DeveloperProductInput,
) -> Result<DeveloperProduct, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_create_product(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_update_product(
    state: State<'_, AppState>,
    product_id: String,
    input: DeveloperProductInput,
) -> Result<DeveloperProduct, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_update_product(&state.db, &session, &product_id, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_create_version(
    state: State<'_, AppState>,
    input: DeveloperVersionInput,
) -> Result<DeveloperProductVersion, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_create_version(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_upload_package(
    state: State<'_, AppState>,
    input: DeveloperUploadPackageInput,
) -> Result<MarketplacePackageReport, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_upload_package(
        &state.db,
        &state.data_dir,
        &session,
        input,
        APP_VERSION,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_get_package_report(
    state: State<'_, AppState>,
    product_id: String,
    version: String,
) -> Result<MarketplacePackageReport, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_get_package_report(
        &state.db,
        &session,
        &product_id,
        &version,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_submit_product(
    state: State<'_, AppState>,
    input: DeveloperSubmitInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_submit_product(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_submit_version(
    state: State<'_, AppState>,
    input: DeveloperSubmitInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_submit_product(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_list_earnings(
    state: State<'_, AppState>,
) -> Result<Vec<DeveloperEarning>, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_list_earnings(&state.db, &session)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn developer_get_dashboard(state: State<'_, AppState>) -> Result<DeveloperDashboard, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::developer_dashboard(&state.db, &session).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_list_submissions(
    state: State<'_, AppState>,
    status: Option<MarketplaceReviewStatus>,
) -> Result<Vec<MarketplaceSubmission>, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_list_submissions(&state.db, &session, status)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_get_submission(
    state: State<'_, AppState>,
    submission_id: i64,
) -> Result<MarketplaceSubmission, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_get_submission(&state.db, &session, submission_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_start_review(
    state: State<'_, AppState>,
    input: AdminReviewInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_start_review(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_approve_submission(
    state: State<'_, AppState>,
    input: AdminReviewInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_approve_submission(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_reject_submission(
    state: State<'_, AppState>,
    input: AdminReviewInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_reject_submission(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_suspend_product(
    state: State<'_, AppState>,
    input: AdminProductModerationInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_suspend_product(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_restore_product(
    state: State<'_, AppState>,
    input: AdminProductModerationInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_restore_product(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_delist_product(
    state: State<'_, AppState>,
    input: AdminProductModerationInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_delist_product(&state.db, &session, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn admin_revoke_version(
    state: State<'_, AppState>,
    input: AdminVersionModerationInput,
) -> Result<MarketplaceActionResult, String> {
    let session = current_session(&state)?;
    MarketplaceSupplyService::admin_revoke_version(&state.db, &session, input)
        .map_err(|e| e.to_string())
}
