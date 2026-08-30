use mediadrop_installer_worker::{
    msi::{map_result_code, CancelLifecycle, InstallOutcome, ProgressTracker},
    payload::{ExpectedMsiIdentity, MsiMetadata, PayloadError},
};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, time::SystemTime};

fn test_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "MediaDrop Payload 你好-{}-{:?}-{name}.msi",
        std::process::id(),
        SystemTime::now()
    ));
    fs::write(&path, bytes).expect("write test payload");
    path
}

fn expected_for(bytes: &[u8]) -> ExpectedMsiIdentity {
    ExpectedMsiIdentity::new(
        bytes.len() as u64,
        format!("{:x}", Sha256::digest(bytes)),
        MsiMetadata {
            product_name: "MediaDrop".into(),
            manufacturer: "mab".into(),
            product_version: "1.0.0".into(),
            upgrade_code: "{8585B38D-5F90-4110-B089-6B89A3FB6339}".into(),
            template: "x64;0".into(),
        },
    )
    .expect("valid expected identity")
}

#[test]
fn payload_rejects_size_and_hash_mismatches_before_metadata() {
    let bytes = b"real MediaDrop package";
    let path = test_file("identity", bytes);

    let wrong_size = ExpectedMsiIdentity::new(
        bytes.len() as u64 + 1,
        format!("{:x}", Sha256::digest(bytes)),
        expected_for(bytes).metadata().clone(),
    )
    .expect("identity");
    assert!(matches!(
        wrong_size.verify_file_bytes(&path),
        Err(PayloadError::SizeMismatch { .. })
    ));

    let wrong_hash = ExpectedMsiIdentity::new(
        bytes.len() as u64,
        "00".repeat(32),
        expected_for(bytes).metadata().clone(),
    )
    .expect("identity");
    assert!(matches!(
        wrong_hash.verify_file_bytes(&path),
        Err(PayloadError::HashMismatch)
    ));

    fs::remove_file(path).expect("remove payload");
}

#[test]
fn arbitrary_msi_and_wrong_product_metadata_are_rejected() {
    let bytes = b"arbitrary attacker package";
    let path = test_file("arbitrary", bytes);
    assert!(matches!(
        expected_for(b"real MediaDrop package").verify_file_bytes(&path),
        Err(PayloadError::SizeMismatch { .. } | PayloadError::HashMismatch)
    ));

    let expected = expected_for(bytes);
    let mut actual = expected.metadata().clone();
    actual.product_name = "Not MediaDrop".into();
    assert!(matches!(
        expected.validate_metadata(&actual),
        Err(PayloadError::MetadataMismatch {
            field: "ProductName",
            ..
        })
    ));
    actual = expected.metadata().clone();
    actual.upgrade_code = "{00000000-0000-0000-0000-000000000000}".into();
    assert!(matches!(
        expected.validate_metadata(&actual),
        Err(PayloadError::MetadataMismatch {
            field: "UpgradeCode",
            ..
        })
    ));

    fs::remove_file(path).expect("remove payload");
}

#[test]
fn release_msi_matches_compiled_product_contract_when_supplied() {
    let Ok(path) = std::env::var("MEDIADROP_TEST_MSI_PATH") else {
        return;
    };
    let size = std::env::var("MEDIADROP_TEST_MSI_SIZE")
        .expect("MSI size accompanies test path")
        .parse()
        .expect("numeric MSI size");
    let hash = std::env::var("MEDIADROP_TEST_MSI_SHA256").expect("MSI hash accompanies test path");
    let expected = ExpectedMsiIdentity::new(
        size,
        hash,
        MsiMetadata {
            product_name: "MediaDrop".into(),
            manufacturer: "mab".into(),
            product_version: "1.0.0".into(),
            upgrade_code: "{8585B38D-5F90-4110-B089-6B89A3FB6339}".into(),
            template: "x64;0".into(),
        },
    )
    .expect("release identity");
    expected
        .verify_file(&PathBuf::from(path))
        .expect("release MSI identity and metadata");
}

#[test]
fn progress_parser_uses_ticks_in_forward_and_rollback_directions() {
    let mut forward = ProgressTracker::default();
    assert_eq!(forward.on_progress_record([0, 100, 0, 0]), Some(0));
    assert_eq!(forward.on_progress_record([2, 25, 0, 0]), Some(25));
    assert_eq!(forward.on_progress_record([1, 5, 1, 0]), None);
    assert_eq!(forward.on_action_data(), Some(30));
    assert_eq!(forward.on_progress_record([3, 100, 0, 0]), Some(15));

    let mut rollback = ProgressTracker::default();
    assert_eq!(rollback.on_progress_record([0, 100, 1, 1]), Some(100));
    assert!(rollback.is_rollback());
    assert_eq!(rollback.on_progress_record([2, 30, 0, 0]), Some(70));
    assert_eq!(rollback.on_progress_record([3, 20, 0, 0]), Some(75));
}

#[test]
fn progress_messages_before_reset_and_malformed_records_are_ignored() {
    let mut tracker = ProgressTracker::default();
    assert_eq!(tracker.on_progress_record([2, 20, 0, 0]), None);
    assert_eq!(tracker.on_progress_record([0, -1, 0, 0]), None);
    assert_eq!(tracker.on_progress_record([99, 1, 2, 3]), None);
    assert_eq!(tracker.on_action_data(), None);
}

#[test]
fn installer_result_codes_keep_elevation_and_msi_cancellation_distinct() {
    assert_eq!(
        map_result_code(0),
        InstallOutcome::Succeeded { reboot: false }
    );
    for code in [3010, 1641] {
        assert_eq!(
            map_result_code(code),
            InstallOutcome::Succeeded { reboot: true }
        );
    }
    assert_eq!(map_result_code(1602), InstallOutcome::Cancelled);
    assert_eq!(map_result_code(1618), InstallOutcome::InstallerBusy);
    assert_eq!(map_result_code(1223), InstallOutcome::ElevationCancelled);
    for code in [1603, 87] {
        assert_eq!(map_result_code(code), InstallOutcome::Failed { code });
    }
}

#[test]
fn cancellation_is_not_terminal_until_windows_installer_returns() {
    let mut lifecycle = CancelLifecycle::default();
    assert!(!lifecycle.is_cancel_requested());
    lifecycle.request();
    assert!(lifecycle.is_cancel_requested());
    assert!(!lifecycle.is_terminal());
    lifecycle.mark_rolling_back();
    assert!(lifecycle.is_rolling_back());
    assert!(!lifecycle.is_terminal());
    lifecycle.complete(1602);
    assert!(lifecycle.is_terminal());
    assert_eq!(lifecycle.result_code(), Some(1602));
}
