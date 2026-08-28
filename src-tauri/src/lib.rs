include!("app_impl.rs");

const COMPANION_LAUNCH_ARG: &str = "--companion";

fn companion_launch_requested(args: &[String]) -> bool {
    args.iter().any(|arg| arg == COMPANION_LAUNCH_ARG)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if extension_setup_requested(&argv) {
                queue_extension_setup_request();
                let _ = app.emit("open-extension-setup", ());
            }
            if !companion_launch_requested(&argv) {
                let _ = show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "windows")]
            if platform::windows::setup_windows_tray(app).is_err() {
                eprintln!("mediadrop: tray_setup_failed");
            }
            let launch_args = std::env::args().collect::<Vec<_>>();
            if extension_setup_requested(&launch_args) {
                queue_extension_setup_request();
            }
            if !companion_launch_requested(&launch_args) {
                show_main_window(app.handle())?;
            }
            #[cfg(target_os = "windows")]
            if companion::windows_pipe::start_server(app.handle().clone()).is_err() {
                eprintln!("mediadrop: companion_pipe_start_failed");
            }
            let _ = cleanup_stale_download_job_dirs(Duration::from_secs(60 * 60));
            let _ = cleanup_owned_temp_artifacts(
                &std::env::temp_dir(),
                &[
                    "mediadrop-instagram-cookie-",
                    "mediadrop-instagram-cookies-",
                    "mediadrop-instagram-prepared-",
                    "mediadrop-instagram-session-",
                    "mediadrop-youtube-cookie-export-",
                    "mediadrop-youtube-cookies-",
                    "mediadrop-ytdlp-cookie-export-",
                    "mediadrop-ytdlp-cookies-",
                    "mediadrop-youtube-info-",
                ],
                Duration::from_secs(60 * 60),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_version,
            get_extension_setup_info,
            open_extension_setup,
            take_extension_setup_request,
            get_twitter_post_mp4_template,
            minimize_window,
            close_window,
            start_dragging,
            resize_window_height,
            get_window_position,
            set_window_position,
            pause_download,
            cancel_download,
            reveal_path,
            show_download_complete_notification,
            reveal_download,
            reveal_last_error_report,
            get_cloud_reports_enabled,
            set_cloud_reports_enabled,
            flush_pending_cloud_reports,
            update_ytdlp,
            list_cookie_browsers,
            get_cookie_browser_runtime_state,
            prepare_instagram_cookie_auth,
            get_instagram_cookie_state,
            clear_instagram_cookie_state,
            analyze_media,
            probe_instagram_video,
            analyze_video,
            cache_twitter_avatar,
            resolve_twitter_avatar_by_handle,
            prepare_media_preview,
            cache_thumbnail,
            prepare_clip_preview_stream,
            download_media_item,
            download_media_batch,
            download_media_post_card,
            download_video,
            download_twitter_post,
            companion::take_companion_handoff,
            companion::companion_renderer_ready,
            companion::complete_companion_render
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod launch_tests {
    use super::companion_launch_requested;

    #[test]
    fn only_native_host_launches_stay_hidden() {
        assert!(companion_launch_requested(&[
            "mediadrop.exe".to_string(),
            "--companion".to_string(),
        ]));
        assert!(!companion_launch_requested(&["mediadrop.exe".to_string()]));
    }
}
