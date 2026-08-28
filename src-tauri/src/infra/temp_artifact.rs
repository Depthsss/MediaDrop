use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug)]
pub(crate) struct TempArtifact {
    path: PathBuf,
    remove_on_drop: bool,
}

impl TempArtifact {
    pub(crate) fn write(
        parent: &Path,
        prefix: &str,
        suffix: &str,
        contents: &[u8],
    ) -> Result<Self, String> {
        validate_name_part(prefix, "prefix")?;
        validate_name_part(suffix, "suffix")?;
        fs::create_dir_all(parent)
            .map_err(|err| format!("Geçici dosya klasörü oluşturulamadı: {err}"))?;

        for _ in 0..8 {
            let path = parent.join(format!("{prefix}{}{suffix}", uuid::Uuid::new_v4()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(err) = file.write_all(contents).and_then(|_| file.flush()) {
                        let _ = fs::remove_file(&path);
                        return Err(format!("Geçici dosya yazılamadı: {err}"));
                    }
                    return Ok(Self {
                        path,
                        remove_on_drop: true,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(format!("Geçici dosya oluşturulamadı: {err}")),
            }
        }

        Err("Benzersiz geçici dosya adı üretilemedi.".to_string())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn keep(mut self) -> PathBuf {
        self.remove_on_drop = false;
        self.path.clone()
    }
}

impl AsRef<Path> for TempArtifact {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Drop for TempArtifact {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn validate_name_part(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || value.contains("..")
    {
        return Err(format!("Geçici dosya {label} değeri güvenli değil."));
    }
    Ok(())
}

pub(crate) fn cleanup_owned_temp_artifacts(
    parent: &Path,
    prefixes: &[&str],
    minimum_age: Duration,
) -> Result<usize, String> {
    if !parent.is_dir() {
        return Ok(0);
    }

    let now = SystemTime::now();
    let mut removed = 0usize;
    let entries =
        fs::read_dir(parent).map_err(|err| format!("Geçici dosya klasörü okunamadı: {err}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !prefixes.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let old_enough = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .map(|modified| now.duration_since(modified).unwrap_or_default() >= minimum_age)
            .unwrap_or(false);
        if old_enough && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::{cleanup_owned_temp_artifacts, TempArtifact};
    use std::fs;
    use std::time::Duration;

    fn test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mediadrop-temp-artifact-test-{label}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn temp_artifact_removes_plaintext_file_on_drop_and_can_be_disarmed() {
        let root = test_root("drop");
        fs::create_dir_all(&root).unwrap();

        let artifact = TempArtifact::write(&root, "mediadrop-cookie-", ".txt", b"secret")
            .expect("temporary file should be written");
        let removed_path = artifact.path().to_path_buf();
        assert!(removed_path.is_file());
        drop(artifact);
        assert!(!removed_path.exists());

        let kept = TempArtifact::write(&root, "mediadrop-cookie-", ".txt", b"keep")
            .unwrap()
            .keep();
        assert!(kept.is_file());

        fs::remove_file(kept).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_cleanup_only_removes_owned_artifacts() {
        let root = test_root("cleanup");
        fs::create_dir_all(&root).unwrap();
        let owned = root.join("mediadrop-instagram-cookies-stale.txt");
        let unrelated = root.join("unrelated.txt");
        fs::write(&owned, b"secret").unwrap();
        fs::write(&unrelated, b"preserve").unwrap();

        let removed =
            cleanup_owned_temp_artifacts(&root, &["mediadrop-instagram-cookies-"], Duration::ZERO)
                .unwrap();
        assert_eq!(removed, 1);
        assert!(!owned.exists());
        assert!(unrelated.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
