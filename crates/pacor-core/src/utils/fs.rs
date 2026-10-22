//! Filesystem helpers shared by packaging backends.
//!
//! These helpers are compiled on every host: OS-specific primitives (symlinks,
//! unix permissions) are gated with `#[cfg]` *inside* the functions, never at
//! the module level, so a backend can always be type-checked.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};

/// Recursively copy `src` itself (including its own directory name) into `dst`.
pub fn copy_directory_with_parent(src: &Path, dst: &Path) -> Result<()> {
    let dir_name = src
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid source directory path: {}", src.display()))?;

    let target_dir = dst.join(dir_name);
    fs::create_dir_all(&target_dir)?;
    copy_dir(src, &target_dir)
}

/// Recursively copy the *contents* of `src` into `dst`.
///
/// Unlike the previous implementation this propagates copy failures instead of
/// logging and silently skipping entries.
pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    // The destination is created here instead of trusting the caller: `read_dir` returns
    // entries in whatever order the filesystem prefers (APFS handed back the executable
    // before the directories), and copying a file into a directory that does not exist yet
    // fails with ENOENT. `AppDir::populate` is exactly such a caller — it copies the Flutter
    // bundle into `usr/`, which does not exist yet at that point.
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", src_path.display()))?;

        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir(&src_path, &dst_path)?;
        } else if file_type.is_symlink() {
            copy_symlink(&src_path, &dst_path).with_context(|| {
                format!(
                    "failed to copy symlink {} -> {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "failed to copy file {} -> {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Copy a symlink, recreating the link where the host supports it.
#[cfg(unix)]
pub fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    let target = fs::read_link(src)?;
    create_symlink(&target, dst)
}

/// Copy a symlink on hosts without symlink support by copying its target.
#[cfg(not(unix))]
pub fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    let resolved = fs::canonicalize(src)?;
    if resolved.is_dir() {
        fs::create_dir_all(dst)?;
        copy_dir(&resolved, dst)
    } else {
        fs::copy(&resolved, dst).map(|_| ()).map_err(Into::into)
    }
}

/// Create a symbolic link at `dst` pointing to `src`.
#[cfg(unix)]
pub fn create_symlink(src: &Path, dst: &Path) -> Result<()> {
    std::os::unix::fs::symlink(src, dst).map_err(Into::into)
}

/// Create a link on hosts without symlink support by copying the target.
#[cfg(not(unix))]
pub fn create_symlink(src: &Path, dst: &Path) -> Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    if src.is_dir() {
        symlink_dir(src, dst).map_err(Into::into)
    } else {
        symlink_file(src, dst).map_err(Into::into)
    }
}

/// Why a path cannot be used as a location inside an installed package, if it cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackagePathProblem {
    /// The path contains `..`.
    EscapesRoot,
    /// The path names the package root itself rather than something in it.
    IsRoot,
}

impl std::fmt::Display for PackagePathProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EscapesRoot => {
                write!(f, "must not contain `..`: it would leave the package root")
            }
            Self::IsRoot => write!(f, "must name a file or directory inside the package root"),
        }
    }
}

/// Resolve a configured path to the location it occupies inside a package.
///
/// Absolute paths are package-absolute — `/usr/bin/tool` installs to `usr/bin/tool` —
/// which is what the `files` mappings of the Debian backend document. `..` is
/// rejected instead of resolved: the payload is assembled in a staging directory, so
/// resolving it would write wherever the build happens to run, outside the package
/// being assembled and, for a project that came from somewhere else, into the
/// packager's own files.
pub fn package_relative_path(target: &Path) -> Result<PathBuf, PackagePathProblem> {
    let mut resolved = PathBuf::new();
    for component in target.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err(PackagePathProblem::EscapesRoot);
            }
        }
    }

    if resolved.as_os_str().is_empty() {
        return Err(PackagePathProblem::IsRoot);
    }

    Ok(resolved)
}

/// Write a file and, on unix, mark it executable.
pub fn write_executable_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

/// Create a directory (and its parents) if it does not exist.
pub fn ensure_directory_exists(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Total size in bytes of the regular files under `path`.
///
/// Best effort: unreadable entries are skipped rather than failing the call,
/// because this is only used for reporting.
pub fn directory_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![path.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };

        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }

    total
}

/// Recursively remove a directory if it exists.
pub fn clean_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use tempfile::TempDir;

    /// A destination that does not exist yet, with a source whose first entry is a file: the
    /// copy used to run `fs::copy` before anything created `dst`, so it only worked when the
    /// filesystem happened to list a directory first. APFS lists the executable first, which
    /// is how this failed on macOS and nowhere else.
    #[test]
    fn copy_dir_creates_the_destination_itself() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("bundle");
        let dst = temp.path().join("usr");

        stdfs::create_dir_all(&src).unwrap();
        // Only a file, so the very first entry cannot be a directory.
        stdfs::write(src.join("demo"), "#!/bin/sh\n").unwrap();

        copy_dir(&src, &dst).unwrap();

        assert_eq!(
            stdfs::read_to_string(dst.join("demo")).unwrap(),
            "#!/bin/sh\n"
        );
    }

    #[test]
    fn directory_size_sums_files_recursively() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        stdfs::create_dir_all(root.join("nested/deeper")).unwrap();
        stdfs::write(root.join("a.bin"), vec![0u8; 100]).unwrap();
        stdfs::write(root.join("nested/b.bin"), vec![0u8; 250]).unwrap();
        stdfs::write(root.join("nested/deeper/c.bin"), vec![0u8; 7]).unwrap();

        assert_eq!(directory_size(root), 357);
    }

    #[test]
    fn directory_size_of_an_empty_or_missing_directory_is_zero() {
        let temp = TempDir::new().unwrap();

        assert_eq!(directory_size(temp.path()), 0);
        assert_eq!(directory_size(&temp.path().join("does-not-exist")), 0);
    }

    #[test]
    fn package_relative_path_keeps_absolute_targets_inside_the_package() {
        assert_eq!(
            package_relative_path(Path::new("/usr/share/doc/demo/LICENSE")).unwrap(),
            Path::new("usr/share/doc/demo/LICENSE")
        );
        assert_eq!(
            package_relative_path(Path::new("./usr/bin/tool")).unwrap(),
            Path::new("usr/bin/tool")
        );
    }

    #[test]
    fn package_relative_path_refuses_to_leave_the_package() {
        for target in ["../x", "usr/../../x", "/usr/../x"] {
            assert_eq!(
                package_relative_path(Path::new(target)),
                Err(PackagePathProblem::EscapesRoot),
                "{target}"
            );
        }
    }

    #[test]
    fn package_relative_path_refuses_the_package_root_itself() {
        for target in ["", "/", "."] {
            assert_eq!(
                package_relative_path(Path::new(target)),
                Err(PackagePathProblem::IsRoot),
                "{target}"
            );
        }
    }
}
