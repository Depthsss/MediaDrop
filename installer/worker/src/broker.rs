use crate::{
    operation::WorkerOperation,
    protocol::{
        read_frame, write_frame, BrokerCommand, BrokerMessage, ResultKind, WorkerEvent,
        WorkerMessage,
    },
    security::{generate_secret, validate_handshake, validate_session_id, HandshakeExpectation},
    status::{
        read_command_after, read_ini_section, InstallerCommand, InstallerState, StatusSnapshot,
        StatusStore,
    },
    windows,
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(not(feature = "test-engine"))]
const ELEVATION_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(feature = "test-engine")]
const ELEVATION_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(not(feature = "test-engine"))]
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(feature = "test-engine")]
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct BrokerArgs {
    pub session_dir: PathBuf,
    pub operation: WorkerOperation,
}

#[derive(Debug)]
struct SessionConfig {
    session_id: String,
    parent_pid: u32,
    silent: bool,
    #[cfg(feature = "test-engine")]
    test_scenario: String,
}

impl SessionConfig {
    fn read(session_dir: &Path, operation: WorkerOperation) -> io::Result<Self> {
        validate_session_directory(session_dir, operation)?;
        let values = read_ini_section(&session_dir.join("config.ini"), "session")?;
        if values.get("protocol").map(String::as_str) != Some("1") {
            return Err(invalid_data("unsupported broker config protocol"));
        }
        let session_id = values
            .get("session_id")
            .cloned()
            .ok_or_else(|| invalid_data("session_id missing"))?;
        validate_session_id(&session_id).map_err(|_| invalid_data("invalid session_id"))?;
        if session_dir.file_name().and_then(|name| name.to_str()) != Some(&session_id) {
            return Err(invalid_data("session directory does not match session_id"));
        }
        let parent_pid = values
            .get("parent_pid")
            .ok_or_else(|| invalid_data("parent_pid missing"))?
            .parse::<u32>()
            .map_err(|_| invalid_data("invalid parent_pid"))?;
        if parent_pid == 0 {
            return Err(invalid_data("invalid parent_pid"));
        }
        let silent = match values.get("silent").map(String::as_str) {
            Some("0") | None => false,
            Some("1") => true,
            _ => return Err(invalid_data("invalid silent flag")),
        };
        #[cfg(feature = "test-engine")]
        let test_scenario = {
            let test = read_ini_section(&session_dir.join("config.ini"), "test")?;
            let scenario = test
                .get("scenario")
                .cloned()
                .unwrap_or_else(|| "success".into());
            if !matches!(
                scenario.as_str(),
                "success"
                    | "cancel"
                    | "failure"
                    | "worker_crash"
                    | "handshake_invalid"
                    | "handshake_timeout"
                    | "elevation_cancelled"
                    | "typed_failure"
            ) {
                return Err(invalid_data("invalid test scenario"));
            }
            scenario
        };
        Ok(Self {
            session_id,
            parent_pid,
            silent,
            #[cfg(feature = "test-engine")]
            test_scenario,
        })
    }
}

struct BrokerStatus {
    store: StatusStore,
    current: StatusSnapshot,
}

impl BrokerStatus {
    fn new(path: PathBuf) -> Self {
        let mut current = StatusSnapshot::for_state(InstallerState::StartingBroker);
        current.progress = 10;
        current.phase = "Kurulum hizmeti hazırlanıyor".into();
        current.broker_pid = std::process::id();
        Self {
            store: StatusStore::new(path),
            current,
        }
    }

    fn publish(&mut self) -> io::Result<()> {
        self.current = self.store.publish(self.current.clone())?;
        Ok(())
    }

    fn state(&mut self, state: InstallerState, progress: u8, phase: &str) -> io::Result<()> {
        self.current.state = state;
        self.current.progress = progress;
        self.current.phase = phase.into();
        self.publish()
    }

    fn fail(&mut self, result_kind: &str, win32_code: u32, msi_code: u32) -> io::Result<()> {
        self.current.state = InstallerState::Failed;
        self.current.result_kind = result_kind.into();
        self.current.win32_code = win32_code;
        self.current.msi_code = msi_code;
        self.current.phase = "Kurulum tamamlanamadı".into();
        self.publish()
    }
}

pub fn run(args: BrokerArgs) -> u32 {
    match run_inner(args) {
        Ok(code) => code,
        Err(error) => error.raw_os_error().unwrap_or(1) as u32,
    }
}

fn run_inner(args: BrokerArgs) -> io::Result<u32> {
    let session_dir = canonical_session_directory(&args.session_dir, args.operation)?;
    let config = SessionConfig::read(&session_dir, args.operation)?;
    let status_path = session_dir.join("status.ini");
    let command_path = session_dir.join("command.ini");
    let mut status = BrokerStatus::new(status_path);
    status.publish()?;

    let current_pid = std::process::id();
    #[cfg(not(feature = "test-engine"))]
    if windows::process_is_elevated(current_pid)? {
        status.fail("broker_elevated", 740, 0)?;
        return Ok(740);
    }
    let parent = windows::open_parent_process(config.parent_pid)?;
    if parent.is_signaled() {
        status.fail("parent_exited", 6, 0)?;
        return Ok(6);
    }
    let executable = windows::process_image_path(std::process::id())?;
    if args.operation == WorkerOperation::Installer
        && canonical_path(
            executable.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "worker directory missing")
            })?,
        )? != session_dir
    {
        status.fail("worker_path_mismatch", 5, 0)?;
        return Ok(5);
    }

    let secret = generate_secret().map_err(|error| invalid_data(error.to_string()))?;
    let user_sid = windows::current_logon_sid()?;
    let pipe_name = format!(
        "{}{}.{}",
        args.operation.pipe_prefix(),
        config.session_id,
        current_pid
    );
    let pipe_server = windows::create_restricted_pipe(&pipe_name, &user_sid)?;
    let expected = HandshakeExpectation {
        session_id: config.session_id.clone(),
        secret: secret.clone(),
        windows_session_id: windows::process_session_id(current_pid)?,
        executable_path: executable.clone(),
    };

    status.state(
        InstallerState::AwaitingElevation,
        12,
        "Windows izni bekleniyor",
    )?;
    let elevated_arguments = vec![
        args.operation.elevated_mode().into(),
        "--pipe".into(),
        pipe_name,
        "--session-id".into(),
        config.session_id.clone(),
        "--secret".into(),
        secret.as_hex().into(),
        "--broker-pid".into(),
        current_pid.to_string(),
        "--interactive-user-sid".into(),
        user_sid,
    ];
    let (launch_sender, launch_receiver) = mpsc::sync_channel(1);
    let launch_executable = executable.clone();
    #[cfg(feature = "test-engine")]
    let test_scenario = config.test_scenario.clone();
    thread::spawn(move || {
        #[cfg(not(feature = "test-engine"))]
        let result = windows::launch_elevated(&launch_executable, &elevated_arguments);
        #[cfg(feature = "test-engine")]
        let result = launch_test_worker(&launch_executable, &elevated_arguments, &test_scenario);
        let _ = launch_sender.send(result);
    });

    let mut command_sequence = 0;
    let mut cancel_before_start = false;
    let launch_deadline = Instant::now() + ELEVATION_TIMEOUT;
    let mut elevation_heartbeat = Instant::now();
    let elevated_observer = loop {
        match launch_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(handle)) => break handle,
            Ok(Err(error)) => {
                let code = error.raw_os_error().unwrap_or(1) as u32;
                if code == 1223 {
                    status.current.state = InstallerState::ElevationCancelled;
                    status.current.progress = 12;
                    status.current.result_kind = "elevation_cancelled".into();
                    status.current.win32_code = 1223;
                    status.current.phase = "Windows izni verilmedi".into();
                    status.publish()?;
                    return Ok(1223);
                }
                status.fail("elevation_failed", code, 0)?;
                return Ok(code);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                status.fail("elevation_failed", 1, 0)?;
                return Ok(1);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if parent.is_signaled() {
            return Ok(1602);
        }
        if let Some(command) = read_command_after(&command_path, command_sequence)? {
            command_sequence = command.sequence;
            if command.command == InstallerCommand::Cancel {
                cancel_before_start = true;
                status.state(
                    InstallerState::CancelPending,
                    status.current.progress,
                    "Kurulum iptal ediliyor",
                )?;
            }
        }
        if Instant::now() >= launch_deadline {
            status.fail("elevation_timeout", 1460, 0)?;
            return Ok(1460);
        }
        if elevation_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            status.publish()?;
            elevation_heartbeat = Instant::now();
        }
    };

    status.state(
        InstallerState::Handshaking,
        15,
        "Güvenli bağlantı doğrulanıyor",
    )?;
    let (connection_sender, connection_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = connection_sender.send(pipe_server.connect());
    });
    let connection_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let pipe = loop {
        match connection_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => break result?,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                status.fail("handshake_failed", 109, 0)?;
                return Ok(109);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if parent.is_signaled() {
            return Ok(1602);
        }
        if Instant::now() >= connection_deadline {
            status.fail("handshake_timeout", 1460, 0)?;
            return Ok(1460);
        }
    };

    let pipe_client_pid = windows::named_pipe_client_pid(&pipe)?;
    let (hello_sender, hello_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut pipe = pipe;
        let result = read_frame::<_, WorkerMessage>(&mut pipe)
            .map(|message| (message, pipe))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()));
        let _ = hello_sender.send(result);
    });
    let (hello_message, pipe) = hello_receiver
        .recv_timeout(HANDSHAKE_TIMEOUT)
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker hello timed out"))??;
    hello_message
        .validate()
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "protocol rejected"))?;
    let hello = match hello_message.event {
        WorkerEvent::Hello(hello) => hello,
        _ => {
            status.fail("handshake_rejected", 5, 0)?;
            return Ok(5);
        }
    };
    let peer = windows::peer_identity(pipe_client_pid)?;
    #[cfg(feature = "test-engine")]
    let peer = crate::security::PeerIdentity {
        elevated: true,
        ..peer
    };
    if validate_handshake(&hello, pipe_client_pid, &peer, &expected).is_err() {
        status.fail("handshake_rejected", 5, 0)?;
        return Ok(5);
    }
    status.current.worker_pid = peer.process_id;
    status.publish()?;

    let initial_command = if cancel_before_start || parent.is_signaled() {
        BrokerCommand::Abort
    } else {
        match args.operation {
            WorkerOperation::Installer => BrokerCommand::StartInstall,
            WorkerOperation::Component => BrokerCommand::StartComponentInstall {
                session_dir: session_dir.to_string_lossy().into_owned(),
            },
        }
    };
    let hello_sequence = hello_message.sequence;
    let (outbound_sender, event_receiver) = spawn_broker_io(pipe);
    outbound_sender
        .send(initial_command)
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "worker command channel closed"))?;

    let _elevated_observer = elevated_observer;
    let mut last_worker_sequence = hello_sequence;
    let mut last_heartbeat = Instant::now();
    let mut cancel_sent = cancel_before_start;
    loop {
        match event_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(message)) => {
                message.validate().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "worker protocol rejected")
                })?;
                if message.sequence <= last_worker_sequence {
                    continue;
                }
                last_worker_sequence = message.sequence;
                match message.event {
                    WorkerEvent::Hello(_) => {
                        status.fail("duplicate_handshake", 5, 0)?;
                        return Ok(5);
                    }
                    WorkerEvent::Status(worker) => {
                        status.current.state = worker.state;
                        status.current.progress = worker.progress;
                        status.current.phase = worker.phase;
                        status.current.action = worker.action;
                        if !worker.log_path.is_empty() {
                            status.current.log_path = worker.log_path;
                        }
                        status.publish()?;
                    }
                    WorkerEvent::Complete(result) => {
                        apply_result(&mut status, result)?;
                        return Ok(status.current.msi_code.max(status.current.win32_code));
                    }
                }
            }
            Ok(Err(error)) => {
                status.fail(
                    "worker_disconnected",
                    error.raw_os_error().unwrap_or(109) as u32,
                    0,
                )?;
                return Ok(109);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                status.fail("worker_disconnected", 109, 0)?;
                return Ok(109);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let parent_dead = parent.is_signaled();
        if let Some(command) = read_command_after(&command_path, command_sequence)? {
            command_sequence = command.sequence;
            let worker_command = match command.command {
                InstallerCommand::Cancel => Some(BrokerCommand::Cancel),
                InstallerCommand::RetryFiles => Some(BrokerCommand::FilesInUse {
                    response: crate::protocol::FilesInUseResponse::Retry,
                }),
                InstallerCommand::ContinueFiles => Some(BrokerCommand::FilesInUse {
                    response: crate::protocol::FilesInUseResponse::Continue,
                }),
                InstallerCommand::CancelFiles => Some(BrokerCommand::FilesInUse {
                    response: crate::protocol::FilesInUseResponse::Cancel,
                }),
            };
            if let Some(command) = worker_command {
                outbound_sender.send(command).map_err(|_| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "worker command channel closed")
                })?;
            }
            if command.command == InstallerCommand::Cancel {
                cancel_sent = true;
                status.state(
                    InstallerState::CancelPending,
                    status.current.progress,
                    "Windows Installer güvenli biçimde geri alıyor",
                )?;
            }
        }
        if parent_dead && !cancel_sent {
            cancel_sent = true;
            let _ = outbound_sender.send(BrokerCommand::Cancel);
        }
        if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            status.publish()?;
            last_heartbeat = Instant::now();
        }
        if config.silent && parent_dead && status.current.state.is_terminal() {
            return Ok(status.current.msi_code.max(status.current.win32_code));
        }
    }
}

fn spawn_broker_io(
    mut pipe: std::fs::File,
) -> (
    mpsc::SyncSender<BrokerCommand>,
    mpsc::Receiver<io::Result<WorkerMessage>>,
) {
    let (command_sender, command_receiver) = mpsc::sync_channel(32);
    let (event_sender, event_receiver) = mpsc::sync_channel(32);
    thread::spawn(move || {
        let mut outbound_sequence = 0_u64;
        loop {
            loop {
                match command_receiver.try_recv() {
                    Ok(command) => {
                        outbound_sequence = outbound_sequence.saturating_add(1);
                        if let Err(error) =
                            write_frame(&mut pipe, &BrokerMessage::new(outbound_sequence, command))
                        {
                            let _ = event_sender.send(Err(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                error.to_string(),
                            )));
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            match windows::pipe_available(&pipe) {
                Ok(0) => {}
                Ok(_) => {
                    let event = read_frame::<_, WorkerMessage>(&mut pipe).map_err(|error| {
                        io::Error::new(io::ErrorKind::BrokenPipe, error.to_string())
                    });
                    let terminal = matches!(
                        event,
                        Ok(WorkerMessage {
                            event: WorkerEvent::Complete(_),
                            ..
                        })
                    );
                    if event_sender.send(event).is_err() || terminal {
                        return;
                    }
                }
                Err(error) => {
                    let _ = event_sender.send(Err(error));
                    return;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });
    (command_sender, event_receiver)
}

fn apply_result(
    status: &mut BrokerStatus,
    result: crate::protocol::InstallResult,
) -> io::Result<()> {
    status.current.win32_code = result.win32_code;
    status.current.msi_code = result.msi_code;
    status.current.reboot_required = result.reboot_required;
    status.current.result_kind = match result.result_kind {
        ResultKind::Succeeded => "succeeded",
        ResultKind::MsiCancelled => "cancelled",
        ResultKind::AbortedBeforeInstall => "aborted_before_install",
        ResultKind::InstallerBusy => "installer_busy",
        ResultKind::Failed => "failed",
    }
    .into();
    if result.result_kind == ResultKind::Succeeded {
        status.current.state = InstallerState::Succeeded;
        status.current.progress = 100;
        status.current.phase = if result.reboot_required {
            "Kurulum tamamlandı; Windows yeniden başlatılmalı"
        } else {
            "Kurulum tamamlandı"
        }
        .into();
    } else {
        status.current.state = InstallerState::Failed;
        status.current.phase = match result.result_kind {
            ResultKind::MsiCancelled | ResultKind::AbortedBeforeInstall => "Kurulum iptal edildi",
            ResultKind::InstallerBusy => "Başka bir Windows kurulumu çalışıyor",
            _ => "Kurulum tamamlanamadı",
        }
        .into();
    }
    status.publish()
}

fn canonical_session_directory(path: &Path, operation: WorkerOperation) -> io::Result<PathBuf> {
    validate_session_directory(path, operation)?;
    canonical_path(path)
}

fn validate_session_directory(path: &Path, operation: WorkerOperation) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session path is not absolute",
        ));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session path is not a plain directory",
        ));
    }
    #[cfg(feature = "test-engine")]
    let _ = operation;
    #[cfg(not(feature = "test-engine"))]
    {
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA missing"))?;
        let root_name = match operation {
            WorkerOperation::Installer => "InstallerSessions",
            WorkerOperation::Component => "ComponentSessions",
        };
        let root = canonical_path(&PathBuf::from(local).join("MediaDrop").join(root_name))?;
        let parent = canonical_path(
            path.parent()
                .ok_or_else(|| invalid_data("session parent missing"))?,
        )?;
        if parent != root {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "session directory is outside MediaDrop InstallerSessions",
            ));
        }
    }
    Ok(())
}

fn canonical_path(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path).map(|path| {
        PathBuf::from(
            path.to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned(),
        )
    })
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(feature = "test-engine")]
fn launch_test_worker(
    executable: &Path,
    arguments: &[String],
    scenario: &str,
) -> io::Result<Option<windows::OwnedHandle>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if scenario == "elevation_cancelled" {
        return Err(io::Error::from_raw_os_error(1223));
    }
    if scenario == "handshake_timeout" {
        return Ok(None);
    }
    let mut command = std::process::Command::new(executable);
    command
        .args(arguments)
        .env("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO", scenario)
        .creation_flags(CREATE_NO_WINDOW);
    let _child = command.spawn()?;
    Ok(None)
}
