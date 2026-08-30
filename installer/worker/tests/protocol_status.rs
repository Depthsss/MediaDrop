use mediadrop_installer_worker::{
    protocol::{read_frame, write_frame, BrokerCommand, BrokerMessage, ProtocolError},
    status::{read_status, sanitize_status_text, InstallerState, StatusSnapshot, StatusStore},
};
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn test_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "MediaDrop Worker Test {label} 你好-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create test directory");
    path
}

#[test]
fn protocol_round_trip_and_version_validation() {
    let message = BrokerMessage::new(7, BrokerCommand::Cancel);
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &message).expect("encode frame");
    let decoded: BrokerMessage = read_frame(&mut Cursor::new(bytes)).expect("decode frame");

    assert_eq!(decoded.sequence, 7);
    assert_eq!(decoded.command, BrokerCommand::Cancel);
    decoded.validate().expect("current protocol is accepted");

    let stale = BrokerMessage {
        protocol: 0,
        ..decoded
    };
    assert!(matches!(
        stale.validate(),
        Err(ProtocolError::UnsupportedVersion { received: 0 })
    ));
}

#[test]
fn component_install_command_round_trips_without_expanding_the_protocol_surface() {
    let command = BrokerCommand::StartComponentInstall {
        session_dir: r"C:\Users\test\AppData\Local\MediaDrop\ComponentSessions\session".into(),
    };
    let message = BrokerMessage::new(11, command.clone());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &message).expect("encode component frame");
    let decoded: BrokerMessage =
        read_frame(&mut Cursor::new(bytes)).expect("decode component frame");

    assert_eq!(decoded.command, command);
    decoded
        .validate()
        .expect("current component protocol is accepted");
}

#[test]
fn protocol_rejects_oversized_and_truncated_frames() {
    let oversized_length = (64 * 1024 + 1_u32).to_le_bytes();
    let error = read_frame::<_, BrokerMessage>(&mut Cursor::new(oversized_length))
        .expect_err("oversized frame must be rejected before allocation");
    assert!(matches!(error, ProtocolError::FrameTooLarge { .. }));

    let mut truncated = Vec::from(8_u32.to_le_bytes());
    truncated.extend_from_slice(b"{}");
    assert!(matches!(
        read_frame::<_, BrokerMessage>(&mut Cursor::new(truncated)),
        Err(ProtocolError::Io(_))
    ));
}

#[test]
fn status_store_replaces_utf16_ini_atomically_and_increments_sequence() {
    let root = test_dir("status");
    let path = root.join("status.ini");
    let mut store = StatusStore::new(path.clone());

    let first = store
        .publish(StatusSnapshot::for_state(InstallerState::StartingBroker))
        .expect("first status");
    let second = store
        .publish(StatusSnapshot {
            state: InstallerState::Installing,
            progress: 42,
            phase: "Windows Installer".into(),
            action: "Dosyalar kopyalanıyor".into(),
            ..StatusSnapshot::default()
        })
        .expect("second status");

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert_eq!(read_status(&path).expect("read status"), second);
    let raw = fs::read(&path).expect("read bytes");
    assert_eq!(&raw[..2], &[0xff, 0xfe], "status.ini is UTF-16LE");
    assert_eq!(
        fs::read_dir(&root).expect("list session").count(),
        1,
        "atomic temp file was replaced"
    );

    fs::remove_dir_all(root).expect("remove test directory");
}

#[test]
fn status_text_is_bounded_and_strips_controls_and_ini_delimiters() {
    let cleaned = sanitize_status_text("  hello\0\r\n[bad]=value\t😀  ", 12);
    assert_eq!(cleaned, "hello bad va");
    assert!(cleaned.chars().count() <= 12);
    assert!(!cleaned.chars().any(char::is_control));
    assert!(!cleaned.contains(['[', ']', '=']));
}

#[test]
fn only_final_states_are_terminal() {
    for state in [
        InstallerState::Succeeded,
        InstallerState::Failed,
        InstallerState::ElevationCancelled,
    ] {
        assert!(state.is_terminal(), "{state:?}");
    }
    assert!(!InstallerState::RollingBack.is_terminal());
    assert!(!InstallerState::CancelPending.is_terminal());
}

#[test]
fn status_replacement_survives_concurrent_polling() {
    let root = test_dir("concurrent-status");
    let path = root.join("status.ini");
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        let mut store = StatusStore::new(writer_path);
        for progress in 0..=100 {
            store
                .publish(StatusSnapshot {
                    progress,
                    ..StatusSnapshot::for_state(InstallerState::Installing)
                })
                .expect("atomic status publish while polled");
        }
    });
    while !writer.is_finished() {
        let _ = read_status(&path);
    }
    writer.join().expect("writer thread");
    assert_eq!(read_status(&path).unwrap().progress, 100);
    fs::remove_dir_all(root).expect("remove test directory");
}
