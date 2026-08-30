use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{error::Error, fmt, io};

use crate::status::InstallerState;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge { length: usize, maximum: usize },
    UnsupportedVersion { received: u16 },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "protocol I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "invalid protocol JSON: {error}"),
            Self::FrameTooLarge { length, maximum } => {
                write!(formatter, "frame is {length} bytes; maximum is {maximum}")
            }
            Self::UnsupportedVersion { received } => {
                write!(formatter, "unsupported protocol version {received}")
            }
        }
    }
}

impl Error for ProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::FrameTooLarge { .. } | Self::UnsupportedVersion { .. } => None,
        }
    }
}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "command", content = "payload")]
pub enum BrokerCommand {
    StartInstall,
    StartComponentInstall { session_dir: String },
    Cancel,
    Abort,
    FilesInUse { response: FilesInUseResponse },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesInUseResponse {
    Retry,
    Continue,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerMessage {
    pub protocol: u16,
    pub sequence: u64,
    #[serde(flatten)]
    pub command: BrokerCommand,
}

impl BrokerMessage {
    pub fn new(sequence: u64, command: BrokerCommand) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            sequence,
            command,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event", content = "payload")]
pub enum WorkerEvent {
    Hello(WorkerHello),
    Status(WorkerStatus),
    Complete(InstallResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerMessage {
    pub protocol: u16,
    pub sequence: u64,
    #[serde(flatten)]
    pub event: WorkerEvent,
}

impl WorkerMessage {
    pub fn new(sequence: u64, event: WorkerEvent) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            sequence,
            event,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_version(self.protocol)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerHello {
    pub session_id: String,
    pub secret: String,
    pub worker_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub state: InstallerState,
    pub progress: u8,
    pub phase: String,
    pub action: String,
    #[serde(default)]
    pub log_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallResult {
    pub result_kind: ResultKind,
    pub win32_code: u32,
    pub msi_code: u32,
    pub reboot_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Succeeded,
    MsiCancelled,
    AbortedBeforeInstall,
    InstallerBusy,
    Failed,
}

pub fn validate_version(version: u16) -> Result<(), ProtocolError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::UnsupportedVersion { received: version })
    }
}

pub fn write_frame<W: io::Write, T: Serialize>(
    writer: &mut W,
    message: &T,
) -> Result<(), ProtocolError> {
    let payload = serde_json::to_vec(message)?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge {
            length: payload.len(),
            maximum: MAX_FRAME_SIZE,
        });
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: io::Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, ProtocolError> {
    let mut length_bytes = [0_u8; 4];
    reader.read_exact(&mut length_bytes)?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge {
            length,
            maximum: MAX_FRAME_SIZE,
        });
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(serde_json::from_slice(&payload)?)
}
