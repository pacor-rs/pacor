//! Provisioning and invocation of the `linuxdeploy` AppImage tool.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use pacor_core::core::arch::Arch;
use pacor_core::utils::command::{
    CommandOutputMode, execute_command,
    macros::{command_args, command_args_extend},
};
use pacor_core::utils::downloader::download_file;
use pacor_core::utils::fs::{ensure_directory_exists, write_executable_file};

use super::appdir::AppDir;

/// Byte offset of the AppImage type-2 magic (`AI\x02`) in an AppImage file.
const APPIMAGE_MAGIC_OFFSET: u64 = 8;
/// Length of the AppImage magic.
const APPIMAGE_MAGIC_LENGTH: usize = 3;

/// Base URL for the linuxdeploy binaries.
const LINUXDEPLOY_BASE_URL: &str =
    "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy";

/// Spelling of `arch` in the linuxdeploy file names.
fn linuxdeploy_arch(arch: &Arch) -> &'static str {
    match arch {
        Arch::X64 => "x86_64",
        Arch::ARM64 => "aarch64",
    }
}

/// Pinned SHA-256 of the linuxdeploy build for `arch`.
///
/// The URL names a release tag, and a tag is not immutable: its owner can replace the asset at
/// any time, and neither TLS nor the pinned templates would notice. linuxdeploy is then
/// executed as part of every AppImage build, so a replaced asset is arbitrary code execution
/// rather than a bad artifact.
fn linuxdeploy_sha256(arch: &Arch) -> &'static str {
    match arch {
        Arch::X64 => "e762bea85c8eb0d4b3508d46e5c1f037f717d0f9303ae3b4aafc8b04991fa1ef",
        Arch::ARM64 => "b12b5cc57bd0921e1f98d73f58aa364503bc1a27f54b7a69fd2870bce7fa2f55",
    }
}

/// Return the cached `linuxdeploy` binary, downloading it on first use.
///
/// Some desktop environments try to launch any file carrying the AppImage
/// magic as an application, which breaks running the tool from a build script.
/// The magic is therefore cleared after download.
pub(super) fn prepare_linuxdeploy_tool(tools_directory: &Path, arch: &Arch) -> Result<PathBuf> {
    let tool_dir = tools_directory.join("appimage");
    ensure_directory_exists(&tool_dir)?;

    let linuxdeploy_arch = linuxdeploy_arch(arch);
    let path = tool_dir.join(format!("linuxdeploy-{linuxdeploy_arch}.AppImage"));

    if !path.exists() {
        let url = format!("{LINUXDEPLOY_BASE_URL}/linuxdeploy-{linuxdeploy_arch}.AppImage");
        debug!(url = %url, "Downloading linuxdeploy");

        let data = download_file(&url)
            .with_context(|| format!("failed to download linuxdeploy from {url}"))?;
        verify_linuxdeploy(&data, arch, &url)?;
        install_executable(&path, &data)?;

        info!(path = %path.display(), "linuxdeploy downloaded");
    }

    clear_appimage_magic(&path)?;

    Ok(path)
}

/// Reject a download that is not the binary the pinned digest describes.
///
/// Checked before anything is written into the cache, and the error names both digests because
/// the reader has to decide between an upstream change and tampering.
fn verify_linuxdeploy(data: &[u8], arch: &Arch, url: &str) -> Result<()> {
    let expected = linuxdeploy_sha256(arch);
    let actual = pacor_core::utils::crypto::sha256_hex(data);
    if actual == expected {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "{url} has SHA-256 {actual}, expected {expected}: the pinned release asset was replaced"
    ))
}

/// Move a verified download into the cache in one step.
///
/// `path.exists()` is the cache check, so a file written directly and left half-finished — a
/// full disk, a killed process — would be used as the tool on every later run, and its failure
/// would look like a broken linuxdeploy rather than an interrupted download. The temporary file
/// is a sibling of the destination, so the rename cannot cross filesystems and the cache either
/// holds the verified binary or nothing at all.
fn install_executable(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;

    let staged = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create a temporary file in {}", parent.display()))?;
    write_executable_file(staged.path(), data)?;
    staged.persist(path).map_err(|error| {
        anyhow::anyhow!(
            "failed to move the downloaded tool into {}: {}",
            path.display(),
            error.error
        )
    })?;

    Ok(())
}

/// Overwrite the AppImage magic bytes with zeroes.
///
/// This is idempotent, needs no external tool, and works on any host.
fn clear_appimage_magic(path: &Path) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open linuxdeploy at {}", path.display()))?;

    // Structurally too small to be an AppImage: nothing to clear.
    if file.metadata()?.len() < APPIMAGE_MAGIC_OFFSET + APPIMAGE_MAGIC_LENGTH as u64 {
        debug!(path = %path.display(), "File too small for an AppImage header; skipping patch");
        return Ok(());
    }

    file.seek(SeekFrom::Start(APPIMAGE_MAGIC_OFFSET))?;
    let mut current = [0u8; APPIMAGE_MAGIC_LENGTH];
    file.read_exact(&mut current)?;

    if current == [0u8; APPIMAGE_MAGIC_LENGTH] {
        trace!(path = %path.display(), "AppImage magic already cleared");
        return Ok(());
    }

    file.seek(SeekFrom::Start(APPIMAGE_MAGIC_OFFSET))?;
    file.write_all(&[0u8; APPIMAGE_MAGIC_LENGTH])?;
    file.flush()?;

    debug!(path = %path.display(), "Cleared AppImage magic bytes");
    Ok(())
}

/// Run linuxdeploy to turn `app_dir` into an AppImage.
///
/// The output is written into `output_directory`; linuxdeploy chooses the file
/// name itself, which [`super::output`] normalises afterwards.
pub(super) fn run_linuxdeploy(
    linuxdeploy_path: &Path,
    app_dir: &AppDir,
    executable: &Path,
    desktop_file: &Path,
    output_directory: &Path,
    expected_path: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    let mut args = command_args!(
        capacity = 10,
        "--appdir",
        app_dir.path(),
        "--executable",
        executable,
        "--desktop-file",
        desktop_file,
        "--output",
        "appimage",
    );

    if command_output_mode == &CommandOutputMode::Verbose {
        // linuxdeploy: 0 = debug, 1 = info (default).
        command_args_extend!(args, "--verbosity", "0");
    }

    let library_dir = app_dir.library_dir();

    // linuxdeploy's appimage plugin derives the file name from the desktop
    // entry's `Name`, sanitising characters as it goes (`My App` becomes
    // `My_App`), so the result cannot be predicted reliably. `appimagetool`
    // honours `OUTPUT`, which pins the name to the artifact pacor wants.
    let expected_path = expected_path.as_os_str();

    debug!(
        working_dir = %output_directory.display(),
        output = %Path::new(expected_path).display(),
        "Running linuxdeploy"
    );
    execute_command(
        linuxdeploy_path,
        args,
        Some(output_directory),
        Some(&[
            ("LD_LIBRARY_PATH", library_dir.as_os_str()),
            ("OUTPUT", expected_path),
        ]),
        command_output_mode,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// An AppImage header: `ELF` prefix followed by the `AI\x02` magic at offset 8.
    fn fake_appimage() -> Vec<u8> {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(b"\x7fELF");
        data[8..11].copy_from_slice(b"AI\x02");
        data
    }

    /// A truncated or mis-cased paste has to fail here rather than in a user's build, and the two
    /// architectures must not share a digest by accident.
    #[test]
    fn the_pinned_digests_are_full_lowercase_and_distinct() {
        let digests = [
            linuxdeploy_sha256(&Arch::X64),
            linuxdeploy_sha256(&Arch::ARM64),
        ];

        for digest in digests {
            assert_eq!(digest.len(), 64, "{digest}");
            assert!(
                digest
                    .chars()
                    .all(|character| character.is_ascii_digit()
                        || ('a'..='f').contains(&character)),
                "lowercase hex only: {digest}"
            );
        }

        assert_ne!(digests[0], digests[1]);
    }

    #[test]
    fn a_replaced_linuxdeploy_binary_is_refused() {
        let other = pacor_core::utils::crypto::sha256_hex(b"not linuxdeploy");

        let error = verify_linuxdeploy(b"not linuxdeploy", &Arch::X64, "https://example.com/tool")
            .unwrap_err()
            .to_string();

        assert!(error.contains(linuxdeploy_sha256(&Arch::X64)), "{error}");
        assert!(error.contains(&other), "{error}");
    }

    /// The cache holds the verified bytes and nothing else: the temporary file the download was
    /// written to must not survive next to it.
    #[test]
    fn a_verified_download_is_installed_without_leaving_staging_files() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("linuxdeploy-x86_64.AppImage");

        install_executable(&path, b"binary").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"binary");
        let entries: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1, "{entries:?}");
    }
    /// Downloads the published binaries and checks them against the pins.
    ///
    /// Ignored because it needs the network; it is the check to run when refreshing the digests,
    /// since every other test works from the constants themselves.
    #[test]
    #[ignore = "downloads both linuxdeploy AppImages; run when refreshing the pinned digests"]
    fn the_pinned_digests_match_the_published_binaries() {
        for arch in [Arch::X64, Arch::ARM64] {
            let url = format!(
                "{LINUXDEPLOY_BASE_URL}/linuxdeploy-{}.AppImage",
                linuxdeploy_arch(&arch)
            );
            let data = download_file(&url).expect("the linuxdeploy binary must download");

            verify_linuxdeploy(&data, &arch, &url)
                .expect("the published binary must match the pin");
        }
    }
    #[test]
    fn clearing_the_magic_zeroes_only_bytes_8_to_10() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("linuxdeploy.AppImage");
        fs::write(&path, fake_appimage()).unwrap();

        clear_appimage_magic(&path).unwrap();

        let data = fs::read(&path).unwrap();
        assert_eq!(&data[0..4], b"\x7fELF", "the ELF header must be untouched");
        assert_eq!(
            &data[8..11],
            &[0, 0, 0],
            "the AppImage magic must be cleared"
        );
        // Bytes after the patched window are untouched.
        assert!(data[11..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn clearing_the_magic_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("linuxdeploy.AppImage");
        fs::write(&path, fake_appimage()).unwrap();

        clear_appimage_magic(&path).unwrap();
        let once = fs::read(&path).unwrap();
        clear_appimage_magic(&path).unwrap();
        let twice = fs::read(&path).unwrap();

        assert_eq!(once, twice, "a second patch must not change the file");
    }

    #[test]
    fn clearing_the_magic_skips_files_that_are_too_small() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("tiny.AppImage");
        fs::write(&path, b"short").unwrap();

        clear_appimage_magic(&path).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"short");
    }

    #[test]
    fn clearing_the_magic_reports_a_missing_file() {
        let temp = TempDir::new().unwrap();
        let error = clear_appimage_magic(&temp.path().join("absent")).unwrap_err();
        assert!(error.to_string().contains("failed to open linuxdeploy"));
    }
}
