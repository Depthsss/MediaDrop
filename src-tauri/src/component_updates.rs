use mediadrop_component_update::{
    load_activation_state, resolve_package_url, select_healthy_component, sha256_file,
    verify_component_file, verify_signed_manifest, ComponentError, MAX_COMPONENT_BYTES,
    MAX_MANIFEST_BYTES,
};
use reqwest::{blocking::Client, StatusCode};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const MANIFEST_URL: &str = "https://github.com/Depthsss/MediaDrop-Releases/releases/download/components-stable/component-manifest.json";
const SIGNATURE_URL: &str = "https://github.com/Depthsss/MediaDrop-Releases/releases/download/components-stable/component-manifest.sig";
const UPDATE_TIMEOUT: Duration = Duration::from_secs(180);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const STATUS_MAX_BYTES: u64 = 64 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) struct UpdateOutcome {
    pub(crate) updated: bool,
    pub(crate) message: String,
}

pub(crate) fn resolve_managed_ytdlp(
    is_healthy: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    resolve_managed_ytdlp_at(&component_store_root().ok()?, is_healthy)
}

fn resolve_managed_ytdlp_at(
    store_root: &Path,
    is_healthy: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let state = load_activation_state(&store_root.join("activation.json")).ok()?;
    let selected = select_healthy_component(&state, |entry| {
        let path = store_root.join(entry.relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        plain_file(&path)
            && sha256_file(&path).as_deref() == Ok(entry.file_sha256.as_str())
            && is_healthy(&path)
    })
    .ok()?;
    Some(store_root.join(
        selected
            .relative_path
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    ))
}

pub(crate) fn check_for_ytdlp_update(
    worker_path: &Path,
    current_core_version: &str,
) -> Result<UpdateOutcome, String> {
    if !plain_file(worker_path) {
        return Err("MediaDrop araç güncelleme hizmeti bulunamadı. Uygulamayı onarın.".into());
    }
    let client = Client::builder()
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(format!("MediaDrop/{current_core_version}"))
        .build()
        .map_err(|error| format!("Güncelleme bağlantısı hazırlanamadı: {error}"))?;
    let Some(manifest_bytes) = fetch_optional(&client, MANIFEST_URL, MAX_MANIFEST_BYTES as u64)?
    else {
        return Ok(UpdateOutcome {
            updated: false,
            message: "Küçük araç güncellemesi bulunmuyor.".into(),
        });
    };
    let signature_bytes = fetch_required(&client, SIGNATURE_URL, 1024)?;
    let signature = std::str::from_utf8(&signature_bytes)
        .map_err(|_| "Güncelleme imzası geçerli metin değil.".to_string())?;
    let state = load_activation_state(&component_store_root()?.join("activation.json"))
        .map_err(|error| format!("Araç güncelleme kaydı okunamadı: {error}"))?;
    let verified = match verify_signed_manifest(
        &manifest_bytes,
        signature,
        mediadrop_component_update::COMPONENT_PUBLIC_KEY_BASE64,
        current_core_version,
        state.accepted_revision,
    ) {
        Ok(verified) => verified,
        Err(ComponentError::RevisionReplay { .. }) => {
            return Ok(UpdateOutcome {
                updated: false,
                message: "yt-dlp zaten güncel.".into(),
            })
        }
        Err(ComponentError::IncompatibleCore) => {
            return Ok(UpdateOutcome {
                updated: false,
                message: "Yeni araç sürümü bu MediaDrop sürümüyle uyumlu değil.".into(),
            })
        }
        Err(error) => return Err(format!("Araç güncelleme imzası reddedildi: {error}")),
    };

    let component = verified.component();
    let package_url = resolve_package_url(component, verified.manifest.revision)
        .map_err(|error| format!("Araç güncelleme adresi reddedildi: {error}"))?;
    let session = ComponentSession::create()?;
    write_new(&session.path.join("component-manifest.json"), &manifest_bytes)
        .map_err(|error| format!("Güncelleme manifesti hazırlanamadı: {error}"))?;
    write_new(
        &session.path.join("component-manifest.sig"),
        signature.trim().as_bytes(),
    )
        .map_err(|error| format!("Güncelleme imzası hazırlanamadı: {error}"))?;
    let package = fetch_required(
        &client,
        &package_url,
        component.package_size.min(MAX_COMPONENT_BYTES),
    )?;
    let package_path = session.path.join("yt-dlp.exe");
    write_new(&package_path, &package)
        .map_err(|error| format!("Araç güncellemesi diske yazılamadı: {error}"))?;
    verify_component_file(component, &package_path)
        .map_err(|error| format!("İndirilen araç güncellemesi reddedildi: {error}"))?;
    session.write_config()?;
    run_component_broker(worker_path, &session.path)?;
    Ok(UpdateOutcome {
        updated: true,
        message: format!("yt-dlp {} güvenli biçimde etkinleştirildi.", component.version),
    })
}

fn fetch_optional(client: &Client, url: &str, maximum: u64) -> Result<Option<Vec<u8>>, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("Güncelleme sunucusuna ulaşılamadı: {error}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    read_response(response, maximum).map(Some)
}

fn fetch_required(client: &Client, url: &str, maximum: u64) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("Güncelleme dosyası indirilemedi: {error}"))?;
    read_response(response, maximum)
}

fn read_response(response: reqwest::blocking::Response, maximum: u64) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!(
            "Güncelleme sunucusu HTTP {} döndürdü.",
            response.status().as_u16()
        ));
    }
    if response.content_length().is_some_and(|length| length == 0 || length > maximum) {
        return Err("Güncelleme dosyası izin verilen boyutta değil.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Güncelleme yanıtı okunamadı: {error}"))?;
    if bytes.is_empty() || bytes.len() as u64 > maximum {
        return Err("Güncelleme dosyası izin verilen boyutta değil.".into());
    }
    Ok(bytes)
}

struct ComponentSession {
    path: PathBuf,
    session_id: String,
}

impl ComponentSession {
    fn create() -> Result<Self, String> {
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| "LOCALAPPDATA bulunamadı.".to_string())?;
        let root = PathBuf::from(local)
            .join("MediaDrop")
            .join("ComponentSessions");
        fs::create_dir_all(&root)
            .map_err(|error| format!("Güncelleme oturumu hazırlanamadı: {error}"))?;
        let session_id = Uuid::new_v4().to_string();
        let path = root.join(&session_id);
        fs::create_dir(&path)
            .map_err(|error| format!("Güncelleme oturumu oluşturulamadı: {error}"))?;
        Ok(Self { path, session_id })
    }

    fn write_config(&self) -> Result<(), String> {
        let body = format!(
            "[session]\r\nprotocol=1\r\nsession_id={}\r\nparent_pid={}\r\nsilent=1\r\n",
            self.session_id,
            std::process::id()
        );
        write_utf16_new(&self.path.join("config.ini"), &body)
            .map_err(|error| format!("Güncelleme oturumu kaydedilemedi: {error}"))
    }
}

impl Drop for ComponentSession {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_component_broker(worker_path: &Path, session_dir: &Path) -> Result<(), String> {
    let mut command = Command::new(worker_path);
    command
        .arg("--component-broker")
        .arg("--session-dir")
        .arg(session_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Araç güncelleme hizmeti başlatılamadı: {error}"))?;
    let status_path = session_dir.join("status.ini");
    let deadline = Instant::now() + UPDATE_TIMEOUT;
    loop {
        if let Some(status) = read_status(&status_path)? {
            match status.get("state").map(String::as_str) {
                Some("succeeded") => {
                    let _ = child.wait();
                    return Ok(());
                }
                Some("elevation_cancelled") => {
                    let _ = child.wait();
                    return Err("Windows yönetici izni verilmedi; güncelleme yapılmadı.".into());
                }
                Some("failed") => {
                    let _ = child.wait();
                    let kind = status
                        .get("result_kind")
                        .map(String::as_str)
                        .unwrap_or("failed");
                    return Err(format!("Araç güncellemesi tamamlanamadı ({kind})."));
                }
                _ => {}
            }
        }
        if let Some(exit) = child
            .try_wait()
            .map_err(|error| format!("Güncelleme hizmeti izlenemedi: {error}"))?
        {
            return Err(format!(
                "Araç güncelleme hizmeti erken kapandı ({:?}).",
                exit.code()
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Araç güncellemesi zaman aşımına uğradı.".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_status(path: &Path) -> Result<Option<HashMap<String, String>>, String> {
    let bytes = match fs::metadata(path) {
        Ok(metadata) if metadata.len() <= STATUS_MAX_BYTES => fs::read(path)
            .map_err(|error| format!("Güncelleme durumu okunamadı: {error}"))?,
        Ok(_) => return Err("Güncelleme durum dosyası çok büyük.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Güncelleme durumu açılamadı: {error}")),
    };
    if bytes.len() < 2 || bytes[..2] != [0xff, 0xfe] || !(bytes.len() - 2).is_multiple_of(2) {
        return Err("Güncelleme durum dosyası bozuk.".into());
    }
    let units = bytes[2..]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let text = String::from_utf16(&units)
        .map_err(|_| "Güncelleme durum dosyası geçersiz.".to_string())?;
    let mut values = HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    Ok(Some(values))
}

fn write_utf16_new(path: &Path, value: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(&[0xff, 0xfe])?;
    for unit in value.encode_utf16() {
        file.write_all(&unit.to_le_bytes())?;
    }
    file.sync_all()
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn plain_file(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

#[cfg(windows)]
fn component_store_root() -> Result<PathBuf, String> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
    };
    let mut raw = std::ptr::null_mut();
    // SAFETY: the known-folder ID and output pointer follow SHGetKnownFolderPath's contract.
    let result = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        return Err("ProgramData klasörü bulunamadı.".into());
    }
    let mut length = 0;
    // SAFETY: a successful call returns a NUL-terminated CoTaskMem string.
    while unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: length was measured within the returned NUL-terminated allocation.
    let root = PathBuf::from(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    // SAFETY: SHGetKnownFolderPath allocated this buffer with the COM task allocator.
    unsafe { CoTaskMemFree(raw.cast()) };
    if !root.is_absolute() {
        return Err("ProgramData yolu geçersiz.".into());
    }
    Ok(root.join("MediaDrop").join("Components"))
}

#[cfg(not(windows))]
fn component_store_root() -> Result<PathBuf, String> {
    Err("Araç bileşeni güncellemesi yalnız Windows'ta destekleniyor.".into())
}

#[cfg(test)]
mod tests {
    use super::resolve_managed_ytdlp_at;
    use mediadrop_component_update::{ActivationEntry, ActivationState};
    use std::{fs, path::PathBuf};

    #[test]
    fn unhealthy_active_component_uses_the_verified_last_known_good_file() {
        let root = std::env::temp_dir().join(format!(
            "mediadrop-runtime-component-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let active_path = root.join("yt-dlp/2/yt-dlp.exe");
        let lkg_path = root.join("yt-dlp/1/yt-dlp.exe");
        fs::create_dir_all(active_path.parent().unwrap()).unwrap();
        fs::create_dir_all(lkg_path.parent().unwrap()).unwrap();
        fs::write(&active_path, b"bad").unwrap();
        fs::write(&lkg_path, b"good").unwrap();
        let entry = |revision, hash: String| ActivationEntry {
            revision,
            version: revision.to_string(),
            manifest_sha256: "a".repeat(64),
            file_sha256: hash,
            relative_path: format!("yt-dlp/{revision}/yt-dlp.exe"),
        };
        let state = ActivationState {
            schema: 1,
            accepted_revision: 2,
            active: Some(entry(2, mediadrop_component_update::sha256_file(&active_path).unwrap())),
            last_known_good: Some(entry(1, mediadrop_component_update::sha256_file(&lkg_path).unwrap())),
        };
        fs::write(
            root.join("activation.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();

        let resolved = resolve_managed_ytdlp_at(&root, |path| path == lkg_path).unwrap();
        assert_eq!(resolved, PathBuf::from(lkg_path));
        fs::remove_dir_all(root).unwrap();
    }
}
