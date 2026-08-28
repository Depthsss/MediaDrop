use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::thread;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenLogonSid, SECURITY_ATTRIBUTES, TOKEN_GROUPS, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenEventW, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    SetEvent, EVENT_MODIFY_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::companion::protocol::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};

const PIPE_WORKERS: usize = 4;
const INSTALLER_EXTENSION_CONNECTED_EVENT: &str =
    r"Local\MediaDrop.ExtensionSetup.Connected.v1";

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn signal_named_event(name: &str) -> bool {
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, wide(name).as_ptr()) };
    if handle.is_null() {
        return false;
    }
    let handle = OwnedHandle(handle);
    unsafe { SetEvent(handle.0) != 0 }
}

pub(crate) fn signal_installer_extension_connection() {
    let _ = signal_named_event(INSTALLER_EXTENSION_CONNECTED_EVENT);
}

pub(crate) fn current_logon_sid() -> io::Result<String> {
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

pub(crate) fn pipe_name_for_sid(sid: &str) -> io::Result<String> {
    if sid.is_empty()
        || sid.len() > 184
        || !sid.starts_with("S-")
        || !sid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-')
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid SID"));
    }
    Ok(format!(r"\\.\pipe\com.mab.mediadrop.companion.v1.{sid}"))
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

fn normalized_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_lowercase()
}

fn expected_native_host_path() -> io::Result<PathBuf> {
    let executable = process_image_path(std::process::id())?;
    Ok(executable
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "install directory missing"))?
        .join("mediadrop-native-host.exe"))
}

fn client_is_expected_native_host(pipe: HANDLE) -> io::Result<bool> {
    let mut process_id = 0u32;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut process_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(normalized_path(&process_image_path(process_id)?)
        == normalized_path(&expected_native_host_path()?))
}

fn security_descriptor(sid: &str) -> io::Result<*mut c_void> {
    let sddl = wide(format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"));
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(descriptor)
}

fn create_pipe(name: &str, sid: &str) -> io::Result<OwnedHandle> {
    let descriptor = security_descriptor(sid)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateNamedPipeW(
            wide(name).as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            MAX_RESPONSE_BYTES as u32 + 4,
            MAX_REQUEST_BYTES as u32 + 4,
            0,
            &attributes,
        )
    };
    unsafe { LocalFree(descriptor) };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(OwnedHandle(handle))
    }
}

fn read_exact(pipe: HANDLE, bytes: &mut [u8]) -> io::Result<()> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                pipe,
                bytes[offset..].as_mut_ptr(),
                (bytes.len() - offset).min(u32::MAX as usize) as u32,
                &mut read,
                ptr::null_mut(),
            )
        };
        if ok == 0 || read == 0 {
            return Err(io::Error::last_os_error());
        }
        offset += read as usize;
    }
    Ok(())
}

fn write_all(pipe: HANDLE, bytes: &[u8]) -> io::Result<()> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                pipe,
                bytes[offset..].as_ptr(),
                (bytes.len() - offset).min(u32::MAX as usize) as u32,
                &mut written,
                ptr::null_mut(),
            )
        };
        if ok == 0 || written == 0 {
            return Err(io::Error::last_os_error());
        }
        offset += written as usize;
    }
    Ok(())
}

fn serve_connection(app: &tauri::AppHandle, pipe: HANDLE) -> io::Result<()> {
    loop {
        let mut prefix = [0u8; 4];
        if let Err(error) = read_exact(pipe, &mut prefix) {
            if error.raw_os_error().map(|value| value as u32) == Some(ERROR_BROKEN_PIPE) {
                return Ok(());
            }
            return Err(error);
        }
        let length = u32::from_le_bytes(prefix) as usize;
        if length == 0 || length > MAX_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid frame length",
            ));
        }
        let mut request = vec![0u8; length];
        read_exact(pipe, &mut request)?;
        let response = super::handle_request(app, &request);
        if response.is_empty() || response.len() > MAX_RESPONSE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid response length",
            ));
        }
        write_all(pipe, &(response.len() as u32).to_le_bytes())?;
        write_all(pipe, &response)?;
        unsafe { FlushFileBuffers(pipe) };
    }
}

fn server_worker(app: tauri::AppHandle, name: String, sid: String) {
    loop {
        let Ok(pipe) = create_pipe(&name, &sid) else {
            return;
        };
        let connected = unsafe { ConnectNamedPipe(pipe.0, ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if connected && client_is_expected_native_host(pipe.0).unwrap_or(false) {
            let _ = serve_connection(&app, pipe.0);
        }
        unsafe { DisconnectNamedPipe(pipe.0) };
    }
}

pub(crate) fn start_server(app: tauri::AppHandle) -> io::Result<()> {
    let sid = current_logon_sid()?;
    let name = pipe_name_for_sid(&sid)?;
    for _ in 0..PIPE_WORKERS {
        let app = app.clone();
        let name = name.clone();
        let sid = sid.clone();
        thread::Builder::new()
            .name("mediadrop-companion-pipe".to_string())
            .spawn(move || server_worker(app, name, sid))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    use super::{create_pipe, current_logon_sid, pipe_name_for_sid, signal_named_event, wide};

    #[test]
    fn pipe_name_is_versioned_and_scoped_without_path_metacharacters() {
        assert_eq!(
            pipe_name_for_sid("S-1-5-21-100-200-300-1001").unwrap(),
            r"\\.\pipe\com.mab.mediadrop.companion.v1.S-1-5-21-100-200-300-1001"
        );
        assert!(pipe_name_for_sid(r"S-1-5-21\evil").is_err());
    }

    #[test]
    fn current_windows_identity_produces_a_sid_safe_for_the_pipe_name() {
        let sid = current_logon_sid().expect("current logon SID");
        assert!(sid.starts_with("S-"));
        assert!(pipe_name_for_sid(&sid).is_ok());
    }

    #[test]
    fn current_logon_sid_builds_a_restricted_named_pipe() {
        let sid = current_logon_sid().expect("current logon SID");
        let name = format!(
            r"\\.\pipe\com.mab.mediadrop.companion.test.{}",
            std::process::id()
        );
        let _pipe = create_pipe(&name, &sid).expect("restricted named pipe");
    }

    #[test]
    fn signals_an_existing_installer_connection_event() {
        let name = format!(
            r"Local\MediaDrop.ExtensionSetup.Test.{}.{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        let handle = unsafe { CreateEventW(ptr::null(), 1, 0, wide(&name).as_ptr()) };
        assert!(!handle.is_null());

        assert!(signal_named_event(&name));
        assert_eq!(unsafe { WaitForSingleObject(handle, 0) }, WAIT_OBJECT_0);

        unsafe { CloseHandle(handle) };
    }
}
