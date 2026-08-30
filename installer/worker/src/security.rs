use crate::protocol::WorkerHello;
use std::{error::Error, fmt, io, path::PathBuf};

const SECRET_BYTES: usize = 32;

#[derive(Clone, PartialEq, Eq)]
pub struct SessionSecret(String);

impl fmt::Debug for SessionSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionSecret([redacted])")
    }
}

impl SessionSecret {
    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        let mut encoded = String::with_capacity(SECRET_BYTES * 2);
        for byte in bytes {
            use fmt::Write as _;
            let _ = write!(encoded, "{byte:02x}");
        }
        Self(encoded)
    }

    pub fn parse_hex(value: &str) -> Result<Self, SecurityError> {
        if value.len() != SECRET_BYTES * 2
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SecurityError::InvalidSecret);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }

    fn constant_time_eq(&self, candidate: &str) -> bool {
        if candidate.len() != self.0.len() {
            return false;
        }
        self.0
            .bytes()
            .zip(candidate.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }
}

#[derive(Debug)]
pub enum SecurityError {
    Random(io::Error),
    InvalidSecret,
    InvalidSessionId,
    HandshakeRejected,
}

impl fmt::Display for SecurityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random(error) => write!(formatter, "secure random generation failed: {error}"),
            Self::InvalidSecret => formatter.write_str("invalid session secret"),
            Self::InvalidSessionId => formatter.write_str("invalid installer session identifier"),
            Self::HandshakeRejected => formatter.write_str("elevated worker handshake rejected"),
        }
    }
}

impl Error for SecurityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Random(error) => Some(error),
            _ => None,
        }
    }
}

pub fn generate_secret() -> Result<SessionSecret, SecurityError> {
    #[cfg(windows)]
    {
        use std::ptr;
        use windows_sys::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        let mut bytes = [0_u8; SECRET_BYTES];
        // SAFETY: bytes is a writable 32-byte buffer; a null algorithm handle is required with
        // BCRYPT_USE_SYSTEM_PREFERRED_RNG.
        let status = unsafe {
            BCryptGenRandom(
                ptr::null_mut(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            return Err(SecurityError::Random(io::Error::from_raw_os_error(status)));
        }
        Ok(SessionSecret::from_bytes(bytes))
    }
    #[cfg(not(windows))]
    {
        Err(SecurityError::Random(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure Windows random source unavailable",
        )))
    }
}

pub fn validate_session_id(value: &str) -> Result<(), SecurityError> {
    if value.len() != 36 {
        return Err(SecurityError::InvalidSessionId);
    }
    for (index, byte) in value.bytes().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return Err(SecurityError::InvalidSessionId);
            }
        } else if !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err(SecurityError::InvalidSessionId);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    pub process_id: u32,
    pub elevated: bool,
    pub windows_session_id: u32,
    pub executable_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeExpectation {
    pub session_id: String,
    pub secret: SessionSecret,
    pub windows_session_id: u32,
    pub executable_path: PathBuf,
}

pub fn validate_handshake(
    hello: &WorkerHello,
    pipe_client_pid: u32,
    peer: &PeerIdentity,
    expected: &HandshakeExpectation,
) -> Result<(), SecurityError> {
    validate_session_id(&hello.session_id)?;
    if hello.session_id != expected.session_id
        || !expected.secret.constant_time_eq(&hello.secret)
        || hello.worker_pid == 0
        || hello.worker_pid != pipe_client_pid
        || hello.worker_pid != peer.process_id
        || !peer.elevated
        || peer.windows_session_id != expected.windows_session_id
        || normalized_path(&peer.executable_path) != normalized_path(&expected.executable_path)
    {
        return Err(SecurityError::HandshakeRejected);
    }
    Ok(())
}

fn normalized_path(path: &std::path::Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase()
}
