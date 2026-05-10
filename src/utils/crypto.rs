use std::{
    fs,
    path::{Path, PathBuf},
};

use sha1::{Digest, Sha1};

use anyhow::{Context, Result};

/// Calculate SHA1 hash of a file
pub(crate) fn calculate_sha1(file_path: &Path) -> Result<String> {
    let mut file = fs::File::open(file_path)?;
    let mut hasher = Sha1::new();
    std::io::copy(&mut file, &mut hasher).context("Failed to calculate SHA1 hash")?;

    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

/// Create a `.sha1` file containing the SHA1 hash of the given file.
///
/// The hash file will be named `<original>.<ext>.sha1` or `<original>.sha1` if no extension.
pub(crate) fn generate_sha1_checksum_file(file_path: &Path) -> Result<PathBuf> {
    let hash = calculate_sha1(file_path)?;
    let hash_file_path = file_path.with_extension(format!(
        "{}.sha1",
        file_path.extension().unwrap_or_default().to_string_lossy()
    ));

    fs::write(&hash_file_path, hash).context("Failed to write SHA1 hash to file")?;
    Ok(hash_file_path)
}
