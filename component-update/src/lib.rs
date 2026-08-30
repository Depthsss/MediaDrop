use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{error::Error, fmt, fs, io::Read, path::Path};

pub const COMPONENT_MANIFEST_SCHEMA: u32 = 1;
pub const COMPONENT_ID: &str = "yt-dlp";
pub const COMPONENT_TARGET: &str = "windows-x86_64";
pub const COMPONENT_PUBLIC_KEY_BASE64: &str = "8p6EvCFWo9NTck89SF799U6PDWE1V3PlB42+KWDt6GE=";
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_COMPONENT_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentManifest {
    pub schema: u32,
    pub channel: String,
    pub revision: u64,
    pub issued_at: String,
    pub build_id: String,
    pub minimum_core_version: String,
    pub maximum_core_version_exclusive: String,
    pub components: Vec<ComponentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentSpec {
    pub name: String,
    pub version: String,
    pub target: String,
    pub package_url_id: String,
    pub package_size: u64,
    pub package_sha256: String,
    pub files: Vec<ComponentFile>,
    pub health_check: HealthCheck,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthCheck {
    pub argv: Vec<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationEntry {
    pub revision: u64,
    pub version: String,
    pub manifest_sha256: String,
    pub file_sha256: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationState {
    pub schema: u32,
    pub accepted_revision: u64,
    pub active: Option<ActivationEntry>,
    pub last_known_good: Option<ActivationEntry>,
}

impl Default for ActivationState {
    fn default() -> Self {
        Self {
            schema: COMPONENT_MANIFEST_SCHEMA,
            accepted_revision: 0,
            active: None,
            last_known_good: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentError {
    InvalidPublicKey,
    InvalidSignatureEncoding,
    InvalidSignature,
    InvalidManifest(String),
    UnsupportedSchema,
    UnsupportedChannel,
    RevisionReplay { accepted: u64, received: u64 },
    IncompatibleCore,
    InvalidComponent(&'static str),
    PackageSizeMismatch { expected: u64, actual: u64 },
    PackageHashMismatch,
    NoHealthyComponent,
    Io(String),
}

impl fmt::Display for ComponentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPublicKey => formatter.write_str("component public key is invalid"),
            Self::InvalidSignatureEncoding => {
                formatter.write_str("component signature encoding is invalid")
            }
            Self::InvalidSignature => formatter.write_str("component signature is invalid"),
            Self::InvalidManifest(error) => {
                write!(formatter, "component manifest is invalid: {error}")
            }
            Self::UnsupportedSchema => {
                formatter.write_str("component manifest schema is unsupported")
            }
            Self::UnsupportedChannel => formatter.write_str("component channel is unsupported"),
            Self::RevisionReplay { accepted, received } => write!(
                formatter,
                "component revision {received} is not newer than accepted revision {accepted}"
            ),
            Self::IncompatibleCore => {
                formatter.write_str("component is incompatible with this MediaDrop version")
            }
            Self::InvalidComponent(field) => {
                write!(formatter, "component field is invalid: {field}")
            }
            Self::PackageSizeMismatch { expected, actual } => {
                write!(
                    formatter,
                    "component size mismatch: expected {expected}, got {actual}"
                )
            }
            Self::PackageHashMismatch => formatter.write_str("component SHA-256 mismatch"),
            Self::NoHealthyComponent => formatter.write_str("no healthy component is available"),
            Self::Io(error) => write!(formatter, "component I/O failed: {error}"),
        }
    }
}

impl Error for ComponentError {}

impl From<std::io::Error> for ComponentError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    pub manifest: ComponentManifest,
    pub manifest_sha256: String,
}

impl VerifiedManifest {
    pub fn component(&self) -> &ComponentSpec {
        &self.manifest.components[0]
    }
}

pub fn verify_signed_manifest(
    manifest_bytes: &[u8],
    signature_base64: &str,
    public_key_base64: &str,
    current_core_version: &str,
    accepted_revision: u64,
) -> Result<VerifiedManifest, ComponentError> {
    if manifest_bytes.is_empty() || manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ComponentError::InvalidManifest("size".into()));
    }
    let public_key: [u8; 32] = STANDARD
        .decode(public_key_base64.trim())
        .map_err(|_| ComponentError::InvalidPublicKey)?
        .try_into()
        .map_err(|_| ComponentError::InvalidPublicKey)?;
    let signature: [u8; 64] = STANDARD
        .decode(signature_base64.trim())
        .map_err(|_| ComponentError::InvalidSignatureEncoding)?
        .try_into()
        .map_err(|_| ComponentError::InvalidSignatureEncoding)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| ComponentError::InvalidPublicKey)?;
    verifying_key
        .verify_strict(manifest_bytes, &Signature::from_bytes(&signature))
        .map_err(|_| ComponentError::InvalidSignature)?;

    let manifest: ComponentManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|error| ComponentError::InvalidManifest(error.to_string()))?;
    validate_manifest(&manifest, current_core_version, accepted_revision)?;
    Ok(VerifiedManifest {
        manifest,
        manifest_sha256: sha256_bytes(manifest_bytes),
    })
}

fn validate_manifest(
    manifest: &ComponentManifest,
    current_core_version: &str,
    accepted_revision: u64,
) -> Result<(), ComponentError> {
    if manifest.schema != COMPONENT_MANIFEST_SCHEMA {
        return Err(ComponentError::UnsupportedSchema);
    }
    if manifest.channel != "stable" {
        return Err(ComponentError::UnsupportedChannel);
    }
    if manifest.revision <= accepted_revision {
        return Err(ComponentError::RevisionReplay {
            accepted: accepted_revision,
            received: manifest.revision,
        });
    }
    let current = parse_core_version(current_core_version)?;
    let minimum = parse_core_version(&manifest.minimum_core_version)?;
    let maximum = parse_core_version(&manifest.maximum_core_version_exclusive)?;
    if current < minimum || current >= maximum {
        return Err(ComponentError::IncompatibleCore);
    }
    if !is_sha256_id(&manifest.build_id)
        || manifest.issued_at.is_empty()
        || manifest.issued_at.len() > 64
        || manifest.components.len() != 1
    {
        return Err(ComponentError::InvalidComponent("manifest"));
    }
    validate_component(&manifest.components[0], manifest.revision)
}

fn validate_component(component: &ComponentSpec, revision: u64) -> Result<(), ComponentError> {
    if component.name != COMPONENT_ID || component.target != COMPONENT_TARGET {
        return Err(ComponentError::InvalidComponent("identity"));
    }
    if component.version.is_empty()
        || component.version.len() > 64
        || !component
            .version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(ComponentError::InvalidComponent("version"));
    }
    if component.package_size == 0 || component.package_size > MAX_COMPONENT_BYTES {
        return Err(ComponentError::InvalidComponent("packageSize"));
    }
    if !is_sha256(&component.package_sha256) {
        return Err(ComponentError::InvalidComponent("packageSha256"));
    }
    if component.package_url_id != format!("github-release:components-r{revision}/yt-dlp.exe") {
        return Err(ComponentError::InvalidComponent("packageUrlId"));
    }
    if component.files.len() != 1 {
        return Err(ComponentError::InvalidComponent("files"));
    }
    let file = &component.files[0];
    if file.path != "yt-dlp.exe"
        || file.size != component.package_size
        || file.sha256 != component.package_sha256
    {
        return Err(ComponentError::InvalidComponent("files"));
    }
    if component.health_check.argv != ["--ignore-config", "--no-plugin-dirs", "--version"]
        || !(500..=10_000).contains(&component.health_check.timeout_ms)
    {
        return Err(ComponentError::InvalidComponent("healthCheck"));
    }
    Ok(())
}

pub fn resolve_package_url(
    component: &ComponentSpec,
    revision: u64,
) -> Result<String, ComponentError> {
    validate_component(component, revision)?;
    Ok(format!(
        "https://github.com/Depthsss/MediaDrop-Releases/releases/download/components-r{revision}/yt-dlp.exe"
    ))
}

pub fn verify_component_file(component: &ComponentSpec, path: &Path) -> Result<(), ComponentError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ComponentError::InvalidComponent("packagePath"));
    }
    if metadata.len() != component.package_size {
        return Err(ComponentError::PackageSizeMismatch {
            expected: component.package_size,
            actual: metadata.len(),
        });
    }
    if sha256_file(path)? != component.package_sha256 {
        return Err(ComponentError::PackageHashMismatch);
    }
    Ok(())
}

pub fn select_healthy_component<'a>(
    state: &'a ActivationState,
    is_healthy: impl Fn(&ActivationEntry) -> bool,
) -> Result<&'a ActivationEntry, ComponentError> {
    if state.schema != COMPONENT_MANIFEST_SCHEMA {
        return Err(ComponentError::UnsupportedSchema);
    }
    for entry in [state.active.as_ref(), state.last_known_good.as_ref()]
        .into_iter()
        .flatten()
    {
        if validate_activation_entry(entry) && is_healthy(entry) {
            return Ok(entry);
        }
    }
    Err(ComponentError::NoHealthyComponent)
}

pub fn load_activation_state(path: &Path) -> Result<ActivationState, ComponentError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ActivationState::default())
        }
        Err(error) => return Err(error.into()),
    };
    let state: ActivationState = serde_json::from_slice(&bytes)
        .map_err(|error| ComponentError::InvalidManifest(error.to_string()))?;
    if state.schema != COMPONENT_MANIFEST_SCHEMA {
        return Err(ComponentError::UnsupportedSchema);
    }
    Ok(state)
}

fn validate_activation_entry(entry: &ActivationEntry) -> bool {
    entry.revision > 0
        && !entry.version.is_empty()
        && is_sha256(&entry.manifest_sha256)
        && is_sha256(&entry.file_sha256)
        && entry.relative_path.starts_with("yt-dlp/")
        && entry.relative_path.ends_with("/yt-dlp.exe")
        && !entry.relative_path.contains("..")
        && !entry.relative_path.contains('\\')
}

pub fn sha256_file(path: &Path) -> Result<String, ComponentError> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256_id(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_sha256)
}

fn parse_core_version(value: &str) -> Result<(u64, u64, u64), ComponentError> {
    let mut parts = value.split('.');
    let parsed = (
        parts.next().and_then(|part| part.parse().ok()),
        parts.next().and_then(|part| part.parse().ok()),
        parts.next().and_then(|part| part.parse().ok()),
    );
    if parts.next().is_some() {
        return Err(ComponentError::InvalidComponent("coreVersion"));
    }
    match parsed {
        (Some(major), Some(minor), Some(patch)) => Ok((major, minor, patch)),
        _ => Err(ComponentError::InvalidComponent("coreVersion")),
    }
}
