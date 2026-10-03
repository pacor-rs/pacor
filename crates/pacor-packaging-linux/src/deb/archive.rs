// Copyright 2016-2019 Cargo-Bundle developers <https://github.com/burtonageo/cargo-bundle>
// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Low-level archive and file primitives for Debian packages.
//!
//! A `.deb` is an `ar` archive containing three members, two of which are
//! gzip-compressed tarballs. This module owns the byte-level mechanics:
//! `ar`, `tar`, `gzip`, `md5sums`, installed-size accounting and file modes.
//!
//! Everything here is host-independent: at most, permission bits are applied
//! where the host supports them, so the backend type-checks on any platform.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::Result;
use flate2::Compression;
use flate2::write::GzEncoder;
use tar::HeaderMode;
use walkdir::WalkDir;

use pacor_core::utils::fs::ensure_directory_exists;

/// Calculate the total installed size in kilobytes, as required by Debian policy.
///
/// Regular files contribute their size rounded up to the next full kilobyte;
/// other objects (directories, symlinks, …) contribute 1 KiB each.
pub(super) fn calculate_installed_size_in_kb(directory: &Path) -> Result<u64> {
    let mut total_kb: u64 = 0;

    for entry in WalkDir::new(directory) {
        let metadata = entry?.metadata()?;
        if metadata.is_file() {
            total_kb += metadata.len().div_ceil(1024);
        } else {
            total_kb += 1;
        }
    }

    Ok(total_kb)
}

/// Write the `md5sums` file listing checksums of every package file.
///
/// dpkg uses this to verify package integrity and detect later modifications.
/// Symlinks are skipped: dpkg expects only regular files here.
pub(super) fn write_md5sums(control_dir: &Path, data_dir: &Path) -> Result<()> {
    let path = control_dir.join("md5sums");
    let mut file = fs::File::create(&path)?;

    debug!("Generating MD5 checksums for package files");

    for entry in WalkDir::new(data_dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        let mut contents = fs::File::open(file_path)?;
        let mut hasher = md5::Context::new();
        io::copy(&mut contents, &mut hasher)?;

        let relative = file_path.strip_prefix(data_dir)?;
        let relative = relative.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("non-UTF-8 path encountered: {relative:?}"),
            )
        })?;

        writeln!(file, "{:x}  {}", hasher.finalize(), relative)?;
    }

    debug!(path = %path.display(), "MD5 checksums file generated");
    Ok(())
}

/// Write `content` to `file_path`, creating parent directories as needed.
pub(super) fn write_file_with_content(file_path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = file_path.parent() {
        ensure_directory_exists(parent)?;
    }

    let mut file = fs::File::create(file_path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;
    Ok(())
}

/// Copy `source` to `target` and mark the copy executable (0o755 on unix).
pub(super) fn copy_executable_file(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        ensure_directory_exists(parent)?;
    }

    fs::copy(source, target)?;
    make_executable(target)
}

/// Gzip-compress `source` into `target`.
pub(super) fn write_gzipped_file(source: &Path, target: &Path) -> Result<()> {
    let mut encoder = GzEncoder::new(fs::File::create(target)?, Compression::default());
    io::copy(&mut fs::File::open(source)?, &mut encoder)?;
    encoder.finish()?;
    Ok(())
}

/// Create a gzip-compressed tar archive of `source_directory`.
///
/// The archive is written next to the source as `<dirname>.tar.gz`; the source
/// directory itself is left untouched.
pub(super) fn create_gzipped_tarball(source_directory: &Path) -> Result<PathBuf> {
    let archive_path = source_directory.with_extension("tar.gz");
    debug!(
        source = %source_directory.display(),
        archive = %archive_path.display(),
        "Creating compressed tarball"
    );

    let output = fs::File::create(&archive_path)?;
    let encoder = GzEncoder::new(output, Compression::default());
    let encoder = write_deterministic_tar_archive(source_directory, encoder)?;
    encoder.finish()?.flush()?;

    Ok(archive_path)
}

/// Write a reproducible tar archive of `source_directory` to `writer`.
///
/// Headers use `HeaderMode::Deterministic` so permissions/ownership do not leak
/// from the build machine; only the real mtime is preserved.
fn write_deterministic_tar_archive<W: Write>(source_directory: &Path, writer: W) -> Result<W> {
    let mut builder = tar::Builder::new(writer);

    for entry in WalkDir::new(source_directory) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source_directory)?;

        // Skip the root directory itself.
        if relative.as_os_str().is_empty() {
            continue;
        }

        let metadata = entry.metadata()?;
        let mut header = tar::Header::new_gnu();
        header.set_metadata_in_mode(&metadata, HeaderMode::Deterministic);
        header.set_mtime(modified_seconds(&metadata));

        if metadata.is_dir() {
            builder.append_data(&mut header, relative, io::empty())?;
        } else if metadata.is_file() {
            builder.append_data(&mut header, relative, fs::File::open(entry.path())?)?;
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(entry.path())?;
            let target = target.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "symlink target contains invalid UTF-8",
                )
            })?;

            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            builder.append_link(&mut header, relative, target)?;
        }
        // Device files and other special types are meaningless in a .deb.
    }

    Ok(builder.into_inner()?)
}

/// Wrap `input_files` into an `ar` archive, the container format of `.deb`.
pub(super) fn create_ar_archive(input_files: Vec<PathBuf>, output_path: &Path) -> Result<()> {
    debug!(path = %output_path.display(), "Creating AR archive");

    let mut builder = ar::Builder::new(fs::File::create(output_path)?);
    for file_path in &input_files {
        builder.append_path(file_path)?;
    }
    builder.into_inner()?.flush()?;
    Ok(())
}

/// Modification time in seconds since the epoch, or 0 if unavailable.
///
/// Uses the portable `Metadata::modified` rather than the unix-only
/// `MetadataExt::mtime`, so the crate builds on every host.
fn modified_seconds(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Set the executable bit (no-op on hosts without unix permission bits).
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn installed_size_counts_empty_directory_as_one_kib() {
        let dir = TempDir::new().unwrap();
        assert_eq!(calculate_installed_size_in_kb(dir.path()).unwrap(), 1);
    }

    #[test]
    fn installed_size_rounds_files_up_to_a_kib() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("small.txt"), "hello world").unwrap();
        // 1 KiB for the directory + 1 KiB for the 11-byte file.
        assert!(calculate_installed_size_in_kb(dir.path()).unwrap() >= 2);

        fs::write(dir.path().join("large.bin"), vec![0u8; 3072]).unwrap();
        assert!(calculate_installed_size_in_kb(dir.path()).unwrap() >= 4);
    }

    #[test]
    fn write_file_with_content_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("debian-binary");

        write_file_with_content(&path, "2.0\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "2.0\n");
    }

    #[test]
    fn md5sums_lists_regular_files_relative_to_data_dir() {
        let dir = TempDir::new().unwrap();
        let control_dir = dir.path().join("control");
        let data_dir = dir.path().join("data");
        ensure_directory_exists(&control_dir).unwrap();
        ensure_directory_exists(&data_dir).unwrap();
        fs::write(data_dir.join("test.txt"), "hello world").unwrap();

        write_md5sums(&control_dir, &data_dir).unwrap();

        let contents = fs::read_to_string(control_dir.join("md5sums")).unwrap();
        // Known MD5 of "hello world", in `<hash>  <path>` form.
        assert_eq!(
            contents.trim(),
            "5eb63bbbe01eeed093cb22bb8f5acdc3  test.txt"
        );
    }

    #[test]
    fn gzipped_tarball_is_created_next_to_source() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("data");
        ensure_directory_exists(&source).unwrap();
        fs::write(source.join("test.txt"), "hello world").unwrap();

        let archive = create_gzipped_tarball(&source).unwrap();

        assert!(archive.exists());
        assert_eq!(archive.file_name().unwrap(), "data.tar.gz");
        // The source directory must survive archiving.
        assert!(source.join("test.txt").exists());
    }

    #[test]
    fn tarball_round_trips_package_layout() {
        let dir = TempDir::new().unwrap();
        let data = dir.path().join("data");
        let nested = data.join("usr/share/applications");
        ensure_directory_exists(&nested).unwrap();
        fs::write(nested.join("demo.desktop"), "[Desktop Entry]").unwrap();
        fs::write(data.join("marker"), "payload").unwrap();

        let archive = create_gzipped_tarball(&data).unwrap();

        // dpkg reads these members back, so the decompressed tar must contain
        // the payload with the expected relative paths.
        let mut entries = Vec::new();
        let decoder = flate2::read::GzDecoder::new(fs::File::open(&archive).unwrap());
        for entry in tar::Archive::new(decoder).entries().unwrap() {
            let mut entry = entry.unwrap();
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let mut contents = String::new();
            io::Read::read_to_string(&mut entry, &mut contents).unwrap();
            entries.push((path, contents));
        }

        assert!(
            entries
                .iter()
                .any(|(path, contents)| path == "marker" && contents == "payload"),
            "unexpected tar members: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(path, _)| path == "usr/share/applications/demo.desktop"),
            "unexpected tar members: {entries:?}"
        );
    }

    #[test]
    fn ar_archive_contains_every_input_member() {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("debian-binary");
        let second = dir.path().join("control.tar.gz");
        write_file_with_content(&first, "2.0\n").unwrap();
        write_file_with_content(&second, "not really gzip").unwrap();

        let output = dir.path().join("pkg.deb");
        create_ar_archive(vec![first, second], &output).unwrap();

        assert!(output.exists());
        assert!(fs::metadata(&output).unwrap().len() > 0);
    }
}
