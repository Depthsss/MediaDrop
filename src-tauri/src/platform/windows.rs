use crate::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrayAction {
    Show,
    Quit,
    Ignore,
}

fn tray_action_for_id(id: &str) -> TrayAction {
    match id {
        "show" => TrayAction::Show,
        "quit" => TrayAction::Quit,
        _ => TrayAction::Ignore,
    }
}

pub(crate) fn setup_windows_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItemBuilder::with_id("show", "MediaDrop'u aç").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Çıkış").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("MediaDrop")
        .on_menu_event(|app, event| match tray_action_for_id(event.id().as_ref()) {
            TrayAction::Show => {
                let _ = show_main_window(app);
            }
            TrayAction::Quit => app.exit(0),
            TrayAction::Ignore => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub(crate) fn show_main_window(app: &tauri::AppHandle) -> ApiResult<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| ApiError::new("window_error", "MediaDrop ana penceresi bulunamadı."))?;
    window
        .show()
        .map_err(|err| ApiError::new("window_error", format!("Pencere gösterilemedi: {err}")))?;
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().map_err(|err| {
            ApiError::new("window_error", format!("Pencere geri yüklenemedi: {err}"))
        })?;
    }
    window
        .set_focus()
        .map_err(|err| ApiError::new("window_error", format!("Pencere öne getirilemedi: {err}")))
}

#[tauri::command]
pub(crate) fn minimize_window(window: tauri::Window) -> ApiResult<()> {
    window
        .minimize()
        .map_err(|err| ApiError::new("window_error", format!("Pencere küçültülemedi: {err}")))
}

#[tauri::command]
pub(crate) fn close_window(window: tauri::Window) -> ApiResult<()> {
    window
        .hide()
        .map_err(|err| ApiError::new("window_error", format!("Pencere gizlenemedi: {err}")))
}

#[tauri::command]
pub(crate) fn start_dragging(window: tauri::Window) -> ApiResult<()> {
    window
        .start_dragging()
        .map_err(|err| ApiError::new("window_error", format!("Pencere sürükleme başlatılamadı: {err}")))
}

#[tauri::command]
pub(crate) fn resize_window_height(window: tauri::Window, height: f64) -> ApiResult<()> {
    const MIN_WINDOW_HEIGHT: f64 = 520.0;
    const MAX_WINDOW_HEIGHT: f64 = 980.0;
    const WORK_AREA_GAP: f64 = 16.0;

    let scale_factor = window.scale_factor().unwrap_or(1.0).max(0.5);
    let current_size = window
        .outer_size()
        .map_err(|err| format!("Pencere boyutu okunamadı: {}", err))?;
    let current_width = (current_size.width as f64 / scale_factor).max(680.0);
    let monitor_max_height = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|monitor| {
            (monitor.work_area().size.height as f64 / monitor.scale_factor()) - WORK_AREA_GAP
        })
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(MAX_WINDOW_HEIGHT)
        .min(MAX_WINDOW_HEIGHT);
    let effective_min = MIN_WINDOW_HEIGHT.min(monitor_max_height);
    let safe_height = height.max(effective_min).min(monitor_max_height).round();

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: current_width,
            height: safe_height,
        }))
        .map_err(|err| format!("Pencere boyutu ayarlanamadı: {}", err))?;

    keep_window_visible(&window).map_err(ApiError::from)
}

#[tauri::command]
pub(crate) fn get_window_position(window: tauri::Window) -> ApiResult<WindowPosition> {
    let position = window
        .outer_position()
        .map_err(|err| format!("Pencere konumu okunamadı: {}", err))?;

    Ok(WindowPosition {
        x: position.x,
        y: position.y,
    })
}

#[tauri::command]
pub(crate) fn set_window_position(window: tauri::Window, x: i32, y: i32) -> ApiResult<WindowPosition> {
    let safe = safe_window_position(&window, x, y);

    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: safe.x,
            y: safe.y,
        }))
        .map_err(|err| format!("Pencere konumu ayarlanamadı: {}", err))?;

    Ok(safe)
}

#[tauri::command]
pub(crate) fn pause_download(job_id: Option<String>) -> ApiResult<()> {
    request_active_process_stop(DownloadStopRequest::Pause, job_id.as_deref())
        .map_err(ApiError::from_legacy)
}

#[tauri::command]
pub(crate) fn cancel_download(job_id: Option<String>) -> ApiResult<()> {
    request_active_process_stop(DownloadStopRequest::Cancel, job_id.as_deref())
        .map_err(ApiError::from_legacy)
}

fn normalize_explorer_path(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err("Dosya bulunamadı. Çıktı dosyası taşınmış veya silinmiş olabilir.".to_string());
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("Geçerli klasör okunamadı: {}", err))?
            .join(path)
    };

    let mut text = absolute.to_string_lossy().to_string();

    // fs::canonicalize Windows'ta çoğu zaman \\?\ prefix'i üretir.
    // Explorer /select bu prefix'i görünce dosyayı seçmek yerine Belgeler'e düşebiliyor.
    // Bu yüzden Explorer'a her zaman normal C:\... formatı gönderiyoruz.
    if let Some(stripped) = text.strip_prefix("\\\\?\\") {
        text = stripped.to_string();
    }

    Ok(text)
}

pub(crate) fn reveal_file_in_explorer(path: &Path) -> Result<(), String> {
    let target = normalize_explorer_path(path)?;

    #[cfg(target_os = "windows")]
    {
        // Explorer için daha stabil çağrı:
        // explorer.exe /select, "C:\path\file.mp4"
        // Tek string /select,"..." bazı sistemlerde Belgeler'i açabiliyor.
        hidden_command("explorer.exe")
            .arg("/select,")
            .arg(&target)
            .spawn()
            .map_err(|err| format!("Klasörde gösterilemedi: {}", err))?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let target_path = PathBuf::from(target);

        if let Some(parent) = target_path.parent() {
            return open_folder_in_explorer(parent);
        }

        Err("Klasör bulunamadı.".to_string())
    }
}

fn open_folder_in_explorer(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("Klasör bulunamadı.".to_string());
    }

    hidden_command("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|err| format!("Klasör açılamadı: {}", err))?;

    Ok(())
}

#[tauri::command]
pub(crate) fn reveal_path(window: tauri::Window, path: String) -> ApiResult<()> {
    let clean = path.trim();

    if clean.is_empty() {
        return Err(ApiError::new("path_missing", "Gösterilecek dosya yolu bulunamadı."));
    }

    let target = PathBuf::from(clean);

    if target.is_file() {
        reveal_file_in_explorer(&target).map_err(ApiError::from)?;
    } else if target.is_dir() {
        open_folder_in_explorer(&target).map_err(ApiError::from)?;
    } else {
        return Err(ApiError::new(
            "path_missing",
            "Dosya bulunamadı. Çıktı dosyası taşınmış veya silinmiş olabilir.",
        ));
    }

    let _ = window.minimize();
    Ok(())
}

pub(crate) fn reveal_download_notification_target(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return reveal_file_in_explorer(path);
    }

    if path.is_dir() {
        return open_folder_in_explorer(path);
    }

    Err("İndirilen dosya taşınmış veya silinmiş olabilir.".to_string())
}

fn download_complete_notification_body(target: &Path, file_count: usize) -> String {
    if file_count > 1 {
        format!(
            "{} dosya hazır. Klasörü açmak için bildirime tıklayın.",
            file_count
        )
    } else {
        let raw_name = target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("İndirilen dosya");
        let file_name = truncate_filename_chars(raw_name, 52);

        format!(
            "{} hazır. Dosyada görmek için bildirime tıklayın.",
            file_name
        )
    }
}

#[cfg(target_os = "windows")]
fn download_notification_icon_path() -> Result<PathBuf, String> {
    let icon_path = mediadrop_config_dir()?.join("notification-icon.png");
    let needs_refresh = fs::read(&icon_path)
        .map(|current| current != DOWNLOAD_NOTIFICATION_ICON_BYTES)
        .unwrap_or(true);

    if needs_refresh {
        fs::write(&icon_path, DOWNLOAD_NOTIFICATION_ICON_BYTES)
            .map_err(|err| format!("Bildirim logosu hazırlanamadı: {}", err))?;
    }

    Ok(icon_path)
}

#[cfg(target_os = "windows")]
fn build_download_complete_toast(
    app_id: &str,
    target: &Path,
    icon_path: &Path,
    file_count: usize,
) -> tauri_winrt_notification::Toast {
    use tauri_winrt_notification::{Duration as ToastDuration, IconCrop, Toast};

    let click_target = target.to_path_buf();
    let body = download_complete_notification_body(target, file_count);
    let action_label = if file_count > 1 {
        "Klasörü aç"
    } else {
        "Dosyada göster"
    };

    Toast::new(app_id)
        .title("MediaDrop")
        .text1("✓ İndirmeniz tamamlandı")
        .text2(&body)
        .icon(icon_path, IconCrop::Square, "MediaDrop")
        .duration(ToastDuration::Short)
        .add_button(action_label, "reveal")
        .on_activated(move |_| {
            let _ = reveal_download_notification_target(&click_target);
            Ok(())
        })
}

#[tauri::command]
pub(crate) fn show_download_complete_notification(
    window: tauri::Window,
    file_path: String,
    file_count: Option<usize>,
) -> ApiResult<()> {
    if !should_show_download_notification(window.is_focused().unwrap_or(false)) {
        return Ok(());
    }

    let clean = file_path.trim();
    if clean.is_empty() {
        return Err(ApiError::new(
            "notification_target_missing",
            "Bildirim için indirilen dosya yolu bulunamadı.",
        ));
    }

    let target = PathBuf::from(clean);
    let absolute_target = if target.is_absolute() {
        target
    } else {
        std::env::current_dir()
            .map_err(|err| format!("Geçerli klasör okunamadı: {}", err))?
            .join(target)
    };

    if !absolute_target.exists() {
        return Err(ApiError::new(
            "notification_target_missing",
            "Bildirim için indirilen dosya bulunamadı.",
        ));
    }

    #[cfg(target_os = "windows")]
    {
        use tauri_winrt_notification::Toast;

        let count = file_count.unwrap_or(1).max(1);
        let icon_path = download_notification_icon_path()?;
        let toast = build_download_complete_toast(
            MEDIADROP_IDENTIFIER,
            &absolute_target,
            &icon_path,
            count,
        );

        if toast.show().is_ok() {
            return Ok(());
        }

        build_download_complete_toast(
            Toast::POWERSHELL_APP_ID,
            &absolute_target,
            &icon_path,
            count,
        )
        .show()
        .map_err(|err| ApiError::new("notification_error", format!("Windows bildirimi gösterilemedi: {err}")))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = file_count;
        Ok(())
    }
}

fn should_show_download_notification(window_focused: bool) -> bool {
    !window_focused
}

fn is_media_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };

    matches!(
        ext.to_lowercase().as_str(),
        "mp4"
            | "mp3"
            | "m4a"
            | "webm"
            | "mkv"
            | "mov"
            | "wav"
            | "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "gif"
            | "avif"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_window_suppresses_download_notification() {
        assert!(!should_show_download_notification(true));
        assert!(should_show_download_notification(false));
    }

    #[test]
    fn tray_menu_routes_only_known_actions() {
        assert_eq!(tray_action_for_id("show"), TrayAction::Show);
        assert_eq!(tray_action_for_id("quit"), TrayAction::Quit);
        assert_eq!(tray_action_for_id("unknown"), TrayAction::Ignore);
    }
}

fn normalize_search_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
}

fn title_match_score(path: &Path, title: &str) -> usize {
    let title_norm = normalize_search_text(title);
    let name_norm = normalize_search_text(
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(""),
    );

    title_norm
        .split_whitespace()
        .filter(|token| token.len() >= 4 && name_norm.contains(*token))
        .count()
}

fn modified_distance_ms(path: &Path, target_ms: u64) -> u128 {
    let modified_ms = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    let target = target_ms as u128;

    modified_ms.abs_diff(target)
}

fn find_moved_or_renamed_download(
    output_dir: &Path,
    original_path: &Path,
    title: &str,
    downloaded_at_ms: Option<u64>,
    expected_file_size: Option<u64>,
) -> Option<PathBuf> {
    if !output_dir.is_dir() {
        return None;
    }

    // Dosya silindiyse rastgele "yakın görünen" videoya atlamasını istemiyoruz.
    // Bu yüzden rename fallback'i sadece dosya boyutu biliniyorsa çalışsın.
    let expected_size = expected_file_size.filter(|value| *value > 0)?;

    let original_name = original_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mut candidates = fs::read_dir(output_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && is_media_extension(path)
                && file_size(path).unwrap_or(0) == expected_size
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return None;
    }

    // Rename işlemi normalde modified time'ı değiştirmez.
    // Bu yüzden aynı boyuttaki adaylar içinde indirme zamanına makul yakın olanları tutuyoruz.
    // Dosya tamamen silindiyse ve klasörde başka videolar varsa artık onları seçmeyecek.
    if let Some(target_ms) = downloaded_at_ms {
        const RENAME_TIME_TOLERANCE_MS: u128 = 6 * 60 * 60 * 1000;

        candidates.retain(|path| modified_distance_ms(path, target_ms) <= RENAME_TIME_TOLERANCE_MS);
    }

    if candidates.is_empty() {
        return None;
    }

    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }

    // Birden fazla aynı boyutta aday varsa önce orijinal dosya adı birebir aynı mı bak.
    if !original_name.is_empty() {
        let exact_name_matches = candidates
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .map(|name| name.to_lowercase() == original_name)
                    .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();

        if exact_name_matches.len() == 1 {
            return exact_name_matches.into_iter().next();
        }
    }

    // Son çare: başlık eşleşmesi çok güçlü olmalı.
    // Eski kodda > 0 olduğu için "official", "video", "music" gibi tek kelimeyle yanlış dosya seçiyordu.
    let strong_title_matches = candidates
        .iter()
        .filter(|path| title_match_score(path, title) >= 3)
        .cloned()
        .collect::<Vec<_>>();

    if strong_title_matches.len() == 1 {
        return strong_title_matches.into_iter().next();
    }

    // Belirsizse yanlış dosya seçme. Kullanıcıya "bulunamadı" dedirt.
    None
}

#[tauri::command]
pub(crate) fn reveal_download(
    file_path: String,
    output_dir: String,
    title: String,
    downloaded_at_ms: Option<u64>,
    file_size: Option<u64>,
) -> ApiResult<()> {
    let clean_file = file_path.trim();
    let clean_dir = output_dir.trim();

    if clean_file.is_empty() && clean_dir.is_empty() {
        return Err(ApiError::new(
            "path_missing",
            "Gösterilecek çıktı dosyası yolu bulunamadı.",
        ));
    }

    let target = PathBuf::from(clean_file);

    if target.is_file() {
        return reveal_file_in_explorer(&target).map_err(ApiError::from);
    }

    let search_dir = if !clean_dir.is_empty() {
        PathBuf::from(clean_dir)
    } else {
        target
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_default()
    };

    if let Some(found) =
        find_moved_or_renamed_download(&search_dir, &target, &title, downloaded_at_ms, file_size)
    {
        return reveal_file_in_explorer(&found).map_err(ApiError::from);
    }

    Err(ApiError::new(
        "path_missing",
        "Dosya bulunamadı. Çıktı dosyası taşınmış veya silinmiş olabilir.",
    ))
}
