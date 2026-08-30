use mediadrop_installer_worker::{
    msi::MsiControl,
    protocol::WorkerHello,
    security::{
        generate_secret, validate_handshake, validate_session_id, HandshakeExpectation,
        PeerIdentity, SecurityError, SessionSecret,
    },
    status::{read_command_after, CommandSnapshot, InstallerCommand},
};
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

fn test_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "MediaDrop Command 你好-{}-{nonce}-{label}",
        std::process::id(),
    ));
    fs::create_dir_all(&path).expect("create test directory");
    path
}

#[test]
fn secrets_are_256_bit_random_and_round_trip_as_fixed_hex() {
    let first = generate_secret().expect("first secret");
    let second = generate_secret().expect("second secret");
    assert_ne!(first, second);
    assert_eq!(first.as_hex().len(), 64);
    assert_eq!(SessionSecret::parse_hex(first.as_hex()).unwrap(), first);
    assert!(SessionSecret::parse_hex("00").is_err());
    assert!(!format!("{first:?}").contains(first.as_hex()));
}

#[test]
fn session_ids_are_plain_lowercase_guids_not_paths() {
    assert!(validate_session_id("2d9bf7c7-2fb1-4cc8-9cc0-ec3a7d868d1f").is_ok());
    for invalid in [
        "2D9BF7C7-2FB1-4CC8-9CC0-EC3A7D868D1F",
        "../../Windows",
        "2d9bf7c7-2fb1-4cc8-9cc0-ec3a7d868d1",
        "00000000-0000-0000-0000-000000000000\\evil",
    ] {
        assert!(validate_session_id(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn handshake_requires_secret_pipe_pid_elevation_session_and_same_image() {
    let secret = SessionSecret::from_bytes([7; 32]);
    let expected = HandshakeExpectation {
        session_id: "2d9bf7c7-2fb1-4cc8-9cc0-ec3a7d868d1f".into(),
        secret: secret.clone(),
        windows_session_id: 3,
        executable_path: PathBuf::from(r"C:\Users\Test\MediaDrop Worker.exe"),
    };
    let hello = WorkerHello {
        session_id: expected.session_id.clone(),
        secret: secret.as_hex().into(),
        worker_pid: 4321,
    };
    let peer = PeerIdentity {
        process_id: 4321,
        elevated: true,
        windows_session_id: 3,
        executable_path: PathBuf::from(r"c:\users\test\MEDIADROP WORKER.EXE"),
    };
    validate_handshake(&hello, 4321, &peer, &expected).expect("valid handshake");

    let mut wrong = hello.clone();
    wrong.secret = SessionSecret::from_bytes([8; 32]).as_hex().into();
    assert!(matches!(
        validate_handshake(&wrong, 4321, &peer, &expected),
        Err(SecurityError::HandshakeRejected)
    ));
    assert!(matches!(
        validate_handshake(&hello, 9999, &peer, &expected),
        Err(SecurityError::HandshakeRejected)
    ));
    let mut unelevated = peer.clone();
    unelevated.elevated = false;
    assert!(matches!(
        validate_handshake(&hello, 4321, &unelevated, &expected),
        Err(SecurityError::HandshakeRejected)
    ));
}

#[test]
fn stale_command_sequences_are_ignored() {
    let root = test_dir("sequence");
    let path = root.join("command.ini");
    CommandSnapshot::write_atomic(&path, 4, InstallerCommand::Cancel).expect("write command");

    assert_eq!(
        read_command_after(&path, 3).expect("read new command"),
        Some(CommandSnapshot {
            protocol: 1,
            sequence: 4,
            command: InstallerCommand::Cancel,
            response: String::new(),
        })
    );
    assert_eq!(read_command_after(&path, 4).unwrap(), None);
    assert_eq!(read_command_after(&path, 10).unwrap(), None);

    fs::remove_dir_all(root).expect("remove test directory");
}

#[test]
fn pipe_disconnect_requests_safe_msi_cancellation() {
    let control = Arc::new(MsiControl::default());
    assert!(!control.cancel_requested());
    control.disconnect();
    assert!(control.cancel_requested());
}
