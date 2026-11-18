mod commands;
mod db;
mod keys;
mod session;
mod sftp;
mod terminal;

use commands::{
    app_health, host_delete, host_list, host_save, known_host_delete, known_host_list, AppState,
};
use keys::{key_delete, key_import, key_list};
use session::SessionManager;
use sftp::{
    local_home, local_list, sftp_cancel, sftp_chmod, sftp_close, sftp_delete, sftp_download,
    sftp_list, sftp_mkdir, sftp_open, sftp_rename, sftp_upload, SftpManager,
};
use tauri::Manager;
use terminal::{term_close, term_open, term_resize, term_write, TerminalManager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("mangoshell".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let (pool, db_path) = tauri::async_runtime::block_on(db::init(&handle))
                .map_err(|e| e.to_string())?;
            app.manage(AppState {
                sessions: SessionManager::new(pool.clone()),
                terminals: TerminalManager::default(),
                sftp: SftpManager::default(),
                db: pool,
                db_path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_health,
            host_list,
            host_save,
            host_delete,
            known_host_list,
            known_host_delete,
            key_list,
            key_import,
            key_delete,
            term_open,
            term_write,
            term_resize,
            term_close,
            sftp_open,
            sftp_close,
            sftp_list,
            sftp_mkdir,
            sftp_rename,
            sftp_delete,
            sftp_chmod,
            sftp_download,
            sftp_upload,
            sftp_cancel,
            local_home,
            local_list
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
