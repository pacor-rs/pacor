use std::{fs, path::Path};

use anyhow::Result;

/// Recursively copy a directory including its parent directory.
///
/// This function copies the source directory itself (not just its contents)
/// into the destination path.
#[cfg(target_os = "macos")]
pub(crate) fn copy_directory_with_parent(src: &Path, dst: &Path) -> Result<()> {
    use std::io;

    if !src.exists() || !src.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Source directory does not exist or is not a directory",
        )
        .into());
    }

    let dir_name = src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Invalid source directory path")
    })?;

    let target_dir = dst.join(dir_name);

    fs::create_dir_all(&target_dir)?;

    copy_dir(src, &target_dir)?;

    Ok(())
}

/// Recursively copy the contents of a directory (not the directory itself).
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                warn!("Failed to get file type for {:?}: {}", src_path, e);
                continue;
            }
        };

        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir(&src_path, &dst_path)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                if let Err(e) = copy_symlink(&src_path, &dst_path) {
                    warn!(
                        "Failed to copy symlink {:?} -> {:?}: {}",
                        src_path, dst_path, e
                    );
                }
            }
            #[cfg(not(unix))]
            {
                // Windows doesn't support symlinks in the same way; skip or treat as file
                warn!("Skipping symlink {:?} on non-Unix platform", src_path);
            }
        } else if file_type.is_file() {
            if let Err(e) = fs::copy(&src_path, &dst_path) {
                warn!(
                    "Failed to copy file {:?} -> {:?}: {}",
                    src_path, dst_path, e
                );
            }
        } else {
            warn!("Skipping unsupported file type at {:?}", src_path);
        }
    }

    Ok(())
}

#[cfg(unix)]
pub(crate) fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    let target = fs::read_link(src)?;

    create_symlink(&target, dst)
}

#[cfg(unix)]
pub(crate) fn create_symlink(src: &Path, dst: &Path) -> Result<()> {
    use std::os::unix::fs as unix_fs;

    unix_fs::symlink(src, dst).map_err(Into::into)
}

#[cfg(unix)]
pub(crate) fn write_executable_file(path: &Path, data: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, data)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o770))?;

    Ok(())
}

/// Create a directory if it does not exist
pub(crate) fn ensure_directory_exists(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Clean a directory
pub(crate) fn clean_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}
