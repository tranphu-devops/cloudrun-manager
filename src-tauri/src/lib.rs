pub mod audit;
pub mod commands;
pub mod config;
pub mod error;
pub mod state;
pub mod vault;

use tauri::Manager;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Thư mục cấu hình và dữ liệu do OS cấp (trên Windows là %APPDATA%\<identifier>).
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;

            let state = AppState::new(config_dir, data_dir).map_err(boxed)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // project / auth / cấu hình
            commands::projects::auth_info,
            commands::projects::list_projects,
            commands::projects::get_settings,
            commands::projects::set_read_only,
            commands::projects::set_project_label,
            commands::projects::set_preferences,
            commands::projects::check_permissions,
            commands::projects::select_project,
            commands::projects::verify_metrics,
            commands::projects::audit_tail,
            commands::projects::audit_path,
            commands::projects::clear_cache,
            // service
            commands::services::list_services,
            commands::services::get_service,
            commands::services::list_revisions,
            commands::services::refresh_project,
            commands::services::project_load,
            // ghi
            commands::mutate::preview_env,
            commands::mutate::apply_env,
            commands::mutate::preview_scaling,
            commands::mutate::apply_scaling,
            // metric
            commands::metrics::service_charts,
            // log
            commands::logs::fetch_logs,
            commands::logs::log_explorer_url,
            // credential vault (v2)
            commands::auth::vault_status,
            commands::auth::import_service_account,
            commands::auth::unlock_vault,
            commands::auth::lock_vault,
            commands::auth::remove_credential,
            commands::auth::set_allowed_projects,
            // jobs (v2)
            commands::jobs::jobs_overview,
            commands::jobs::refresh_jobs,
            commands::jobs::get_job,
            commands::jobs::run_job,
            commands::jobs::set_schedule_paused,
            // chi phí + recommendation (v2)
            commands::insights::cost_report,
            commands::insights::recommendations,
            commands::insights::mark_recommendation,
            // secret
            commands::secrets::list_secrets,
            commands::secrets::list_secret_versions,
            commands::secrets::reveal_secret,
        ])
        .run(tauri::generate_context!())
        .expect("không khởi động được Cloud Run Cockpit");
}

/// `setup` trả `Box<dyn Error>`; gói chuỗi lỗi lại cho gọn.
fn boxed(msg: String) -> Box<dyn std::error::Error> {
    #[derive(Debug)]
    struct E(String);
    impl std::fmt::Display for E {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for E {}
    Box::new(E(msg))
}
