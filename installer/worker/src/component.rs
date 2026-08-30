use mediadrop_component_update::{
    load_activation_state, sha256_file, verify_component_file, verify_signed_manifest,
    ActivationEntry, ActivationState, ComponentError, COMPONENT_MANIFEST_SCHEMA,
    MAX_MANIFEST_BYTES,
};
use std::{
    error::Error,
    fmt, fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

#[derive(Debug)]
pub enum ComponentInstallError {
    Component(ComponentError),
    InvalidPath(&'static str),
    HealthCheckFailed,
    Cancelled,
    ActivationState(String),
    Io(std::io::Error),
}

impl fmt::Display for ComponentInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Component(error) => error.fmt(formatter),
            Self::InvalidPath(reason) => write!(formatter, "invalid component path: {reason}"),
            Self::HealthCheckFailed => formatter.write_str("component health check failed"),
            Self::Cancelled => formatter.write_str("component installation cancelled"),
            Self::ActivationState(error) => {
                write!(formatter, "activation state is invalid: {error}")
            }
            Self::Io(error) => write!(formatter, "component installation I/O failed: {error}"),
        }
    }
}

impl Error for ComponentInstallError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Component(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ComponentError> for ComponentInstallError {
    fn from(error: ComponentError) -> Self {
        Self::Component(error)
    }
}

impl From<std::io::Error> for ComponentInstallError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn install_component(
    session_dir: &Path,
    store_root: &Path,
    public_key_base64: &str,
    current_core_version: &str,
    health_check: impl Fn(&Path, &[String], Duration) -> bool,
    cancel_requested: impl Fn() -> bool,
) -> Result<ActivationEntry, ComponentInstallError> {
    require_plain_directory(session_dir)?;
    ensure_store_root(store_root)?;
    let activation_path = store_root.join("activation.json");
    let previous = load_activation_state(&activation_path)?;

    let manifest_bytes = read_bounded(
        &session_dir.join("component-manifest.json"),
        MAX_MANIFEST_BYTES as u64,
    )?;
    let signature = String::from_utf8(read_bounded(
        &session_dir.join("component-manifest.sig"),
        1024,
    )?)
    .map_err(|error| ComponentInstallError::ActivationState(error.to_string()))?;
    let verified = verify_signed_manifest(
        &manifest_bytes,
        &signature,
        public_key_base64,
        current_core_version,
        previous.accepted_revision,
    )?;
    let component = verified.component();
    let source = session_dir.join("yt-dlp.exe");
    verify_component_file(component, &source)?;
    if cancel_requested() {
        return Err(ComponentInstallError::Cancelled);
    }

    let component_root = store_root.join("yt-dlp");
    fs::create_dir_all(&component_root)?;
    require_plain_directory(&component_root)?;
    let revision = verified.manifest.revision;
    let relative_path = format!("yt-dlp/{revision}/yt-dlp.exe");
    let final_dir = component_root.join(revision.to_string());
    let final_file = final_dir.join("yt-dlp.exe");
    if final_dir.exists() {
        require_plain_directory(&final_dir)?;
        verify_component_file(component, &final_file)?;
        if !health_check(
            &final_file,
            &component.health_check.argv,
            Duration::from_millis(component.health_check.timeout_ms),
        ) {
            return Err(ComponentInstallError::HealthCheckFailed);
        }
    } else {
        let staging_dir = component_root.join(format!("{revision}.staging-{}", std::process::id()));
        if staging_dir.exists() {
            remove_plain_tree(&staging_dir)?;
        }
        fs::create_dir(&staging_dir)?;
        let staging_file = staging_dir.join("yt-dlp.exe");
        copy_new(&source, &staging_file)?;
        verify_component_file(component, &staging_file)?;
        if cancel_requested() {
            remove_plain_tree(&staging_dir)?;
            return Err(ComponentInstallError::Cancelled);
        }
        if !health_check(
            &staging_file,
            &component.health_check.argv,
            Duration::from_millis(component.health_check.timeout_ms),
        ) {
            remove_plain_tree(&staging_dir)?;
            return Err(ComponentInstallError::HealthCheckFailed);
        }
        if cancel_requested() {
            remove_plain_tree(&staging_dir)?;
            return Err(ComponentInstallError::Cancelled);
        }
        if let Err(error) = fs::rename(&staging_dir, &final_dir) {
            let _ = remove_plain_tree(&staging_dir);
            return Err(error.into());
        }
        verify_component_file(component, &final_file)?;
    }

    let active = ActivationEntry {
        revision,
        version: component.version.clone(),
        manifest_sha256: verified.manifest_sha256.clone(),
        file_sha256: component.package_sha256.clone(),
        relative_path,
    };
    let last_known_good = previous
        .active
        .as_ref()
        .filter(|entry| entry.revision != active.revision)
        .filter(|entry| activation_entry_file_is_valid(store_root, entry))
        .cloned()
        .or(previous.last_known_good);
    let next = ActivationState {
        schema: COMPONENT_MANIFEST_SCHEMA,
        accepted_revision: revision,
        active: Some(active.clone()),
        last_known_good,
    };
    if cancel_requested() {
        return Err(ComponentInstallError::Cancelled);
    }
    write_activation_state(&activation_path, &next)?;
    Ok(active)
}

fn write_activation_state(
    path: &Path,
    state: &ActivationState,
) -> Result<(), ComponentInstallError> {
    let temporary = path.with_extension("json.new");
    let bytes = serde_json::to_vec(state)
        .map_err(|error| ComponentInstallError::ActivationState(error.to_string()))?;
    let result = (|| -> Result<(), ComponentInstallError> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        atomic_replace(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: both paths are live NUL-terminated UTF-16 buffers for the duration of the call.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

fn activation_entry_file_is_valid(store_root: &Path, entry: &ActivationEntry) -> bool {
    let path = store_root.join(
        entry
            .relative_path
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    path.is_file() && sha256_file(&path).as_deref() == Ok(entry.file_sha256.as_str())
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, ComponentInstallError> {
    require_plain_file(path)?;
    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size == 0 || size > maximum {
        return Err(ComponentInstallError::InvalidPath("file size"));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn copy_new(source: &Path, destination: &Path) -> Result<(), ComponentInstallError> {
    require_plain_file(source)?;
    let mut source = fs::File::open(source)?;
    let mut destination = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    std::io::copy(&mut source, &mut destination)?;
    destination.flush()?;
    destination.sync_all()?;
    Ok(())
}

fn ensure_store_root(path: &Path) -> Result<(), ComponentInstallError> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    require_plain_directory(path)
}

fn require_plain_directory(path: &Path) -> Result<(), ComponentInstallError> {
    if !path.is_absolute() {
        return Err(ComponentInstallError::InvalidPath(
            "directory is not absolute",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(ComponentInstallError::InvalidPath("directory is not plain"));
    }
    Ok(())
}

fn require_plain_file(path: &Path) -> Result<(), ComponentInstallError> {
    if !path.is_absolute() {
        return Err(ComponentInstallError::InvalidPath("file is not absolute"));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(ComponentInstallError::InvalidPath("file is not plain"));
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn remove_plain_tree(path: &Path) -> Result<(), ComponentInstallError> {
    require_plain_directory(path)?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        if metadata.is_symlink() {
            return Err(ComponentInstallError::InvalidPath("staging symlink"));
        }
        if metadata.is_dir() {
            remove_plain_tree(&entry.path())?;
        } else if metadata.is_file() {
            fs::remove_file(entry.path())?;
        } else {
            return Err(ComponentInstallError::InvalidPath("staging entry"));
        }
    }
    fs::remove_dir(path)?;
    Ok(())
}
