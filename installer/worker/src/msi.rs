use crate::{protocol::FilesInUseResponse, status::sanitize_status_text};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Succeeded { reboot: bool },
    Cancelled,
    InstallerBusy,
    ElevationCancelled,
    Failed { code: u32 },
}

pub fn map_result_code(code: u32) -> InstallOutcome {
    match code {
        0 => InstallOutcome::Succeeded { reboot: false },
        3010 | 1641 => InstallOutcome::Succeeded { reboot: true },
        1602 => InstallOutcome::Cancelled,
        1618 => InstallOutcome::InstallerBusy,
        1223 => InstallOutcome::ElevationCancelled,
        code => InstallOutcome::Failed { code },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelState {
    Active,
    Requested,
    RollingBack,
    Complete(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelLifecycle {
    state: CancelState,
}

impl Default for CancelLifecycle {
    fn default() -> Self {
        Self {
            state: CancelState::Active,
        }
    }
}

impl CancelLifecycle {
    pub fn request(&mut self) {
        if self.state == CancelState::Active {
            self.state = CancelState::Requested;
        }
    }

    pub fn mark_rolling_back(&mut self) {
        if matches!(self.state, CancelState::Requested | CancelState::Active) {
            self.state = CancelState::RollingBack;
        }
    }

    pub fn complete(&mut self, result_code: u32) {
        self.state = CancelState::Complete(result_code);
    }

    pub fn is_cancel_requested(self) -> bool {
        matches!(
            self.state,
            CancelState::Requested | CancelState::RollingBack
        )
    }

    pub fn is_rolling_back(self) -> bool {
        self.state == CancelState::RollingBack
    }

    pub fn is_terminal(self) -> bool {
        matches!(self.state, CancelState::Complete(_))
    }

    pub fn result_code(self) -> Option<u32> {
        match self.state {
            CancelState::Complete(code) => Some(code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressTracker {
    initialized: bool,
    total_ticks: i64,
    completed_ticks: i64,
    direction: ProgressDirection,
    action_data_ticks: i64,
    action_data_enabled: bool,
}

impl Default for ProgressTracker {
    fn default() -> Self {
        Self {
            initialized: false,
            total_ticks: 0,
            completed_ticks: 0,
            direction: ProgressDirection::Forward,
            action_data_ticks: 0,
            action_data_enabled: false,
        }
    }
}

impl ProgressTracker {
    pub fn on_progress_record(&mut self, fields: [i32; 4]) -> Option<u8> {
        match fields[0] {
            0 if fields[1] > 0 && matches!(fields[2], 0 | 1) => {
                self.initialized = true;
                self.total_ticks = i64::from(fields[1]);
                self.direction = if fields[2] == 1 {
                    ProgressDirection::Backward
                } else {
                    ProgressDirection::Forward
                };
                self.completed_ticks = if self.direction == ProgressDirection::Backward {
                    self.total_ticks
                } else {
                    0
                };
                self.action_data_enabled = false;
                self.action_data_ticks = 0;
                self.percent()
            }
            1 if self.initialized => {
                self.action_data_ticks = i64::from(fields[1].max(0));
                self.action_data_enabled = fields[2] == 1;
                None
            }
            2 if self.initialized && fields[1] >= 0 => {
                self.move_ticks(i64::from(fields[1]));
                self.percent()
            }
            3 if self.initialized && fields[1] >= 0 => {
                let added = i64::from(fields[1]);
                self.total_ticks = self.total_ticks.saturating_add(added);
                if self.direction == ProgressDirection::Backward {
                    self.completed_ticks = self.completed_ticks.saturating_add(added);
                }
                self.percent()
            }
            _ => None,
        }
    }

    pub fn on_action_data(&mut self) -> Option<u8> {
        if !self.initialized || !self.action_data_enabled {
            return None;
        }
        self.move_ticks(self.action_data_ticks);
        self.percent()
    }

    pub fn is_rollback(&self) -> bool {
        self.initialized && self.direction == ProgressDirection::Backward
    }

    fn move_ticks(&mut self, ticks: i64) {
        self.completed_ticks = match self.direction {
            ProgressDirection::Forward => self.completed_ticks.saturating_add(ticks),
            ProgressDirection::Backward => self.completed_ticks.saturating_sub(ticks),
        }
        .clamp(0, self.total_ticks);
    }

    fn percent(&self) -> Option<u8> {
        if !self.initialized || self.total_ticks <= 0 {
            return None;
        }
        Some(((self.completed_ticks.saturating_mul(100) / self.total_ticks).clamp(0, 100)) as u8)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsiEvent {
    Progress { percent: u8, rollback: bool },
    Action(String),
    Warning(String),
    Error(String),
    FilesInUse(Vec<String>),
    Initialized,
    Terminated,
}

pub trait InstallBackend {
    fn install(&self, package: &Path, log_path: &Path) -> u32;
}

pub struct MsiControl {
    cancel: AtomicBool,
    connected: AtomicBool,
    files_response: Mutex<Option<FilesInUseResponse>>,
    files_response_ready: Condvar,
}

impl Default for MsiControl {
    fn default() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            connected: AtomicBool::new(true),
            files_response: Mutex::new(None),
            files_response_ready: Condvar::new(),
        }
    }
}

impl MsiControl {
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        self.files_response_ready.notify_all();
    }

    pub fn disconnect(&self) {
        self.connected.store(false, Ordering::Release);
        self.request_cancel();
    }

    pub fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub fn respond_files_in_use(&self, response: FilesInUseResponse) {
        if let Ok(mut slot) = self.files_response.lock() {
            *slot = Some(response);
            self.files_response_ready.notify_all();
        }
    }

    fn wait_for_files_response(&self) -> FilesInUseResponse {
        let Ok(slot) = self.files_response.lock() else {
            return FilesInUseResponse::Cancel;
        };
        let Ok((mut slot, _)) = self.files_response_ready.wait_timeout_while(
            slot,
            Duration::from_secs(120),
            |response| {
                response.is_none()
                    && !self.cancel.load(Ordering::Acquire)
                    && self.connected.load(Ordering::Acquire)
            },
        ) else {
            return FilesInUseResponse::Cancel;
        };
        slot.take().unwrap_or(FilesInUseResponse::Cancel)
    }
}

#[cfg(windows)]
pub struct WindowsInstallerBackend {
    control: Arc<MsiControl>,
    events: SyncSender<MsiEvent>,
}

#[cfg(windows)]
impl WindowsInstallerBackend {
    pub fn new(control: Arc<MsiControl>, events: SyncSender<MsiEvent>) -> Self {
        Self { control, events }
    }
}

#[cfg(windows)]
impl InstallBackend for WindowsInstallerBackend {
    fn install(&self, package: &Path, log_path: &Path) -> u32 {
        use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::{ERROR_SUCCESS, HWND},
            System::ApplicationInstallationAndServicing::{
                MsiEnableLogW, MsiInstallProductW, MsiSetExternalUIRecord, MsiSetInternalUI,
                INSTALLLOGATTRIBUTES_FLUSHEACHLINE, INSTALLLOGMODE_ACTIONDATA,
                INSTALLLOGMODE_ACTIONSTART, INSTALLLOGMODE_ERROR, INSTALLLOGMODE_FATALEXIT,
                INSTALLLOGMODE_FILESINUSE, INSTALLLOGMODE_INITIALIZE, INSTALLLOGMODE_PROGRESS,
                INSTALLLOGMODE_RMFILESINUSE, INSTALLLOGMODE_TERMINATE, INSTALLLOGMODE_WARNING,
                INSTALLUILEVEL_NONE,
            },
        };

        fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
            value.encode_wide().chain([0]).collect()
        }

        struct CallbackContext {
            control: Arc<MsiControl>,
            events: SyncSender<MsiEvent>,
            progress: Mutex<ProgressTracker>,
        }

        fn send_droppable(sender: &SyncSender<MsiEvent>, event: MsiEvent) {
            match sender.try_send(event) {
                Ok(()) | Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {}
            }
        }

        fn send_required(sender: &SyncSender<MsiEvent>, event: MsiEvent) -> bool {
            sender.try_send(event).is_ok()
        }

        unsafe extern "system" fn callback(
            context: *mut c_void,
            message_type: u32,
            record: u32,
        ) -> i32 {
            use windows_sys::Win32::{
                System::ApplicationInstallationAndServicing::{
                    MsiRecordGetFieldCount, MsiRecordGetInteger, INSTALLMESSAGE_ACTIONDATA,
                    INSTALLMESSAGE_ACTIONSTART, INSTALLMESSAGE_ERROR, INSTALLMESSAGE_FATALEXIT,
                    INSTALLMESSAGE_FILESINUSE, INSTALLMESSAGE_INITIALIZE, INSTALLMESSAGE_PROGRESS,
                    INSTALLMESSAGE_RMFILESINUSE, INSTALLMESSAGE_TERMINATE, INSTALLMESSAGE_TYPEMASK,
                    INSTALLMESSAGE_WARNING,
                },
                UI::WindowsAndMessaging::{IDCANCEL, IDIGNORE, IDOK, IDRETRY},
            };

            unsafe fn string_field(record: u32, field: u32, maximum: usize) -> String {
                use std::ptr;
                use windows_sys::Win32::{
                    Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS},
                    System::ApplicationInstallationAndServicing::MsiRecordGetStringW,
                };
                let mut length = 0_u32;
                // SAFETY: null buffer probes the record field length.
                let probe =
                    unsafe { MsiRecordGetStringW(record, field, ptr::null_mut(), &mut length) };
                if probe != ERROR_MORE_DATA && probe != ERROR_SUCCESS {
                    return String::new();
                }
                let length = (length as usize).min(maximum);
                let mut buffer = vec![0_u16; length + 1];
                let mut capacity = buffer.len() as u32;
                // SAFETY: buffer and capacity describe writable UTF-16 storage.
                if unsafe { MsiRecordGetStringW(record, field, buffer.as_mut_ptr(), &mut capacity) }
                    != ERROR_SUCCESS
                {
                    return String::new();
                }
                sanitize_status_text(
                    &String::from_utf16_lossy(&buffer[..capacity as usize]),
                    maximum,
                )
            }

            if context.is_null() {
                return IDCANCEL;
            }
            // SAFETY: context points to CallbackContext for the synchronous install call.
            let context = unsafe { &*(context.cast::<CallbackContext>()) };
            if context.control.cancel_requested() {
                return IDCANCEL;
            }

            let kind = (message_type as i32) & INSTALLMESSAGE_TYPEMASK;
            match kind {
                INSTALLMESSAGE_PROGRESS => {
                    let fields = [
                        // SAFETY: progress records have four integer fields by contract.
                        unsafe { MsiRecordGetInteger(record, 1) },
                        unsafe { MsiRecordGetInteger(record, 2) },
                        unsafe { MsiRecordGetInteger(record, 3) },
                        unsafe { MsiRecordGetInteger(record, 4) },
                    ];
                    if let Ok(mut tracker) = context.progress.lock() {
                        if let Some(percent) = tracker.on_progress_record(fields) {
                            let rollback = tracker.is_rollback();
                            send_droppable(
                                &context.events,
                                MsiEvent::Progress { percent, rollback },
                            );
                        }
                    }
                    IDOK
                }
                INSTALLMESSAGE_ACTIONDATA => {
                    if let Ok(mut tracker) = context.progress.lock() {
                        if let Some(percent) = tracker.on_action_data() {
                            let rollback = tracker.is_rollback();
                            send_droppable(
                                &context.events,
                                MsiEvent::Progress { percent, rollback },
                            );
                        }
                    }
                    IDOK
                }
                INSTALLMESSAGE_ACTIONSTART => {
                    // SAFETY: action description is field 2 of ACTIONSTART.
                    let action = unsafe { string_field(record, 2, 256) };
                    if !action.is_empty() {
                        send_droppable(&context.events, MsiEvent::Action(action));
                    }
                    IDOK
                }
                INSTALLMESSAGE_FILESINUSE | INSTALLMESSAGE_RMFILESINUSE => {
                    // SAFETY: the record belongs to this callback and is valid here.
                    let count = unsafe { MsiRecordGetFieldCount(record) }.min(32);
                    let mut entries = Vec::new();
                    for field in 1..=count {
                        // SAFETY: fields are bounded to the record count.
                        let value = unsafe { string_field(record, field, 128) };
                        if !value.is_empty() {
                            entries.push(value);
                        }
                    }
                    entries.truncate(16);
                    if !send_required(&context.events, MsiEvent::FilesInUse(entries)) {
                        return IDCANCEL;
                    }
                    match context.control.wait_for_files_response() {
                        FilesInUseResponse::Retry => IDRETRY,
                        FilesInUseResponse::Continue => IDIGNORE,
                        FilesInUseResponse::Cancel => IDCANCEL,
                    }
                }
                INSTALLMESSAGE_WARNING => {
                    // SAFETY: field 1 is bounded and copied during the callback.
                    let text = unsafe { string_field(record, 1, 512) };
                    if send_required(&context.events, MsiEvent::Warning(text)) {
                        IDOK
                    } else {
                        IDCANCEL
                    }
                }
                INSTALLMESSAGE_ERROR | INSTALLMESSAGE_FATALEXIT => {
                    // SAFETY: field 1 is bounded and copied during the callback.
                    let text = unsafe { string_field(record, 1, 512) };
                    if send_required(&context.events, MsiEvent::Error(text)) {
                        IDOK
                    } else {
                        IDCANCEL
                    }
                }
                INSTALLMESSAGE_INITIALIZE => {
                    let _ = send_required(&context.events, MsiEvent::Initialized);
                    IDOK
                }
                INSTALLMESSAGE_TERMINATE => {
                    let _ = send_required(&context.events, MsiEvent::Terminated);
                    IDOK
                }
                _ => IDOK,
            }
        }

        let package = wide(package.as_os_str());
        let log_path = wide(log_path.as_os_str());
        let properties: Vec<u16> = "REBOOT=ReallySuppress".encode_utf16().chain([0]).collect();
        let context = CallbackContext {
            control: Arc::clone(&self.control),
            events: self.events.clone(),
            progress: Mutex::new(ProgressTracker::default()),
        };

        let filter = (INSTALLLOGMODE_FATALEXIT
            | INSTALLLOGMODE_ERROR
            | INSTALLLOGMODE_WARNING
            | INSTALLLOGMODE_FILESINUSE
            | INSTALLLOGMODE_ACTIONSTART
            | INSTALLLOGMODE_ACTIONDATA
            | INSTALLLOGMODE_PROGRESS
            | INSTALLLOGMODE_INITIALIZE
            | INSTALLLOGMODE_TERMINATE
            | INSTALLLOGMODE_RMFILESINUSE) as u32;

        // SAFETY: all pointers reference live NUL-terminated buffers for these calls. The
        // callback context remains live until MsiInstallProductW returns.
        unsafe {
            let log_code = MsiEnableLogW(
                filter,
                log_path.as_ptr(),
                INSTALLLOGATTRIBUTES_FLUSHEACHLINE as u32,
            );
            if log_code != ERROR_SUCCESS {
                return log_code;
            }
            let mut owner: HWND = ptr::null_mut();
            let previous_ui = MsiSetInternalUI(INSTALLUILEVEL_NONE, &mut owner);
            let callback_code = MsiSetExternalUIRecord(
                Some(callback),
                filter,
                (&context as *const CallbackContext).cast(),
                None,
            );
            if callback_code != ERROR_SUCCESS {
                MsiSetInternalUI(previous_ui, &mut owner);
                return callback_code;
            }
            let result = MsiInstallProductW(package.as_ptr(), properties.as_ptr());
            MsiSetExternalUIRecord(None, 0, ptr::null(), None);
            MsiSetInternalUI(previous_ui, &mut owner);
            result
        }
    }
}
