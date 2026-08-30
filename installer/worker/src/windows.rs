use crate::security::PeerIdentity;
use std::{
    ffi::c_void,
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    Security::{
        GetTokenInformation, TokenElevation, TokenLogonSid, SECURITY_ATTRIBUTES, TOKEN_ELEVATION,
        TOKEN_GROUPS, TOKEN_QUERY,
    },
    Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, PeekNamedPipe, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            CreateMutexW, GetCurrentProcess, OpenProcess, OpenProcessToken,
            QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

pub const MACHINE_MUTEX_NAME: &str = r"Global\MediaDrop.Installer.Worker.v1";
pub const COMPONENT_MUTEX_NAME: &str = r"Global\MediaDrop.Component.Worker.v1";
const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

pub struct OwnedHandle(pub HANDLE);

// Kernel handles are process-scoped values and may be passed between threads.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: OwnedHandle uniquely owns this live kernel handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}

impl OwnedHandle {
    pub fn is_signaled(&self) -> bool {
        // SAFETY: the handle remains live for this non-blocking wait.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
}

pub struct PipeServer(OwnedHandle);

unsafe impl Send for PipeServer {}

impl PipeServer {
    pub fn raw_handle(&self) -> HANDLE {
        self.0 .0
    }

    pub fn connect(self) -> io::Result<std::fs::File> {
        // SAFETY: this is a listening named-pipe server handle. A null OVERLAPPED requests a
        // synchronous connection because the pipe was not created for overlapped I/O.
        let connected = unsafe { ConnectNamedPipe(self.0 .0, ptr::null_mut()) };
        if connected == 0 {
            // SAFETY: GetLastError is read immediately after ConnectNamedPipe failed.
            let code = unsafe { GetLastError() };
            if code != ERROR_PIPE_CONNECTED {
                return Err(io::Error::from_raw_os_error(code as i32));
            }
        }
        let raw = self.0 .0;
        std::mem::forget(self.0);
        // SAFETY: ownership of the connected pipe handle transfers into File exactly once.
        Ok(unsafe { std::fs::File::from_raw_handle(raw) })
    }
}

pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

pub fn open_parent_process(process_id: u32) -> io::Result<OwnedHandle> {
    if process_id == 0 || process_id == std::process::id() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid parent process id",
        ));
    }
    // SAFETY: OpenProcess validates the PID; only synchronization rights are requested.
    let handle = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, process_id) };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(OwnedHandle(handle))
    }
}

pub fn current_logon_sid() -> io::Result<String> {
    let mut token = ptr::null_mut();
    // SAFETY: token output points to initialized handle storage.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let mut length = 0_u32;
    // SAFETY: a null buffer probes the required size.
    unsafe {
        GetTokenInformation(token.0, TokenLogonSid, ptr::null_mut(), 0, &mut length);
    }
    if length < size_of::<TOKEN_GROUPS>() as u32 {
        return Err(io::Error::last_os_error());
    }
    let word = size_of::<usize>();
    let mut buffer = vec![0_usize; (length as usize).div_ceil(word)];
    // SAFETY: the aligned buffer is at least `length` bytes and remains live for the call.
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
    // SAFETY: GetTokenInformation initialized a TOKEN_GROUPS structure in the aligned buffer.
    let groups = unsafe { &*(buffer.as_ptr().cast::<TOKEN_GROUPS>()) };
    if groups.GroupCount != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid logon SID",
        ));
    }
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    let mut sid_text = ptr::null_mut();
    // SAFETY: the token-owned SID remains live while it is converted.
    if unsafe { ConvertSidToStringSidW(groups.Groups[0].Sid, &mut sid_text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut sid_length = 0;
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated LocalAlloc buffer.
    while unsafe { *sid_text.add(sid_length) } != 0 {
        sid_length += 1;
    }
    // SAFETY: sid_length was measured within the NUL-terminated allocation.
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_text, sid_length) });
    // SAFETY: LocalFree releases the buffer allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(sid_text.cast::<c_void>()) };
    validate_sid(&sid)?;
    Ok(sid)
}

pub fn validate_sid(sid: &str) -> io::Result<()> {
    if sid.is_empty()
        || sid.len() > 184
        || !sid.starts_with("S-")
        || !sid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-')
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid SID"));
    }
    Ok(())
}

fn security_descriptor(sid: &str) -> io::Result<*mut c_void> {
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    validate_sid(sid)?;
    let sddl = wide(format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})"));
    let mut descriptor = ptr::null_mut();
    // SAFETY: the SDDL buffer is NUL-terminated and descriptor is writable output storage.
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

pub fn create_restricted_pipe(name: &str, sid: &str) -> io::Result<PipeServer> {
    if !(name.starts_with(r"\\.\pipe\MediaDrop.Installer.v1.")
        || name.starts_with(r"\\.\pipe\MediaDrop.Component.v1."))
        || name.len() > 240
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'\\' | b'.' | b'-'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid installer pipe name",
        ));
    }
    let descriptor = security_descriptor(sid)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: all pointers remain live; the descriptor has a valid protected DACL.
    let handle = unsafe {
        CreateNamedPipeW(
            wide(name).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            (crate::protocol::MAX_FRAME_SIZE + 4) as u32,
            (crate::protocol::MAX_FRAME_SIZE + 4) as u32,
            0,
            &attributes,
        )
    };
    // SAFETY: descriptor was allocated with LocalAlloc by the SDDL conversion API.
    unsafe { LocalFree(descriptor) };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(PipeServer(OwnedHandle(handle)))
    }
}

pub fn connect_pipe(name: &str, timeout: Duration) -> io::Result<std::fs::File> {
    let deadline = Instant::now() + timeout;
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
        {
            Ok(file) => return Ok(file),
            Err(error) if Instant::now() < deadline => {
                let code = error.raw_os_error().unwrap_or_default();
                if !matches!(code, 2 | 3 | 231) {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn named_pipe_client_pid(pipe: &std::fs::File) -> io::Result<u32> {
    let mut process_id = 0_u32;
    // SAFETY: pipe is a connected server handle and output storage is valid.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut process_id) } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(process_id)
    }
}

pub fn named_pipe_server_pid(pipe: &std::fs::File) -> io::Result<u32> {
    let mut process_id = 0_u32;
    // SAFETY: pipe is a connected client handle and output storage is valid.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut process_id) } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(process_id)
    }
}

pub fn pipe_available(pipe: &std::fs::File) -> io::Result<u32> {
    let mut available = 0_u32;
    // SAFETY: the pipe handle is live and only total-bytes-available output is requested.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle() as HANDLE,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(available)
    }
}

pub fn process_image_path(process_id: u32) -> io::Result<PathBuf> {
    // SAFETY: OpenProcess validates the PID and grants only query access.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = OwnedHandle(process);
    let mut buffer = vec![0_u16; 32768];
    let mut length = buffer.len() as u32;
    // SAFETY: buffer and length describe writable UTF-16 storage.
    if unsafe { QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buffer[..length as usize],
    )))
}

pub fn process_session_id(process_id: u32) -> io::Result<u32> {
    let mut session = 0_u32;
    // SAFETY: output storage is valid and ProcessIdToSessionId validates the PID.
    if unsafe { ProcessIdToSessionId(process_id, &mut session) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(session)
    }
}

pub fn process_is_elevated(process_id: u32) -> io::Result<bool> {
    // SAFETY: OpenProcess validates the PID and grants only query access.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = OwnedHandle(process);
    let mut token = ptr::null_mut();
    // SAFETY: output token storage is valid and process stays alive for the call.
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0_u32;
    // SAFETY: elevation points to a correctly sized TOKEN_ELEVATION buffer.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(elevation.TokenIsElevated != 0)
}

pub fn peer_identity(process_id: u32) -> io::Result<PeerIdentity> {
    Ok(PeerIdentity {
        process_id,
        elevated: process_is_elevated(process_id)?,
        windows_session_id: process_session_id(process_id)?,
        executable_path: process_image_path(process_id)?,
    })
}

pub fn create_machine_mutex() -> io::Result<OwnedHandle> {
    create_named_mutex(MACHINE_MUTEX_NAME)
}

pub fn create_component_mutex() -> io::Result<OwnedHandle> {
    create_named_mutex(COMPONENT_MUTEX_NAME)
}

fn create_named_mutex(name: &str) -> io::Result<OwnedHandle> {
    // SAFETY: the name is NUL-terminated and no SECURITY_ATTRIBUTES are supplied.
    let handle = unsafe { CreateMutexW(ptr::null(), 0, wide(name).as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetLastError is read immediately after CreateMutexW.
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = OwnedHandle(handle);
    if already_exists {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another MediaDrop worker is active",
        ))
    } else {
        Ok(handle)
    }
}

pub fn launch_elevated(executable: &Path, arguments: &[String]) -> io::Result<Option<OwnedHandle>> {
    use windows_sys::Win32::{
        System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        },
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_HIDE,
        },
    };

    if !executable.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "worker path is not absolute",
        ));
    }
    let parameters = arguments
        .iter()
        .map(|argument| quote_windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let verb = wide("runas");
    let executable = wide(executable.as_os_str());
    let parameters = wide(parameters);
    // SAFETY: this dedicated native thread has not initialized COM. We uninitialize only after a
    // successful initialization result.
    let com_result = unsafe {
        CoInitializeEx(
            ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    if com_result < 0 {
        return Err(io::Error::from_raw_os_error(com_result));
    }
    // SAFETY: zero is a valid initial state for SHELLEXECUTEINFOW before required fields are set.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: all buffers and the structure remain live for the synchronous shell call.
    let launched = unsafe { ShellExecuteExW(&mut info) };
    let result = if launched == 0 {
        Err(io::Error::last_os_error())
    } else if info.hProcess.is_null() {
        Ok(None)
    } else {
        Ok(Some(OwnedHandle(info.hProcess)))
    };
    // SAFETY: COM initialization succeeded on this thread.
    unsafe { CoUninitialize() };
    result
}

pub fn quote_windows_argument(value: &str) -> String {
    if !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return value.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
        } else if character == '"' {
            quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
            quoted.push('"');
            backslashes = 0;
        } else {
            quoted.extend(std::iter::repeat_n('\\', backslashes));
            backslashes = 0;
            quoted.push(character);
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

pub fn installer_program_data_paths(
    version: &str,
    session_id: &str,
    interactive_user_sid: &str,
) -> io::Result<(PathBuf, PathBuf)> {
    crate::security::validate_session_id(session_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid installer session id"))?;
    validate_sid(interactive_user_sid)?;
    if version.is_empty()
        || version.len() > 32
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid installer version",
        ));
    }
    let program_data = known_program_data()?;
    ensure_plain_directory(&program_data)?;
    let product_root = program_data.join("MediaDrop");
    ensure_plain_directory(&product_root)?;

    let cache_root = product_root.join("InstallerCache");
    secure_directory(&cache_root, interactive_user_sid)?;
    let cache_version = cache_root.join(version);
    secure_directory(&cache_version, interactive_user_sid)?;
    cleanup_stale_cache_sessions(&cache_version, session_id);
    let session_cache = cache_version.join(session_id);
    secure_directory(&session_cache, interactive_user_sid)?;

    let log_root = product_root.join("InstallerLogs");
    secure_directory(&log_root, interactive_user_sid)?;
    Ok((session_cache, log_root))
}

pub fn component_store_path(interactive_user_sid: &str) -> io::Result<PathBuf> {
    validate_sid(interactive_user_sid)?;
    let program_data = known_program_data()?;
    ensure_plain_directory(&program_data)?;
    let product_root = program_data.join("MediaDrop");
    ensure_plain_directory(&product_root)?;
    let component_root = product_root.join("Components");
    secure_directory(&component_root, interactive_user_sid)?;
    Ok(component_root)
}

fn cleanup_stale_cache_sessions(version_root: &Path, active_session_id: &str) {
    use std::{os::windows::fs::MetadataExt, time::Duration};
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let Ok(entries) = std::fs::read_dir(version_root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name == active_session_id || crate::security::validate_session_id(&name).is_err() {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || metadata
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_none_or(|age| age < Duration::from_secs(7 * 24 * 60 * 60))
        {
            continue;
        }
        let msi = path.join("MediaDrop.msi");
        if std::fs::symlink_metadata(&msi).is_ok_and(|file| {
            file.is_file() && file.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        }) {
            let _ = std::fs::remove_file(msi);
        }
        let _ = std::fs::remove_dir(path);
    }
}

fn known_program_data() -> io::Result<PathBuf> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
    };
    let mut raw = ptr::null_mut();
    // SAFETY: FOLDERID_ProgramData is a valid known-folder GUID and output storage is valid.
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        return Err(io::Error::from_raw_os_error(result));
    }
    let mut length = 0;
    // SAFETY: SHGetKnownFolderPath returns a NUL-terminated CoTaskMem string.
    while unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: length was measured within the returned allocation.
    let path = PathBuf::from(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    // SAFETY: raw was allocated with the COM task allocator.
    unsafe { CoTaskMemFree(raw.cast()) };
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ProgramData path is not absolute",
        ));
    }
    Ok(path)
}

fn ensure_plain_directory(path: &Path) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::ERROR_ALREADY_EXISTS,
        Storage::FileSystem::{
            CreateDirectoryW, GetFileAttributesW, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
        },
    };
    let path_wide = wide(path.as_os_str());
    // SAFETY: path is a NUL-terminated UTF-16 buffer.
    if unsafe { CreateDirectoryW(path_wide.as_ptr(), ptr::null()) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error);
        }
    }
    // SAFETY: path_wide stays live for the attribute query.
    let attributes = unsafe { GetFileAttributesW(path_wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES
        || attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "installer directory is missing, not a directory, or a reparse point",
        ));
    }
    Ok(())
}

fn secure_directory(path: &Path, user_sid: &str) -> io::Result<()> {
    use windows_sys::Win32::{
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
                SDDL_REVISION_1, SE_FILE_OBJECT,
            },
            GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::CreateDirectoryW,
    };

    let sddl = wide(format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;{user_sid})"
    ));
    let mut descriptor = ptr::null_mut();
    // SAFETY: SDDL and output storage are valid for the conversion call.
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
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let path_wide = wide(path.as_os_str());
    // SAFETY: the path and security descriptor remain live for directory creation.
    let created = unsafe { CreateDirectoryW(path_wide.as_ptr(), &attributes) };
    if created == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            // SAFETY: descriptor came from LocalAlloc.
            unsafe { LocalFree(descriptor) };
            return Err(error);
        }
    }
    if let Err(error) = ensure_plain_directory(path) {
        // SAFETY: descriptor came from LocalAlloc.
        unsafe { LocalFree(descriptor) };
        return Err(error);
    }

    let mut dacl_present = 0;
    let mut dacl_defaulted = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: descriptor is valid and output pointers remain live.
    if unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    } == 0
        || dacl_present == 0
        || dacl.is_null()
    {
        // SAFETY: descriptor came from LocalAlloc.
        unsafe { LocalFree(descriptor) };
        return Err(io::Error::last_os_error());
    }
    // SAFETY: dacl points inside the live descriptor; SetNamedSecurityInfoW copies/applies it
    // synchronously to the non-reparse directory checked above.
    let code = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    // SAFETY: descriptor came from LocalAlloc and is no longer needed.
    unsafe { LocalFree(descriptor) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    Ok(())
}
