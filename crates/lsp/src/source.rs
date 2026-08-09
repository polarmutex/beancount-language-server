use anyhow::{Context, Result, anyhow};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Command;

const PGP_MESSAGE_HEADER: &str = "-----BEGIN PGP MESSAGE-----";

/// Read Beancount source from disk, decrypting OpenPGP files in memory.
///
/// Binary `.gpg` files are always treated as encrypted. Armored `.asc` files
/// are decrypted only when they start with an OpenPGP message header, so a
/// regular text file with that extension is still handled normally.
pub(crate) fn read(path: &Path) -> Result<String> {
    if is_encrypted(path)? {
        decrypt(path, OsStr::new("gpg"))
    } else {
        fs::read_to_string(path)
            .with_context(|| format!("Failed to read Beancount source: {}", path.display()))
    }
}

fn is_encrypted(path: &Path) -> Result<bool> {
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("gpg") => Ok(true),
        Some("asc") => {
            let file = fs::File::open(path)
                .with_context(|| format!("Failed to inspect armored source: {}", path.display()))?;
            let mut first_line = String::new();
            BufReader::new(file)
                .read_line(&mut first_line)
                .with_context(|| format!("Failed to inspect armored source: {}", path.display()))?;
            Ok(first_line.trim_end() == PGP_MESSAGE_HEADER)
        }
        _ => Ok(false),
    }
}

fn decrypt(path: &Path, gpg_command: &OsStr) -> Result<String> {
    let output = Command::new(gpg_command)
        .args(["--batch", "--no-tty", "--decrypt", "--"])
        .arg(path)
        .output()
        .with_context(|| {
            format!(
                "Failed to execute GPG while reading encrypted Beancount source: {}",
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            anyhow!(
                "GPG failed to decrypt Beancount source {} (status {})",
                path.display(),
                output.status
            )
        } else {
            anyhow!(
                "GPG failed to decrypt Beancount source {}: {}",
                path.display(),
                detail
            )
        });
    }

    String::from_utf8(output.stdout).with_context(|| {
        format!(
            "Decrypted Beancount source is not valid UTF-8: {}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::fs;

    #[test]
    fn reads_plain_beancount_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.beancount");
        fs::write(&path, "2026-01-01 open Assets:Cash\n").unwrap();

        assert_eq!(read(&path).unwrap(), "2026-01-01 open Assets:Cash\n");
    }

    #[test]
    fn recognizes_binary_gpg_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.beancount.gpg");
        fs::write(&path, [0_u8, 159, 146, 150]).unwrap();

        assert!(is_encrypted(&path).unwrap());
    }

    #[test]
    fn recognizes_only_armored_pgp_asc_files() {
        let dir = tempfile::tempdir().unwrap();
        let encrypted = dir.path().join("main.beancount.asc");
        let plain = dir.path().join("notes.asc");
        fs::write(
            &encrypted,
            "-----BEGIN PGP MESSAGE-----\nVersion: test\n\nciphertext\n",
        )
        .unwrap();
        fs::write(&plain, "2026-01-01 open Assets:Cash\n").unwrap();

        assert!(is_encrypted(&encrypted).unwrap());
        assert!(!is_encrypted(&plain).unwrap());
        assert_eq!(read(&plain).unwrap(), "2026-01-01 open Assets:Cash\n");
    }

    #[test]
    fn reports_gpg_execution_failure_without_reading_ciphertext_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.beancount.gpg");
        fs::write(&path, [0_u8, 159, 146, 150]).unwrap();

        let error = decrypt(&path, OsStr::new("definitely-not-a-gpg-command"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Failed to execute GPG"));
        assert!(error.contains("main.beancount.gpg"));
    }
}
