use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TokenLogonSid, TOKEN_GROUPS, TOKEN_QUERY};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::framing::{read_frame, write_frame};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const START_TIMEOUT: Duration = Duration::from_secs(15);

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn valid_origin(value: &str) -> bool {
    let Some(id) = value
        .strip_prefix("chrome-extension://")
        .and_then(|value| value.strip_suffix('/'))
    else {
        return false;
    };
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

pub(crate) fn origin_is_allowed(manifest: &Value, origin: &str) -> bool {
    valid_origin(origin)
        && manifest
            .get("allowed_origins")
            .and_then(Value::as_array)
            .is_some_and(|origins| {
                origins.iter().any(|allowed| {
                    allowed.as_str() == Some(origin) && allowed.as_str().is_some_and(valid_origin)
                })
            })
}

pub(crate) fn inject_client_origin(payload: &[u8], origin: &str) -> io::Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(payload)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid_request"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid_request"))?;
    object.insert(
        "clientOrigin".to_string(),
        Value::String(origin.to_string()),
    );
    let encoded = serde_json::to_vec(&value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid_request"))?;
    if encoded.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message_too_large",
        ));
    }
    Ok(encoded)
}

fn current_logon_sid() -> io::Result<String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let mut length = 0u32;
    unsafe {
        GetTokenInformation(token.0, TokenLogonSid, ptr::null_mut(), 0, &mut length);
    }
    if length < size_of::<TOKEN_GROUPS>() as u32 {
        return Err(io::Error::last_os_error());
    }
    let word = size_of::<usize>();
    let mut buffer = vec![0usize; (length as usize + word - 1) / word];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenLogonSid,
            buffer.as_mut_ptr().cast::<c_void>(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let groups = unsafe { &*(buffer.as_ptr().cast::<TOKEN_GROUPS>()) };
    if groups.GroupCount != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid logon SID",
        ));
    }
    let mut sid_text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(groups.Groups[0].Sid, &mut sid_text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut sid_length = 0usize;
    while unsafe { *sid_text.add(sid_length) } != 0 {
        sid_length += 1;
    }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_text, sid_length) });
    unsafe { LocalFree(sid_text.cast::<c_void>()) };
    Ok(sid)
}

fn pipe_name() -> io::Result<String> {
    let sid = current_logon_sid()?;
    if !sid
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-')
    {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid SID"));
    }
    Ok(format!(r"\\.\pipe\com.mab.mediadrop.companion.v1.{sid}"))
}

fn sibling(name: &str) -> io::Result<PathBuf> {
    let executable = process_image_path(std::process::id())?;
    Ok(executable
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "install directory missing"))?
        .join(name))
}

fn normalized_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_lowercase()
}

fn process_image_path(process_id: u32) -> io::Result<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = OwnedHandle(handle);
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

fn server_is_expected_app(pipe: &File) -> io::Result<bool> {
    let mut process_id = 0u32;
    let handle = pipe.as_raw_handle() as HANDLE;
    if unsafe { GetNamedPipeServerProcessId(handle, &mut process_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(normalized_path(&process_image_path(process_id)?)
        == normalized_path(&sibling("mediadrop.exe")?))
}

fn connect_pipe() -> io::Result<File> {
    let pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name()?)?;
    if !server_is_expected_app(&pipe)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "pipe_server_forbidden",
        ));
    }
    Ok(pipe)
}

fn launch_app() -> io::Result<()> {
    let app = sibling("mediadrop.exe")?;
    if !app.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "app_not_found"));
    }
    Command::new(app)
        .arg("--companion")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "app_start_failed"))
}

fn wait_for_pipe() -> io::Result<File> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match connect_pipe() {
            Ok(pipe) => return Ok(pipe),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Err(error),
            Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "app_start_timeout"));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn manifest_allows_origin(origin: &str) -> io::Result<bool> {
    for name in ["com.mab.mediadrop.json", "com.mab.mediadrop.dev.json"] {
        let path = sibling(name)?;
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_REQUEST_BYTES as u64 {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if origin_is_allowed(&manifest, origin) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn error_payload(request: &[u8], code: &str, status: &str) -> Vec<u8> {
    let value = serde_json::from_slice::<Value>(request).unwrap_or_else(|_| json!({}));
    json!({
        "messageType":"response",
        "protocolVersion":1,
        "requestId":value.get("requestId").and_then(Value::as_str).unwrap_or("00000000-0000-4000-8000-000000000000"),
        "command":value.get("command").and_then(Value::as_str).unwrap_or("unknown"),
        "status":status,
        "stateRevision":0,
        "payload":{},
        "capabilities":{},
        "error":{"code":code,"message":"MediaDrop native bridge unavailable.","retryable":true,"action":null,"reportId":null}
    })
    .to_string()
    .into_bytes()
}

fn app_starting_payload(request: &[u8]) -> Vec<u8> {
    let value = serde_json::from_slice::<Value>(request).unwrap_or_else(|_| json!({}));
    json!({
        "messageType":"event",
        "protocolVersion":1,
        "requestId":value.get("requestId").and_then(Value::as_str).unwrap_or("00000000-0000-4000-8000-000000000000"),
        "command":value.get("command").and_then(Value::as_str).unwrap_or("unknown"),
        "status":"app_starting",
        "stateRevision":0,
        "payload":{},
        "capabilities":{},
        "error":null
    })
    .to_string()
    .into_bytes()
}

pub(crate) fn run() -> io::Result<()> {
    // Chromium supplies the invoking extension origin in argv[1]. It remains
    // untrusted until the exact sibling-manifest allowlist check below passes.
    let origin = std::env::args().nth(1).unwrap_or_default();
    if !manifest_allows_origin(&origin)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native_host_forbidden",
        ));
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut pipe: Option<File> = None;

    while let Some(browser_request) = read_frame(&mut input, MAX_REQUEST_BYTES)? {
        let request = match inject_client_origin(&browser_request, &origin) {
            Ok(request) => request,
            Err(error) => {
                let code = if error.to_string() == "message_too_large" {
                    "message_too_large"
                } else {
                    "invalid_request"
                };
                write_frame(
                    &mut output,
                    &error_payload(&browser_request, code, "invalid_request"),
                    MAX_RESPONSE_BYTES,
                )?;
                continue;
            }
        };

        if pipe.is_none() {
            match connect_pipe() {
                Ok(connected) => pipe = Some(connected),
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    write_frame(
                        &mut output,
                        &error_payload(&browser_request, "pipe_server_forbidden", "error"),
                        MAX_RESPONSE_BYTES,
                    )?;
                    continue;
                }
                Err(_) => {}
            }
        }
        if pipe.is_none() {
            if let Err(error) = launch_app() {
                write_frame(
                    &mut output,
                    &error_payload(&browser_request, &error.to_string(), "error"),
                    MAX_RESPONSE_BYTES,
                )?;
                continue;
            }
            write_frame(
                &mut output,
                &app_starting_payload(&browser_request),
                MAX_RESPONSE_BYTES,
            )?;
            match wait_for_pipe() {
                Ok(connected) => pipe = Some(connected),
                Err(error) => {
                    let code = if error.kind() == io::ErrorKind::PermissionDenied {
                        "pipe_server_forbidden"
                    } else {
                        "app_start_timeout"
                    };
                    write_frame(
                        &mut output,
                        &error_payload(&browser_request, code, "error"),
                        MAX_RESPONSE_BYTES,
                    )?;
                    continue;
                }
            }
        }

        let relay = pipe
            .as_mut()
            .ok_or_else(|| io::Error::other("pipe_disconnected"));
        let response = relay.and_then(|pipe| {
            write_frame(pipe, &request, MAX_REQUEST_BYTES)?;
            read_frame(pipe, MAX_RESPONSE_BYTES)?
                .ok_or_else(|| io::Error::other("pipe_disconnected"))
        });
        match response {
            Ok(response) => write_frame(&mut output, &response, MAX_RESPONSE_BYTES)?,
            Err(_) => {
                pipe = None;
                write_frame(
                    &mut output,
                    &error_payload(&browser_request, "pipe_disconnected", "error"),
                    MAX_RESPONSE_BYTES,
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{current_logon_sid, inject_client_origin, origin_is_allowed, process_image_path};

    #[test]
    fn manifest_origin_check_is_exact_and_rejects_wildcards() {
        let manifest = json!({
            "allowed_origins": ["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]
        });
        assert!(origin_is_allowed(
            &manifest,
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        ));
        assert!(!origin_is_allowed(
            &manifest,
            "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/"
        ));
        assert!(!origin_is_allowed(
            &json!({"allowed_origins":["chrome-extension://*/"]}),
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        ));
    }

    #[test]
    fn browser_payload_cannot_spoof_the_origin_forwarded_to_the_app() {
        let payload = br#"{"command":"hello","clientOrigin":"chrome-extension://spoof/"}"#;
        let injected = inject_client_origin(
            payload,
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&injected).unwrap();
        assert_eq!(
            value["clientOrigin"],
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        );
        assert!(!String::from_utf8(injected).unwrap().contains("spoof"));
    }

    #[test]
    fn windows_identity_and_process_image_are_read_from_live_handles() {
        let sid = current_logon_sid().expect("current logon SID");
        assert!(sid.starts_with("S-"));
        assert!(sid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-'));
        assert!(process_image_path(std::process::id())
            .expect("current process image")
            .is_file());
    }
}
