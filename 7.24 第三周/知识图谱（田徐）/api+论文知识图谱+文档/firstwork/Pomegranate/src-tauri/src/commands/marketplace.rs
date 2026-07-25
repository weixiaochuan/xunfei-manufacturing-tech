use tauri::State;

use crate::models::{
    MarketplaceAcquireInput, MarketplaceActionResult, MarketplaceEntitlement,
    MarketplaceExternalAuthorizationInput, MarketplaceInstallInput, MarketplaceLedgerEntry,
    MarketplaceMockTestResult, MarketplaceOrder, MarketplacePermissionRejectionInput,
    MarketplaceProductDetail, MarketplaceProductQuery, MarketplaceProductSummary,
    MarketplaceRefundInput, MarketplaceReviewInfo, MarketplaceReviewInput,
    MarketplaceServiceConfigurationInput, MarketplaceUpdateInfo, MarketplaceUpdateInput,
    NormalizedPluginManifest,
};
use crate::services::marketplace::MarketplaceService;
use crate::state::AppState;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tauri::command]
pub fn marketplace_list_products(
    state: State<'_, AppState>,
    query: Option<MarketplaceProductQuery>,
) -> Result<Vec<MarketplaceProductSummary>, String> {
    MarketplaceService::list_products(&state.db, &state.data_dir, query.unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_search_products(
    state: State<'_, AppState>,
    query: MarketplaceProductQuery,
) -> Result<Vec<MarketplaceProductSummary>, String> {
    MarketplaceService::list_products(&state.db, &state.data_dir, query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_get_product(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<MarketplaceProductDetail, String> {
    MarketplaceService::get_product(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_get_product_version(
    state: State<'_, AppState>,
    product_id: String,
    version: Option<String>,
) -> Result<NormalizedPluginManifest, String> {
    MarketplaceService::get_product_version(&state.db, &state.data_dir, &product_id, version)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_acquire_product(
    state: State<'_, AppState>,
    input: MarketplaceAcquireInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::acquire_product(&state.db, &state.data_dir, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_bind_external_authorization(
    state: State<'_, AppState>,
    input: MarketplaceExternalAuthorizationInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::bind_external_authorization(&state.db, &state.data_dir, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_list_entitlements(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceEntitlement>, String> {
    MarketplaceService::list_entitlements(&state.db, &state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_list_orders(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceOrder>, String> {
    MarketplaceService::list_orders(&state.db, &state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_list_ledger(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceLedgerEntry>, String> {
    MarketplaceService::list_ledger(&state.db, &state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_request_refund(
    state: State<'_, AppState>,
    input: MarketplaceRefundInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::request_refund(&state.db, &state.data_dir, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_list_reviews(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<Vec<MarketplaceReviewInfo>, String> {
    MarketplaceService::list_reviews(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_submit_review(
    state: State<'_, AppState>,
    input: MarketplaceReviewInput,
) -> Result<MarketplaceReviewInfo, String> {
    MarketplaceService::submit_review(&state.db, &state.data_dir, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_install_product(
    state: State<'_, AppState>,
    input: MarketplaceInstallInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::install_product(&state.db, &state.data_dir, input, APP_VERSION)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_update_product(
    state: State<'_, AppState>,
    input: MarketplaceUpdateInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::update_product(&state.db, &state.data_dir, input, APP_VERSION)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_uninstall_product(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<MarketplaceActionResult, String> {
    if let Ok(detail) = MarketplaceService::get_product(&state.db, &state.data_dir, &product_id) {
        let _ = state.plugin_tokens.revoke(&detail.summary.plugin_id);
    }
    MarketplaceService::uninstall_product(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_enable_product(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::enable_product(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_disable_product(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::disable_product(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_record_permission_rejection(
    state: State<'_, AppState>,
    input: MarketplacePermissionRejectionInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::record_permission_rejection(
        &state.db,
        &state.data_dir,
        &input.product_id,
        &input.action,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_configure_service(
    state: State<'_, AppState>,
    input: MarketplaceServiceConfigurationInput,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::configure_service(&state.db, &state.data_dir, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_list_installed(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceProductSummary>, String> {
    MarketplaceService::list_installed(&state.db, &state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_check_updates(
    state: State<'_, AppState>,
) -> Result<Vec<MarketplaceUpdateInfo>, String> {
    MarketplaceService::check_updates(&state.db, &state.data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_verify_installation(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::verify_installation(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_dev_revoke_product_version(
    state: State<'_, AppState>,
    product_id: String,
    version: Option<String>,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::dev_revoke_product_version(&state.db, &state.data_dir, &product_id, version)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_dev_restore_product_version(
    state: State<'_, AppState>,
    product_id: String,
    version: Option<String>,
) -> Result<MarketplaceActionResult, String> {
    MarketplaceService::dev_restore_product_version(
        &state.db,
        &state.data_dir,
        &product_id,
        version,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn marketplace_mock_test_product(
    state: State<'_, AppState>,
    product_id: String,
) -> Result<MarketplaceMockTestResult, String> {
    MarketplaceService::mock_test_product(&state.db, &state.data_dir, &product_id)
        .map_err(|e| e.to_string())
}
