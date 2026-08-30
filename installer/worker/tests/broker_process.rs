#![cfg(feature = "test-engine")]

use mediadrop_installer_worker::{
    status::{read_status, CommandSnapshot, InstallerCommand, InstallerState},
    windows,
};
use std::{
    fs,
    io::{self, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct RunningSession {
    root: PathBuf,
    session: PathBuf,
    child: Child,
}

impl Drop for RunningSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn real_broker_and_test_worker_cover_success_cancel_failure_and_isolation() {
    let _serial = integration_test_lock();
    for index in 0..3 {
        let mut run = start_session("success", std::process::id(), index);
        let (status, seen) = wait_terminal(&mut run, false);
        assert_eq!(status.state, InstallerState::Succeeded);
        assert_eq!(status.progress, 100);
        assert!(seen.contains(&InstallerState::Installing));
        assert_eq!(wait_child(&mut run.child).code(), Some(0));
        assert_process_stopped(status.worker_pid);
    }

    let mut cancelled = start_session("cancel", std::process::id(), 10);
    let (status, seen) = wait_terminal(&mut cancelled, true);
    assert_eq!(status.state, InstallerState::Failed);
    assert_eq!(status.result_kind, "cancelled");
    assert_eq!(status.msi_code, 1602);
    assert!(seen.contains(&InstallerState::CancelPending));
    assert!(seen.contains(&InstallerState::RollingBack));
    assert_eq!(wait_child(&mut cancelled.child).code(), Some(1602));
    assert_process_stopped(status.worker_pid);

    let mut failed = start_session("failure", std::process::id(), 11);
    let (status, _) = wait_terminal(&mut failed, false);
    assert_eq!(status.state, InstallerState::Failed);
    assert_eq!(status.msi_code, 1603);
    assert_eq!(wait_child(&mut failed.child).code(), Some(1603));
    assert_process_stopped(status.worker_pid);
}

#[test]
fn handshake_and_elevation_failures_are_typed_without_orphans() {
    let _serial = integration_test_lock();
    for (scenario, expected_kind, expected_code) in [
        ("handshake_invalid", "handshake_rejected", 5),
        ("handshake_timeout", "handshake_timeout", 1460),
        ("elevation_cancelled", "elevation_cancelled", 1223),
        ("worker_crash", "worker_disconnected", 109),
        ("typed_failure", "failed", 87),
    ] {
        let mut run = start_session(scenario, std::process::id(), expected_code);
        let (status, _) = wait_terminal(&mut run, false);
        assert_eq!(status.result_kind, expected_kind, "{scenario}");
        assert_eq!(
            wait_child(&mut run.child).code(),
            Some(expected_code as i32)
        );
        if status.worker_pid != 0 {
            assert_process_stopped(status.worker_pid);
        }
    }
}

#[test]
fn component_broker_uses_the_same_authenticated_elevation_boundary() {
    let _serial = integration_test_lock();
    let mut run = start_session_mode("success", std::process::id(), 12, "--component-broker");
    let (status, seen) = wait_terminal(&mut run, false);

    assert_eq!(status.state, InstallerState::Succeeded);
    assert!(seen.contains(&InstallerState::Installing));
    assert_eq!(wait_child(&mut run.child).code(), Some(0));
    assert_process_stopped(status.worker_pid);
}

#[test]
fn parent_death_requests_rollback_and_broker_death_stops_worker() {
    let _serial = integration_test_lock();
    let mut parent = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("spawn disposable parent");
    let mut run = start_session("cancel", parent.id(), 20);
    wait_for_state(&run.session, InstallerState::Installing);
    parent.kill().expect("kill disposable parent");
    let _ = parent.wait();
    let (status, seen) = wait_terminal(&mut run, false);
    assert_eq!(status.msi_code, 1602);
    assert!(seen.contains(&InstallerState::RollingBack));
    let _ = wait_child(&mut run.child);
    assert_process_stopped(status.worker_pid);

    let mut broker_crash = start_session("cancel", std::process::id(), 21);
    let active = wait_for_state(&broker_crash.session, InstallerState::Installing);
    let worker_pid = active.worker_pid;
    broker_crash.child.kill().expect("stop test broker");
    let _ = broker_crash.child.wait();
    assert_process_stopped(worker_pid);
}

fn integration_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn start_session(scenario: &str, parent_pid: u32, salt: u32) -> RunningSession {
    start_session_mode(scenario, parent_pid, salt, "--broker")
}

fn start_session_mode(
    scenario: &str,
    parent_pid: u32,
    salt: u32,
    broker_mode: &str,
) -> RunningSession {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "MediaDrop Çağrı 你好-{}-{nonce}-{salt}",
        std::process::id()
    ));
    let session_id = format!("{salt:08x}-2fb1-4cc8-9cc0-{nonce:012x}");
    let session_id = &session_id[..36];
    let session = root.join(session_id);
    fs::create_dir_all(&session).expect("create session");
    let source = PathBuf::from(env!("CARGO_BIN_EXE_mediadrop-installer-worker"));
    let worker = session.join("mediadrop-installer-worker.exe");
    fs::copy(source, &worker).expect("copy test worker");
    write_utf16(
        &session.join("config.ini"),
        &format!(
            "[session]\r\nprotocol=1\r\nsession_id={session_id}\r\nparent_pid={parent_pid}\r\nsilent=0\r\n[test]\r\nscenario={scenario}\r\n"
        ),
    )
    .expect("write config");
    let child = Command::new(&worker)
        .args([broker_mode, "--session-dir"])
        .arg(&session)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .expect("start broker");
    RunningSession {
        root,
        session,
        child,
    }
}

fn wait_terminal(
    run: &mut RunningSession,
    request_cancel: bool,
) -> (
    mediadrop_installer_worker::status::StatusSnapshot,
    Vec<InstallerState>,
) {
    let deadline = Instant::now() + Duration::from_secs(12);
    let path = run.session.join("status.ini");
    let mut last_sequence = 0;
    let mut seen = Vec::new();
    let mut last_status = None;
    let mut cancel_written = false;
    loop {
        if let Ok(status) = read_status(&path) {
            last_status = Some(status.clone());
            assert!(status.sequence >= last_sequence);
            if status.sequence > last_sequence {
                last_sequence = status.sequence;
                seen.push(status.state);
            }
            if request_cancel && !cancel_written && status.state == InstallerState::Installing {
                CommandSnapshot::write_atomic(
                    &run.session.join("command.ini"),
                    1,
                    InstallerCommand::Cancel,
                )
                .expect("request cancellation");
                cancel_written = true;
            }
            if status.state.is_terminal() {
                return (status, seen);
            }
        }
        if let Some(status) = run.child.try_wait().expect("poll broker") {
            if let Ok(final_status) = read_status(&path) {
                if final_status.state.is_terminal() {
                    seen.push(final_status.state);
                    return (final_status, seen);
                }
                last_status = Some(final_status);
            }
            panic!("broker exited before terminal status: {status:?}; last={last_status:?}");
        }
        assert!(
            Instant::now() < deadline,
            "terminal status timed out; last={last_status:?}; seen={seen:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_state(
    session: &Path,
    expected: InstallerState,
) -> mediadrop_installer_worker::status::StatusSnapshot {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last = None;
    loop {
        if let Ok(status) = read_status(&session.join("status.ini")) {
            if status.state == expected {
                return status;
            }
            last = Some(status);
        }
        assert!(
            Instant::now() < deadline,
            "state {expected:?} timed out; last={last:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_child(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return status;
        }
        assert!(Instant::now() < deadline, "broker did not exit");
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_process_stopped(process_id: u32) {
    if process_id == 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match windows::open_parent_process(process_id) {
            Err(_) => return,
            Ok(handle) if handle.is_signaled() => return,
            Ok(_) => {}
        }
        assert!(
            Instant::now() < deadline,
            "worker process {process_id} remained alive"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn write_utf16(path: &Path, value: &str) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(&[0xff, 0xfe])?;
    for unit in value.encode_utf16() {
        file.write_all(&unit.to_le_bytes())?;
    }
    file.sync_all()
}
