use crate::{
    component::{install_component, ComponentInstallError},
    msi::{
        map_result_code, InstallBackend, InstallOutcome, MsiControl, MsiEvent,
        WindowsInstallerBackend,
    },
    operation::WorkerOperation,
    payload::ExpectedMsiIdentity,
    protocol::{
        read_frame, write_frame, BrokerCommand, BrokerMessage, InstallResult, ResultKind,
        WorkerEvent, WorkerHello, WorkerMessage, WorkerStatus,
    },
    security::{validate_session_id, SessionSecret},
    status::InstallerState,
    windows,
};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct ElevatedArgs {
    pub pipe_name: String,
    pub session_id: String,
    pub secret: SessionSecret,
    pub broker_pid: u32,
    pub interactive_user_sid: String,
    pub operation: WorkerOperation,
}

#[derive(Clone)]
struct WorkerOutput {
    sender: mpsc::SyncSender<WorkerEvent>,
}

impl WorkerOutput {
    fn new(sender: mpsc::SyncSender<WorkerEvent>) -> Self {
        Self { sender }
    }

    fn send(&self, event: WorkerEvent) -> io::Result<()> {
        self.sender
            .send(event)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "worker pipe closed"))
    }

    fn status(
        &self,
        state: InstallerState,
        progress: u8,
        phase: impl Into<String>,
        action: impl Into<String>,
    ) -> io::Result<()> {
        self.send(WorkerEvent::Status(WorkerStatus {
            state,
            progress,
            phase: phase.into(),
            action: action.into(),
            log_path: String::new(),
        }))
    }

    fn status_with_log(
        &self,
        state: InstallerState,
        progress: u8,
        phase: impl Into<String>,
        log_path: &Path,
    ) -> io::Result<()> {
        self.send(WorkerEvent::Status(WorkerStatus {
            state,
            progress,
            phase: phase.into(),
            action: String::new(),
            log_path: log_path.to_string_lossy().into_owned(),
        }))
    }

    fn complete(&self, result: InstallResult) -> io::Result<()> {
        self.send(WorkerEvent::Complete(result))
    }
}

pub fn run(args: ElevatedArgs) -> u32 {
    match run_inner(args) {
        Ok(code) => code,
        Err(error) => error.raw_os_error().unwrap_or(1) as u32,
    }
}

fn run_inner(args: ElevatedArgs) -> io::Result<u32> {
    validate_args(&args)?;
    #[cfg(not(feature = "test-engine"))]
    if !windows::process_is_elevated(std::process::id())? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "elevated worker token is not elevated",
        ));
    }

    let executable = windows::process_image_path(std::process::id())?;
    let broker = windows::peer_identity(args.broker_pid)?;
    if broker.process_id != args.broker_pid
        || broker.windows_session_id != windows::process_session_id(std::process::id())?
        || !same_path(&broker.executable_path, &executable)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "broker identity rejected",
        ));
    }
    #[cfg(not(feature = "test-engine"))]
    if broker.elevated {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "broker must run at normal integrity",
        ));
    }

    let mut pipe = windows::connect_pipe(&args.pipe_name, Duration::from_secs(15))?;
    if windows::named_pipe_server_pid(&pipe)? != args.broker_pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "pipe server process mismatch",
        ));
    }
    #[cfg(feature = "test-engine")]
    let hello_secret = if std::env::var("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO").as_deref()
        == Ok("handshake_invalid")
    {
        SessionSecret::from_bytes([0x55; 32]).as_hex().to_owned()
    } else {
        args.secret.as_hex().to_owned()
    };
    #[cfg(not(feature = "test-engine"))]
    let hello_secret = args.secret.as_hex().to_owned();
    write_frame(
        &mut pipe,
        &WorkerMessage::new(
            1,
            WorkerEvent::Hello(WorkerHello {
                session_id: args.session_id.clone(),
                secret: hello_secret,
                worker_pid: std::process::id(),
            }),
        ),
    )
    .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()))?;

    let initial = read_frame::<_, BrokerMessage>(&mut pipe)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    initial
        .validate()
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "protocol rejected"))?;
    let component_session = match initial.command {
        BrokerCommand::Abort | BrokerCommand::Cancel => {
            write_frame(
                &mut pipe,
                &WorkerMessage::new(
                    2,
                    WorkerEvent::Complete(InstallResult {
                        result_kind: ResultKind::AbortedBeforeInstall,
                        win32_code: 0,
                        msi_code: 0,
                        reboot_required: false,
                    }),
                ),
            )
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()))?;
            return Ok(1602);
        }
        BrokerCommand::StartInstall if args.operation == WorkerOperation::Installer => None,
        BrokerCommand::StartComponentInstall { session_dir }
            if args.operation == WorkerOperation::Component =>
        {
            Some(PathBuf::from(session_dir))
        }
        BrokerCommand::StartInstall
        | BrokerCommand::StartComponentInstall { .. }
        | BrokerCommand::FilesInUse { .. } => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid initial worker command",
            ));
        }
    };

    let control = Arc::new(MsiControl::default());
    let (output, io_thread) = spawn_worker_io(pipe, initial.sequence, Arc::clone(&control));

    #[cfg(feature = "test-engine")]
    if component_session.is_some() {
        if let Ok(scenario) = std::env::var("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO") {
            return finish_operation(
                run_test_engine(&scenario, &output, &control),
                &output,
                io_thread,
            );
        }
    }

    if let Some(session_dir) = component_session {
        let _component_mutex = match windows::create_component_mutex() {
            Ok(handle) => handle,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                output.complete(InstallResult {
                    result_kind: ResultKind::InstallerBusy,
                    win32_code: 0,
                    msi_code: 1618,
                    reboot_required: false,
                })?;
                let _ = io_thread.join();
                return Ok(1618);
            }
            Err(error) => return Err(error),
        };
        return finish_operation(
            run_production_component(&args, &session_dir, &output, &control),
            &output,
            io_thread,
        );
    }

    let _machine_mutex = match windows::create_machine_mutex() {
        Ok(handle) => handle,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            output.complete(InstallResult {
                result_kind: ResultKind::InstallerBusy,
                win32_code: 0,
                msi_code: 1618,
                reboot_required: false,
            })?;
            let _ = io_thread.join();
            return Ok(1618);
        }
        Err(error) => return Err(error),
    };

    #[cfg(feature = "test-engine")]
    if let Ok(scenario) = std::env::var("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO") {
        if scenario == "worker_crash" {
            return Err(io::Error::other("test worker crash"));
        }
        return finish_operation(
            run_test_engine(&scenario, &output, &control),
            &output,
            io_thread,
        );
    }

    finish_operation(
        run_production_install(&args, &executable, &output, &control),
        &output,
        io_thread,
    )
}

fn run_production_component(
    args: &ElevatedArgs,
    session_dir: &Path,
    output: &WorkerOutput,
    control: &Arc<MsiControl>,
) -> io::Result<u32> {
    if control.cancel_requested() {
        return Ok(1602);
    }
    output.status(
        InstallerState::VerifyingPayload,
        35,
        "Araç güncellemesi doğrulanıyor",
        "",
    )?;
    let store_root = windows::component_store_path(&args.interactive_user_sid)?;
    let result = install_component(
        session_dir,
        &store_root,
        mediadrop_component_update::COMPONENT_PUBLIC_KEY_BASE64,
        env!("CARGO_PKG_VERSION"),
        |path, argv, timeout| {
            !control.cancel_requested() && component_health_check(path, argv, timeout)
        },
        || control.cancel_requested(),
    );
    if control.cancel_requested() {
        return Ok(1602);
    }
    result.map_err(|error| match error {
        ComponentInstallError::Cancelled => io::Error::from_raw_os_error(1602),
        ComponentInstallError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    })?;
    output.status(
        InstallerState::Installing,
        95,
        "Araç güncellemesi etkinleştirildi",
        "yt-dlp hazır",
    )?;
    Ok(0)
}

fn component_health_check(path: &Path, argv: &[String], timeout: Duration) -> bool {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let Some(working_dir) = path.parent() else {
        return false;
    };
    let mut command = Command::new(path);
    command
        .args(argv)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn run_production_install(
    args: &ElevatedArgs,
    executable: &Path,
    output: &WorkerOutput,
    control: &Arc<MsiControl>,
) -> io::Result<u32> {
    let expected = ExpectedMsiIdentity::compiled()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let source = executable
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "worker directory missing"))?
        .join("MediaDrop.msi");
    output.status(
        InstallerState::VerifyingPayload,
        16,
        "Kurulum paketi doğrulanıyor",
        "",
    )?;
    expected
        .verify_file(&source)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    if control.cancel_requested() {
        return Ok(1602);
    }

    output.status(
        InstallerState::PreparingInstaller,
        18,
        "Windows Installer hazırlanıyor",
        "",
    )?;
    let (cache_dir, log_dir) = windows::installer_program_data_paths(
        &expected.metadata().product_version,
        &args.session_id,
        &args.interactive_user_sid,
    )?;
    let cached_msi = cache_dir.join("MediaDrop.msi");
    let log_path = log_dir.join(format!(
        "MediaDrop-{}-{}.log",
        expected.metadata().product_version,
        args.session_id
    ));
    output.status_with_log(
        InstallerState::PreparingInstaller,
        19,
        "Windows Installer günlüğü hazır",
        &log_path,
    )?;
    let install = (|| -> io::Result<u32> {
        copy_new(&source, &cached_msi)?;
        expected
            .verify_file(&cached_msi)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        if control.cancel_requested() {
            return Ok(1602);
        }

        output.status(
            InstallerState::Installing,
            20,
            "MediaDrop kuruluyor",
            "Windows Installer başlatıldı",
        )?;
        let (event_sender, event_receiver) = mpsc::sync_channel(32);
        let event_output = output.clone();
        let event_control = Arc::clone(control);
        let event_thread = thread::spawn(move || {
            let mut ui = MsiUiState::default();
            while let Ok(event) = event_receiver.recv() {
                let _ = forward_msi_event(&event_output, &event_control, &mut ui, event);
            }
        });
        let backend = WindowsInstallerBackend::new(Arc::clone(control), event_sender);
        let result_code = backend.install(&cached_msi, &log_path);
        drop(backend);
        let _ = event_thread.join();
        Ok(result_code)
    })();
    cleanup_cache(&cache_dir, &cached_msi);
    install
}

fn finish_operation(
    operation: io::Result<u32>,
    output: &WorkerOutput,
    io_thread: thread::JoinHandle<()>,
) -> io::Result<u32> {
    let result = match &operation {
        Ok(code) => install_result(*code),
        Err(error) => InstallResult {
            result_kind: ResultKind::Failed,
            win32_code: error
                .raw_os_error()
                .and_then(|code| u32::try_from(code).ok())
                .unwrap_or(1),
            msi_code: 0,
            reboot_required: false,
        },
    };
    let sent = output.complete(result);
    let _ = io_thread.join();
    sent?;
    operation
}

fn spawn_worker_io(
    mut pipe: std::fs::File,
    initial_command_sequence: u64,
    control: Arc<MsiControl>,
) -> (WorkerOutput, thread::JoinHandle<()>) {
    let (event_sender, event_receiver) = mpsc::sync_channel(32);
    let output = WorkerOutput::new(event_sender);
    let thread = thread::spawn(move || {
        let mut outbound_sequence = 1_u64;
        let mut command_sequence = initial_command_sequence;
        loop {
            loop {
                match event_receiver.try_recv() {
                    Ok(event) => {
                        outbound_sequence = outbound_sequence.saturating_add(1);
                        let terminal = matches!(event, WorkerEvent::Complete(_));
                        if write_frame(&mut pipe, &WorkerMessage::new(outbound_sequence, event))
                            .is_err()
                        {
                            control.disconnect();
                            return;
                        }
                        if terminal {
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        control.disconnect();
                        return;
                    }
                }
            }
            match windows::pipe_available(&pipe) {
                Ok(0) => {}
                Ok(_) => {
                    let message = match read_frame::<_, BrokerMessage>(&mut pipe) {
                        Ok(message) => message,
                        Err(_) => {
                            control.disconnect();
                            return;
                        }
                    };
                    if message.validate().is_err() || message.sequence <= command_sequence {
                        control.disconnect();
                        return;
                    }
                    command_sequence = message.sequence;
                    match message.command {
                        BrokerCommand::Cancel | BrokerCommand::Abort => control.request_cancel(),
                        BrokerCommand::FilesInUse { response } => {
                            control.respond_files_in_use(response);
                        }
                        BrokerCommand::StartInstall => {
                            control.disconnect();
                            return;
                        }
                        BrokerCommand::StartComponentInstall { .. } => {
                            control.disconnect();
                            return;
                        }
                    }
                }
                Err(_) => {
                    control.disconnect();
                    return;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });
    (output, thread)
}

fn validate_args(args: &ElevatedArgs) -> io::Result<()> {
    validate_session_id(&args.session_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid session id"))?;
    if args.broker_pid == 0
        || !args.pipe_name.starts_with(args.operation.pipe_prefix())
        || args.pipe_name.len() > 240
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid broker arguments",
        ));
    }
    windows::validate_sid(&args.interactive_user_sid)
}

fn copy_new(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source = fs::OpenOptions::new().read(true).open(source)?;
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut source, &mut destination)?;
    destination.flush()?;
    destination.sync_all()
}

fn cleanup_cache(cache_dir: &Path, cached_msi: &Path) {
    if cached_msi.parent() == Some(cache_dir) {
        let _ = fs::remove_file(cached_msi);
        let _ = fs::remove_dir(cache_dir);
    }
}

#[derive(Debug)]
struct MsiUiState {
    progress: u8,
    rolling_back: bool,
}

impl Default for MsiUiState {
    fn default() -> Self {
        Self {
            progress: 20,
            rolling_back: false,
        }
    }
}

impl MsiUiState {
    fn update_progress(&mut self, percent: u8, rollback: bool) {
        let mapped = 20 + ((u16::from(percent) * 75 / 100) as u8);
        self.progress = if rollback {
            mapped
        } else {
            self.progress.max(mapped)
        };
        self.rolling_back = rollback;
    }

    fn installer_state(&self, cancel_requested: bool) -> InstallerState {
        if self.rolling_back {
            InstallerState::RollingBack
        } else if cancel_requested {
            InstallerState::CancelPending
        } else {
            InstallerState::Installing
        }
    }
}

fn forward_msi_event(
    output: &WorkerOutput,
    control: &MsiControl,
    ui: &mut MsiUiState,
    event: MsiEvent,
) -> io::Result<()> {
    match event {
        MsiEvent::Progress { percent, rollback } => {
            ui.update_progress(percent, rollback);
            output.status(
                ui.installer_state(control.cancel_requested()),
                ui.progress,
                if rollback {
                    "Değişiklikler geri alınıyor"
                } else {
                    "MediaDrop kuruluyor"
                },
                "",
            )
        }
        MsiEvent::Action(action) => output.status(
            ui.installer_state(control.cancel_requested()),
            ui.progress,
            if ui.rolling_back {
                "Değişiklikler geri alınıyor"
            } else {
                "MediaDrop kuruluyor"
            },
            action,
        ),
        MsiEvent::Warning(warning) => output.status(
            ui.installer_state(control.cancel_requested()),
            ui.progress,
            "MediaDrop kuruluyor",
            warning,
        ),
        MsiEvent::Error(error) => output.status(
            ui.installer_state(control.cancel_requested()),
            ui.progress,
            "Windows Installer ayrıntı kaydetti",
            error,
        ),
        MsiEvent::FilesInUse(entries) => output.status(
            InstallerState::FilesInUse,
            ui.progress,
            "Bazı dosyalar kullanımda",
            entries.join(", "),
        ),
        MsiEvent::Initialized => output.status(
            InstallerState::Installing,
            ui.progress,
            "Windows Installer başlatıldı",
            "",
        ),
        MsiEvent::Terminated => output.status(
            ui.installer_state(control.cancel_requested()),
            if control.cancel_requested() {
                ui.progress
            } else {
                95
            },
            "Kurulum sonucu doğrulanıyor",
            "",
        ),
    }
}

fn install_result(code: u32) -> InstallResult {
    match map_result_code(code) {
        InstallOutcome::Succeeded { reboot } => InstallResult {
            result_kind: ResultKind::Succeeded,
            win32_code: 0,
            msi_code: code,
            reboot_required: reboot,
        },
        InstallOutcome::Cancelled => InstallResult {
            result_kind: ResultKind::MsiCancelled,
            win32_code: 0,
            msi_code: code,
            reboot_required: false,
        },
        InstallOutcome::InstallerBusy => InstallResult {
            result_kind: ResultKind::InstallerBusy,
            win32_code: 0,
            msi_code: code,
            reboot_required: false,
        },
        InstallOutcome::ElevationCancelled | InstallOutcome::Failed { .. } => InstallResult {
            result_kind: ResultKind::Failed,
            win32_code: 0,
            msi_code: code,
            reboot_required: false,
        },
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    fn normalize(path: &Path) -> String {
        fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_lowercase()
    }
    normalize(left) == normalize(right)
}

#[cfg(feature = "test-engine")]
fn run_test_engine(scenario: &str, output: &WorkerOutput, control: &MsiControl) -> io::Result<u32> {
    if scenario == "typed_failure" {
        return Err(io::Error::from_raw_os_error(87));
    }
    if scenario == "failure" {
        output.status(
            InstallerState::Installing,
            47,
            "Test kurulumu",
            "Hata senaryosu",
        )?;
        return Ok(1603);
    }
    output.status(
        InstallerState::VerifyingPayload,
        16,
        "Test paketi doğrulanıyor",
        "",
    )?;
    thread::sleep(Duration::from_millis(30));
    output.status(
        InstallerState::Installing,
        35,
        "Test kurulumu",
        "Dosyalar hazırlanıyor",
    )?;

    if scenario == "cancel" {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !control.cancel_requested() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if !control.cancel_requested() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "test cancel command timed out",
            ));
        }
        output.status(
            InstallerState::CancelPending,
            35,
            "Test kurulumu iptal ediliyor",
            "",
        )?;
        thread::sleep(Duration::from_millis(750));
        output.status(
            InstallerState::RollingBack,
            25,
            "Test değişiklikleri geri alınıyor",
            "",
        )?;
        thread::sleep(Duration::from_millis(1500));
        return Ok(1602);
    }

    for progress in [55, 78, 95] {
        if control.cancel_requested() {
            return Ok(1602);
        }
        output.status(
            InstallerState::Installing,
            progress,
            "Test kurulumu",
            "İlerleme pipe üzerinden alındı",
        )?;
        thread::sleep(Duration::from_millis(30));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::MsiUiState;
    use crate::status::InstallerState;

    #[test]
    fn msi_ui_progress_never_regresses_on_forward_actions_and_tracks_rollback() {
        let mut state = MsiUiState::default();
        state.update_progress(80, false);
        assert_eq!(state.progress, 80);
        assert_eq!(state.installer_state(false), InstallerState::Installing);
        state.update_progress(10, false);
        assert_eq!(state.progress, 80);
        state.update_progress(60, true);
        assert_eq!(state.progress, 65);
        assert_eq!(state.installer_state(true), InstallerState::RollingBack);
    }
}
