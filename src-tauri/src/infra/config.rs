use crate::*;

pub(crate) fn mediadrop_config_dir() -> Result<PathBuf, String> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .or_else(|_| {
            std::env::var("USERPROFILE").map(|profile| format!("{}\\AppData\\Local", profile))
        })
        .map_err(|_| "LOCALAPPDATA bulunamadı. Config klasörü oluşturulamadı.".to_string())?;

    let dir = PathBuf::from(local_app_data).join("MediaDrop");

    fs::create_dir_all(&dir)
        .map_err(|err| format!("MediaDrop config klasörü oluşturulamadı: {}", err))?;

    Ok(dir)
}

pub(crate) fn mediadrop_config_path() -> Result<PathBuf, String> {
    Ok(mediadrop_config_dir()?.join("config.json"))
}

static MEDIADROP_CONFIG_IO: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn mediadrop_config_io() -> &'static Mutex<()> {
    MEDIADROP_CONFIG_IO.get_or_init(|| Mutex::new(()))
}

pub(crate) fn try_read_mediadrop_config_unlocked() -> Result<Option<serde_json::Value>, String> {
    let path = mediadrop_config_path()?;
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("MediaDrop config okunamadı: {}", err)),
    };
    let value = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|err| format!("MediaDrop config JSON geçersiz: {}", err))?;
    Ok(Some(value))
}

fn read_mediadrop_config_unlocked() -> serde_json::Value {
    try_read_mediadrop_config_unlocked()
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}))
}

#[cfg(target_os = "windows")]
pub(crate) fn atomic_replace_config_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let ok = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn atomic_replace_config_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

pub(crate) fn write_mediadrop_config_unlocked(value: &serde_json::Value) -> Result<(), String> {
    let path = mediadrop_config_path()?;
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| format!("Config JSON oluşturulamadı: {}", err))?;
    let temp_path = path.with_extension(format!("json.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|err| format!("MediaDrop geçici config dosyası açılamadı: {}", err))?;
        file.write_all(text.as_bytes())
            .map_err(|err| format!("MediaDrop config yazılamadı: {}", err))?;
        file.sync_all()
            .map_err(|err| format!("MediaDrop config diske yazılamadı: {}", err))?;
        atomic_replace_config_file(&temp_path, &path)
            .map_err(|err| format!("MediaDrop config atomik olarak yenilenemedi: {}", err))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub(crate) fn update_mediadrop_config<R>(
    update: impl FnOnce(&mut serde_json::Value) -> R,
) -> Result<R, String> {
    let _guard = mediadrop_config_io()
        .lock()
        .map_err(|_| "MediaDrop config kilidi alınamadı.".to_string())?;
    let mut value = read_mediadrop_config_unlocked();
    let result = update(&mut value);
    write_mediadrop_config_unlocked(&value)?;
    Ok(result)
}
