use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

pub const STATUS_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerCommand {
    Cancel,
    RetryFiles,
    ContinueFiles,
    CancelFiles,
}

impl InstallerCommand {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::RetryFiles => "retry_files",
            Self::ContinueFiles => "continue_files",
            Self::CancelFiles => "cancel_files",
        }
    }
}

impl FromStr for InstallerCommand {
    type Err = io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "cancel" => Ok(Self::Cancel),
            "retry_files" => Ok(Self::RetryFiles),
            "continue_files" => Ok(Self::ContinueFiles),
            "cancel_files" => Ok(Self::CancelFiles),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown installer command",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSnapshot {
    pub protocol: u16,
    pub sequence: u64,
    pub command: InstallerCommand,
    pub response: String,
}

impl CommandSnapshot {
    pub fn write_atomic(path: &Path, sequence: u64, command: InstallerCommand) -> io::Result<()> {
        if sequence == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "command sequence must be positive",
            ));
        }
        let body = format!(
            "[command]\r\nprotocol={}\r\nsequence={}\r\ncommand={}\r\nresponse=\r\n",
            STATUS_PROTOCOL_VERSION,
            sequence,
            command.as_str()
        );
        atomic_write_utf16(path, &body)
    }
}

pub fn read_command_after(path: &Path, last_sequence: u64) -> io::Result<Option<CommandSnapshot>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = decode_utf16(&bytes)?;
    let mut protocol = None;
    let mut sequence = None;
    let mut command = None;
    let mut response = String::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "protocol" => protocol = Some(parse(value, "protocol")?),
            "sequence" => sequence = Some(parse(value, "sequence")?),
            "command" => command = Some(value.trim().parse()?),
            "response" => response = sanitize_status_text(value, 128),
            _ => {}
        }
    }
    let protocol = protocol
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "command protocol missing"))?;
    if protocol != STATUS_PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported command protocol",
        ));
    }
    let sequence = sequence
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "command sequence missing"))?;
    let command = command
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "command value missing"))?;
    if sequence <= last_sequence {
        return Ok(None);
    }
    Ok(Some(CommandSnapshot {
        protocol,
        sequence,
        command,
        response,
    }))
}

pub(crate) fn read_ini_section(path: &Path, section: &str) -> io::Result<HashMap<String, String>> {
    let text = decode_utf16(&fs::read(path)?)?;
    let mut current_section = String::new();
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_owned();
            continue;
        }
        if current_section != section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !key.is_empty() {
            values.insert(key.to_owned(), value.trim().to_owned());
        }
    }
    Ok(values)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallerState {
    Extracting,
    StartingBroker,
    AwaitingElevation,
    Handshaking,
    VerifyingPayload,
    PreparingInstaller,
    Installing,
    FilesInUse,
    CancelPending,
    RollingBack,
    Succeeded,
    Failed,
    ElevationCancelled,
}

impl InstallerState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::ElevationCancelled
        )
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Extracting => "extracting",
            Self::StartingBroker => "starting_broker",
            Self::AwaitingElevation => "awaiting_elevation",
            Self::Handshaking => "handshaking",
            Self::VerifyingPayload => "verifying_payload",
            Self::PreparingInstaller => "preparing_installer",
            Self::Installing => "installing",
            Self::FilesInUse => "files_in_use",
            Self::CancelPending => "cancel_pending",
            Self::RollingBack => "rolling_back",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::ElevationCancelled => "elevation_cancelled",
        }
    }
}

impl fmt::Display for InstallerState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for InstallerState {
    type Err = io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "extracting" => Ok(Self::Extracting),
            "starting_broker" => Ok(Self::StartingBroker),
            "awaiting_elevation" => Ok(Self::AwaitingElevation),
            "handshaking" => Ok(Self::Handshaking),
            "verifying_payload" => Ok(Self::VerifyingPayload),
            "preparing_installer" => Ok(Self::PreparingInstaller),
            "installing" => Ok(Self::Installing),
            "files_in_use" => Ok(Self::FilesInUse),
            "cancel_pending" => Ok(Self::CancelPending),
            "rolling_back" => Ok(Self::RollingBack),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "elevation_cancelled" => Ok(Self::ElevationCancelled),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown installer state",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub protocol: u16,
    pub sequence: u64,
    pub state: InstallerState,
    pub heartbeat: u64,
    pub progress: u8,
    pub phase: String,
    pub action: String,
    pub result_kind: String,
    pub win32_code: u32,
    pub msi_code: u32,
    pub reboot_required: bool,
    pub log_path: String,
    pub broker_pid: u32,
    pub worker_pid: u32,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            protocol: STATUS_PROTOCOL_VERSION,
            sequence: 0,
            state: InstallerState::StartingBroker,
            heartbeat: 0,
            progress: 0,
            phase: String::new(),
            action: String::new(),
            result_kind: String::new(),
            win32_code: 0,
            msi_code: 0,
            reboot_required: false,
            log_path: String::new(),
            broker_pid: 0,
            worker_pid: 0,
        }
    }
}

impl StatusSnapshot {
    pub fn for_state(state: InstallerState) -> Self {
        Self {
            state,
            ..Self::default()
        }
    }

    fn normalize(&mut self) {
        self.protocol = STATUS_PROTOCOL_VERSION;
        self.progress = self.progress.min(100);
        self.phase = sanitize_status_text(&self.phase, 256);
        self.action = sanitize_status_text(&self.action, 512);
        self.result_kind = sanitize_status_text(&self.result_kind, 64);
        self.log_path = sanitize_status_text(&self.log_path, 1024);
    }
}

pub struct StatusStore {
    path: PathBuf,
    sequence: u64,
}

impl StatusStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path, sequence: 0 }
    }

    pub fn publish(&mut self, mut snapshot: StatusSnapshot) -> io::Result<StatusSnapshot> {
        self.sequence = self.sequence.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "status sequence overflow")
        })?;
        snapshot.sequence = self.sequence;
        snapshot.heartbeat = unix_millis();
        snapshot.normalize();
        atomic_write_utf16(&self.path, &encode_status(&snapshot))?;
        Ok(snapshot)
    }
}

pub fn sanitize_status_text(value: &str, maximum_chars: usize) -> String {
    let mut cleaned = String::with_capacity(value.len().min(maximum_chars));
    let mut pending_space = false;
    for character in value.trim().chars() {
        if cleaned.chars().count() >= maximum_chars {
            break;
        }
        if character.is_control() || matches!(character, '[' | ']' | '=') {
            pending_space = !cleaned.is_empty();
            continue;
        }
        if character.is_whitespace() {
            pending_space = !cleaned.is_empty();
            continue;
        }
        if pending_space && cleaned.chars().count() < maximum_chars {
            cleaned.push(' ');
        }
        pending_space = false;
        if cleaned.chars().count() < maximum_chars {
            cleaned.push(character);
        }
    }
    cleaned.trim_end().to_owned()
}

pub fn read_status(path: &Path) -> io::Result<StatusSnapshot> {
    let bytes = fs::read(path)?;
    let text = decode_utf16(&bytes)?;
    let mut snapshot = StatusSnapshot::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "protocol" => snapshot.protocol = parse(value, "protocol")?,
            "sequence" => snapshot.sequence = parse(value, "sequence")?,
            "state" => snapshot.state = value.trim().parse()?,
            "heartbeat" => snapshot.heartbeat = parse(value, "heartbeat")?,
            "progress" => snapshot.progress = parse(value, "progress")?,
            "phase" => snapshot.phase = value.trim().to_owned(),
            "action" => snapshot.action = value.trim().to_owned(),
            "result_kind" => snapshot.result_kind = value.trim().to_owned(),
            "win32_code" => snapshot.win32_code = parse(value, "win32_code")?,
            "msi_code" => snapshot.msi_code = parse(value, "msi_code")?,
            "reboot_required" => snapshot.reboot_required = parse_bool(value)?,
            "log_path" => snapshot.log_path = value.trim().to_owned(),
            "broker_pid" => snapshot.broker_pid = parse(value, "broker_pid")?,
            "worker_pid" => snapshot.worker_pid = parse(value, "worker_pid")?,
            _ => {}
        }
    }
    if snapshot.protocol != STATUS_PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported status protocol",
        ));
    }
    Ok(snapshot)
}

fn encode_status(status: &StatusSnapshot) -> String {
    format!(
        "[status]\r\nprotocol={}\r\nsequence={}\r\nstate={}\r\nheartbeat={}\r\nprogress={}\r\nphase={}\r\naction={}\r\nresult_kind={}\r\nwin32_code={}\r\nmsi_code={}\r\nreboot_required={}\r\nlog_path={}\r\nbroker_pid={}\r\nworker_pid={}\r\n",
        status.protocol,
        status.sequence,
        status.state,
        status.heartbeat,
        status.progress,
        status.phase,
        status.action,
        status.result_kind,
        status.win32_code,
        status.msi_code,
        u8::from(status.reboot_required),
        status.log_path,
        status.broker_pid,
        status.worker_pid,
    )
}

fn atomic_write_utf16(path: &Path, value: &str) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "status path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".status-{}-{}.tmp",
        std::process::id(),
        unix_millis()
    ));

    let mut bytes = Vec::with_capacity(value.len() * 2 + 2);
    bytes.extend_from_slice(&[0xff, 0xfe]);
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        atomic_replace(&temporary, path)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
    for attempt in 0..100 {
        // SAFETY: both UTF-16 buffers are NUL-terminated and remain alive for the call.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(5 | 32 | 33)) || attempt == 99 {
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    unreachable!("bounded atomic replace loop always returns")
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)
}

fn decode_utf16(bytes: &[u8]) -> io::Result<String> {
    if bytes.len() < 2 || bytes[..2] != [0xff, 0xfe] || !bytes.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "status file is not UTF-16LE",
        ));
    }
    let units = bytes[2..]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-16 status"))
}

fn parse<T: FromStr>(value: &str, field: &'static str) -> io::Result<T> {
    value
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("invalid {field} field")))
}

fn parse_bool(value: &str) -> io::Result<bool> {
    match value.trim() {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid boolean field",
        )),
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
