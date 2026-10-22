use std::{
    fs,
    path::{Path, PathBuf},
};

use sha1::{Digest, Sha1};
use sha2::Sha256;

use anyhow::{Context, Result};

/// Lowercase hex SHA-256 of `bytes`.
///
/// Used to pin a download to the exact bytes it is expected to deliver: a release asset can be
/// replaced by its owner at any time, and TLS says nothing about that.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);

    format!("{:x}", hasher.finalize())
}

pub fn calculate_sha1(file_path: &Path) -> Result<String> {
    let mut file = fs::File::open(file_path)?;
    let mut hasher = Sha1::new();
    std::io::copy(&mut file, &mut hasher).context("Failed to calculate SHA1 hash")?;

    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

pub fn generate_sha1_checksum_file(file_path: &Path) -> Result<PathBuf> {
    let hash = calculate_sha1(file_path)?;
    let hash_file_path = file_path.with_extension(format!(
        "{}.sha1",
        file_path.extension().unwrap_or_default().to_string_lossy()
    ));

    // Follow the `sha1sum` format (`<hash>  <file name>`) so the file can be
    // verified with `sha1sum -c`.
    let file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    fs::write(&hash_file_path, format!("{hash}  {file_name}"))
        .context("Failed to write SHA1 hash to file")?;
    Ok(hash_file_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
    #[test]
    fn test_calculate_sha1_known_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        {
            let mut f = fs::File::create(&file_path).unwrap();
            f.write_all(b"hello world").unwrap();
        }
        let hash = calculate_sha1(&file_path).unwrap();
        assert_eq!(hash, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
    }

    #[test]
    fn test_calculate_sha1_empty_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("empty.txt");
        fs::File::create(&file_path).unwrap();
        let hash = calculate_sha1(&file_path).unwrap();
        assert_eq!(hash, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn test_generate_sha1_checksum_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("app.exe");
        fs::write(&file_path, b"binary content").unwrap();

        let checksum_path = generate_sha1_checksum_file(&file_path).unwrap();
        assert!(checksum_path.exists());
        assert_eq!(checksum_path.file_name().unwrap(), "app.exe.sha1");

        let content = fs::read_to_string(&checksum_path).unwrap();
        assert!(content.contains("app.exe"));
        assert_eq!(content.split("  ").count(), 2, "expected `<hash>  <name>`");
    }

    #[test]
    fn test_calculate_sha1_nonexistent_file() {
        let result = calculate_sha1(Path::new("/nonexistent/file.txt"));
        assert!(result.is_err());
    }
}
