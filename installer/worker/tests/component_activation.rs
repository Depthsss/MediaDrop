use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use mediadrop_installer_worker::component::{install_component, ComponentInstallError};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, time::Duration};

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "mediadrop-component-worker-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_signed_session(session: &std::path::Path, revision: u64, package: &[u8]) -> String {
    fs::create_dir_all(session).unwrap();
    let hash = format!("{:x}", Sha256::digest(package));
    let manifest = serde_json::to_vec(&json!({
        "schema": 1,
        "channel": "stable",
        "revision": revision,
        "issuedAt": "2026-08-30T00:00:00Z",
        "buildId": format!("sha256:{hash}"),
        "minimumCoreVersion": "1.0.1",
        "maximumCoreVersionExclusive": "1.1.0",
        "components": [{
            "name": "yt-dlp",
            "version": "2026.08.30",
            "target": "windows-x86_64",
            "packageUrlId": format!("github-release:components-r{revision}/yt-dlp.exe"),
            "packageSize": package.len(),
            "packageSha256": hash,
            "files": [{"path": "yt-dlp.exe", "size": package.len(), "sha256": hash}],
            "healthCheck": {"argv": ["--ignore-config", "--no-plugin-dirs", "--version"], "timeoutMs": 3000}
        }]
    }))
    .unwrap();
    let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
    let signature = STANDARD.encode(signing_key.sign(&manifest).to_bytes());
    fs::write(session.join("component-manifest.json"), manifest).unwrap();
    fs::write(session.join("component-manifest.sig"), signature).unwrap();
    fs::write(session.join("yt-dlp.exe"), package).unwrap();
    STANDARD.encode(signing_key.verifying_key().to_bytes())
}

#[test]
fn staging_health_failure_keeps_the_previous_activation_byte_for_byte() {
    let temp = TestDir::new("health");
    let session = temp.0.join("session");
    let store = temp.0.join("store");
    fs::create_dir_all(&store).unwrap();
    let original = br#"{"schema":1,"acceptedRevision":10002,"active":null,"lastKnownGood":null}"#;
    fs::write(store.join("activation.json"), original).unwrap();
    let public_key = write_signed_session(&session, 10003, b"candidate");

    let result = install_component(
        &session,
        &store,
        &public_key,
        "1.0.1",
        |_path, _argv, _timeout: Duration| false,
        || false,
    );

    assert!(matches!(
        result,
        Err(ComponentInstallError::HealthCheckFailed)
    ));
    assert_eq!(fs::read(store.join("activation.json")).unwrap(), original);
}

#[test]
fn valid_component_is_verified_copied_and_atomically_activated_with_lkg() {
    let temp = TestDir::new("activate");
    let session = temp.0.join("session");
    let store = temp.0.join("store");
    fs::create_dir_all(store.join("yt-dlp/10002")).unwrap();
    fs::write(store.join("yt-dlp/10002/yt-dlp.exe"), b"old").unwrap();
    let old_hash = format!("{:x}", Sha256::digest(b"old"));
    fs::write(
        store.join("activation.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "acceptedRevision": 10002,
            "active": {
                "revision": 10002,
                "version": "2026.08.29",
                "manifestSha256": "a".repeat(64),
                "fileSha256": old_hash,
                "relativePath": "yt-dlp/10002/yt-dlp.exe"
            },
            "lastKnownGood": null
        }))
        .unwrap(),
    )
    .unwrap();
    let public_key = write_signed_session(&session, 10003, b"candidate");

    let active = install_component(
        &session,
        &store,
        &public_key,
        "1.0.1",
        |path, argv, timeout| {
            path.starts_with(&store)
                && argv == ["--ignore-config", "--no-plugin-dirs", "--version"]
                && timeout == Duration::from_secs(3)
        },
        || false,
    )
    .unwrap();

    assert_eq!(active.revision, 10003);
    assert_eq!(
        fs::read(store.join(&active.relative_path)).unwrap(),
        b"candidate"
    );
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("activation.json")).unwrap()).unwrap();
    assert_eq!(state["acceptedRevision"], 10003);
    assert_eq!(state["active"]["revision"], 10003);
    assert_eq!(state["lastKnownGood"]["revision"], 10002);
}

#[test]
fn cancellation_after_health_check_never_switches_activation() {
    let temp = TestDir::new("cancel");
    let session = temp.0.join("session");
    let store = temp.0.join("store");
    fs::create_dir_all(&store).unwrap();
    let original = br#"{"schema":1,"acceptedRevision":10002,"active":null,"lastKnownGood":null}"#;
    fs::write(store.join("activation.json"), original).unwrap();
    let public_key = write_signed_session(&session, 10003, b"candidate");
    let cancelled = std::cell::Cell::new(false);

    let result = install_component(
        &session,
        &store,
        &public_key,
        "1.0.1",
        |_path, _argv, _timeout| {
            cancelled.set(true);
            true
        },
        || cancelled.get(),
    );

    assert!(matches!(result, Err(ComponentInstallError::Cancelled)));
    assert_eq!(fs::read(store.join("activation.json")).unwrap(), original);
}
