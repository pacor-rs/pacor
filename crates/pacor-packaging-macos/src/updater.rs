//! Updater archive creation (`.app.tar.gz`).

use std::path::{Path, PathBuf};

use anyhow::Result;

use pacor_core::utils::archive::create_gzip_archive;

/// Compress the `.app` bundle into `{output_dir}/{filename_base}.app.tar.gz`.
///
/// The archive is consumed by auto-update mechanisms to perform in-place
/// application updates.
pub fn create_updater_archive(
    output_dir: &Path,
    app_bundle_path: &Path,
    filename_base: &str,
) -> Result<PathBuf> {
    let archive_path = output_dir.join(format!("{filename_base}.app.tar.gz"));
    trace!(
        source_app = %app_bundle_path.display(),
        archive_destination = %archive_path.display(),
        "Creating updater archive"
    );

    create_gzip_archive(app_bundle_path, &archive_path)?;

    info!(archive_path = %archive_path.display(), "Updater archive created");
    Ok(archive_path)
}
