use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use mediadrop_component_update::{
    resolve_package_url, select_healthy_component, verify_component_file, verify_signed_manifest,
    ActivationEntry, ActivationState, ComponentError,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const PRODUCTION_KEY_MANIFEST: &[u8] = br#"{"schema":1,"channel":"stable","revision":1788042914,"issuedAt":"2026-08-29T22:35:15Z","buildId":"sha256:652e154bce7170070d0f26415c9a3c35c121f5a7903cb8cde6d31c4577517fb9","minimumCoreVersion":"1.0.1","maximumCoreVersionExclusive":"1.1.0","components":[{"name":"yt-dlp","version":"2026.08.18.122307","target":"windows-x86_64","packageUrlId":"github-release:components-r1788042914/yt-dlp.exe","packageSize":17798916,"packageSha256":"652e154bce7170070d0f26415c9a3c35c121f5a7903cb8cde6d31c4577517fb9","files":[{"path":"yt-dlp.exe","size":17798916,"sha256":"652e154bce7170070d0f26415c9a3c35c121f5a7903cb8cde6d31c4577517fb9"}],"healthCheck":{"argv":["--ignore-config","--no-plugin-dirs","--version"],"timeoutMs":10000}}]}"#;
const PRODUCTION_KEY_SIGNATURE: &str =
    "jHCrDp1iKdVRBOa218k04sK91JpUUDNYDeejZYI8a7o9wQWkBHD3EMaNxdxhp1lB4MoGrZJp1m4yxz5CrAKTDQ==";

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mediadrop-component-contract-{}-{}",
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

fn signed_manifest(
    revision: u64,
    minimum_core: &str,
    maximum_core: &str,
    package: &[u8],
) -> (Vec<u8>, String, String) {
    let package_hash = format!("{:x}", Sha256::digest(package));
    let bytes = serde_json::to_vec(&json!({
        "schema": 1,
        "channel": "stable",
        "revision": revision,
        "issuedAt": "2026-08-30T00:00:00Z",
        "buildId": format!("sha256:{package_hash}"),
        "minimumCoreVersion": minimum_core,
        "maximumCoreVersionExclusive": maximum_core,
        "components": [{
            "name": "yt-dlp",
            "version": "2026.08.30",
            "target": "windows-x86_64",
            "packageUrlId": format!("github-release:components-r{revision}/yt-dlp.exe"),
            "packageSize": package.len(),
            "packageSha256": package_hash,
            "files": [{
                "path": "yt-dlp.exe",
                "size": package.len(),
                "sha256": package_hash
            }],
            "healthCheck": { "argv": ["--ignore-config", "--no-plugin-dirs", "--version"], "timeoutMs": 3000 }
        }]
    }))
    .unwrap();
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let signature = STANDARD.encode(signing_key.sign(&bytes).to_bytes());
    let public_key = STANDARD.encode(signing_key.verifying_key().to_bytes());
    (bytes, signature, public_key)
}

#[test]
fn signed_manifest_accepts_one_new_compatible_ytdlp_revision() {
    let (bytes, signature, public_key) = signed_manifest(10003, "1.0.1", "1.1.0", b"tool");
    let verified = verify_signed_manifest(&bytes, &signature, &public_key, "1.0.1", 10002)
        .expect("valid component manifest");

    assert_eq!(verified.manifest.revision, 10003);
    assert_eq!(verified.component().version, "2026.08.30");
    assert_eq!(
        resolve_package_url(verified.component(), verified.manifest.revision).unwrap(),
        "https://github.com/Depthsss/MediaDrop-Releases/releases/download/components-r10003/yt-dlp.exe"
    );
}

#[test]
fn local_release_signer_matches_the_embedded_production_public_key() {
    let verified = verify_signed_manifest(
        PRODUCTION_KEY_MANIFEST,
        PRODUCTION_KEY_SIGNATURE,
        mediadrop_component_update::COMPONENT_PUBLIC_KEY_BASE64,
        "1.0.1",
        0,
    )
    .expect("manifest signed by the local release key");

    assert_eq!(verified.manifest.revision, 1_788_042_914);
}

#[test]
fn signature_tampering_is_rejected_before_manifest_fields_are_trusted() {
    let (mut bytes, signature, public_key) = signed_manifest(10003, "1.0.1", "1.1.0", b"tool");
    bytes[10] ^= 1;

    assert!(matches!(
        verify_signed_manifest(&bytes, &signature, &public_key, "1.0.1", 10002),
        Err(ComponentError::InvalidSignature)
    ));
}

#[test]
fn accepted_or_older_manifest_revision_is_rejected_as_replay() {
    let (bytes, signature, public_key) = signed_manifest(10003, "1.0.1", "1.1.0", b"tool");

    assert!(matches!(
        verify_signed_manifest(&bytes, &signature, &public_key, "1.0.1", 10003),
        Err(ComponentError::RevisionReplay {
            accepted: 10003,
            received: 10003
        })
    ));
}

#[test]
fn manifest_outside_the_running_core_range_is_rejected() {
    let (bytes, signature, public_key) = signed_manifest(10003, "1.0.2", "1.1.0", b"tool");

    assert!(matches!(
        verify_signed_manifest(&bytes, &signature, &public_key, "1.0.1", 0),
        Err(ComponentError::IncompatibleCore)
    ));
}

#[test]
fn package_size_and_hash_are_checked_before_activation() {
    let temp = TestDir::new();
    let package_path = temp.0.join("yt-dlp.exe");
    fs::write(&package_path, b"tampered").unwrap();
    let (bytes, signature, public_key) = signed_manifest(10003, "1.0.1", "1.1.0", b"expected");
    let verified = verify_signed_manifest(&bytes, &signature, &public_key, "1.0.1", 0).unwrap();

    assert!(matches!(
        verify_component_file(verified.component(), &package_path),
        Err(ComponentError::PackageHashMismatch | ComponentError::PackageSizeMismatch { .. })
    ));
}

#[test]
fn unhealthy_active_component_falls_back_to_last_known_good() {
    let active = ActivationEntry {
        revision: 10003,
        version: "2026.08.30".into(),
        manifest_sha256: "a".repeat(64),
        file_sha256: "b".repeat(64),
        relative_path: "yt-dlp/10003/yt-dlp.exe".into(),
    };
    let last_known_good = ActivationEntry {
        revision: 10002,
        version: "2026.08.29".into(),
        manifest_sha256: "c".repeat(64),
        file_sha256: "d".repeat(64),
        relative_path: "yt-dlp/10002/yt-dlp.exe".into(),
    };
    let state = ActivationState {
        schema: 1,
        accepted_revision: 10003,
        active: Some(active),
        last_known_good: Some(last_known_good.clone()),
    };

    let selected = select_healthy_component(&state, |entry| entry.revision == 10002)
        .expect("last-known-good component");
    assert_eq!(selected, &last_known_good);
}
