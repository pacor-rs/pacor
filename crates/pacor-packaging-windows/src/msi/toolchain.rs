//! Provisioning of the pinned WiX toolset.
//!
//! Like `makensis` for NSIS, WiX is downloaded into pacor's cache instead of
//! being expected on the host: `wix314-binaries.zip` is self-contained (no .NET
//! SDK, no installer) and pinned, so an MSI build does not depend on whatever
//! the machine happens to have.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use pacor_core::{
    error::PlatformError,
    utils::{
        archive::extract_zip,
        downloader::download_file,
        fs::{ensure_directory_exists, write_file_atomically},
    },
};

/// The WiX release pacor is written against.
///
/// WiX v3.14.1 is the last v3 release (2024-03) and needs nothing but a .NET
/// Framework runtime, which every supported Windows ships. WiX v4 and later only
/// ship as an installed tool or a NuGet package, which would break the
/// "everything in the cache" model.
const WIX_DOWNLOAD_URL: &str =
    "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip";

/// SHA-256 of the archive that URL serves (41,297,555 bytes).
///
/// The URL names a *release*, and a release asset can be replaced by whoever owns the
/// repository — `candle.exe` and `light.exe` are then run on the user's machine, so the digest
/// is what makes the toolset the one this code was written against.
const WIX_ARCHIVE_SHA256: &str = "6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31";

/// Template files of the pinned tag that MSI generation needs, as
/// `(path in the tag, file name inside the cache directory)`.
///
/// The toolset binaries live in the same directory: the tag is part of its path,
/// so a template contract change also re-provisions the toolchain (see
/// `pacor_core::tools`).
const WIX_TEMPLATE_FILES: &[(&str, &str)] = &[
    ("windows/wix/main.wxs", "main.wxs"),
    ("windows/wix/culture.wxl.tmpl", "culture.wxl.tmpl"),
];

/// Marker written once the toolset has been unpacked.
const EXTRACTED_MARKER: &str = ".extracted";

/// The executables an MSI build drives.
pub(super) struct WixToolset {
    /// `candle.exe`: compiles `.wxs` sources into `.wixobj`.
    pub(super) candle: PathBuf,

    /// `light.exe`: links `.wixobj` into the `.msi`.
    pub(super) light: PathBuf,
}

/// Makes sure the toolset and the templates are in `install_dir`, and returns
/// the executables to run.
pub(super) fn ensure(install_dir: &Path) -> Result<WixToolset> {
    ensure_templates(install_dir)?;
    ensure_toolset(install_dir)
}

/// Path of a WiX template inside the cache directory.
pub(super) fn template_path(install_dir: &Path, file_name: &str) -> PathBuf {
    install_dir.join(file_name)
}

/// Downloads the templates of the pinned tag that an MSI build renders.
fn ensure_templates(install_dir: &Path) -> Result<()> {
    let pending: Vec<(String, PathBuf)> = WIX_TEMPLATE_FILES
        .iter()
        .filter_map(|(remote, local)| {
            let destination = install_dir.join(local);

            if destination.exists() {
                tracing::debug!(path = %destination.display(), "WiX template already cached");
                return None;
            }

            Some((
                pacor_core::tools::template_url(remote),
                destination.to_path_buf(),
            ))
        })
        .collect();

    if pending.is_empty() {
        return Ok(());
    }

    ensure_directory_exists(install_dir)?;
    tracing::info!(
        repository = pacor_core::tools::TOOLS_REPOSITORY,
        tag = pacor_core::tools::TEMPLATE_REF,
        files = pending.len(),
        "Preparing WiX templates"
    );

    for (url, destination) in pending {
        let data = download_file(&url).with_context(|| format!("failed to download {url}"))?;
        write_file_atomically(&destination, &data)
            .with_context(|| format!("failed to write {}", destination.display()))?;
        tracing::trace!(path = %destination.display(), "WiX template cached");
    }

    Ok(())
}

/// Downloads and unpacks the pinned WiX toolset if it is not cached yet.
fn ensure_toolset(install_dir: &Path) -> Result<WixToolset> {
    let candle = install_dir.join("candle.exe");
    let light = install_dir.join("light.exe");
    let marker = install_dir.join(EXTRACTED_MARKER);

    if !marker.exists() || !candle.exists() || !light.exists() {
        tracing::info!("Installing the pinned WiX toolset...");

        let archive = download_file(WIX_DOWNLOAD_URL)?;
        verify_archive(&archive)?;
        ensure_directory_exists(install_dir)?;
        extract_zip(&archive, install_dir)?;
        fs::write(&marker, "")?;

        tracing::info!(path = %install_dir.display(), "WiX toolset installed");
    } else {
        tracing::debug!(path = %install_dir.display(), "WiX toolset already installed");
    }

    if !candle.exists() || !light.exists() {
        return Err(PlatformError::ToolNotFound {
            tool: "candle.exe / light.exe".to_string(),
            platform: "Windows".to_string(),
        }
        .into());
    }

    Ok(WixToolset { candle, light })
}

/// Reject an archive that is not the one [`WIX_ARCHIVE_SHA256`] describes.
///
/// Checked before anything is unpacked, so a rejected download leaves no half-installed toolset
/// behind. The error names both digests, because the reader has to decide whether upstream
/// replaced the file or the download was tampered with.
fn verify_archive(bytes: &[u8]) -> Result<()> {
    let actual = pacor_core::utils::crypto::sha256_hex(bytes);
    if actual == WIX_ARCHIVE_SHA256 {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "{WIX_DOWNLOAD_URL} has SHA-256 {actual}, expected {WIX_ARCHIVE_SHA256}: the pinned release asset was replaced"
    ))
}

/// Reports whether the toolset is already unpacked in `install_dir`.
///
/// Used by `pacor check` to say a first build will download it, without doing so.
pub(super) fn is_cached(install_dir: &Path) -> bool {
    install_dir.join("candle.exe").is_file()
        && install_dir.join("light.exe").is_file()
        && install_dir.join(EXTRACTED_MARKER).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A truncated or mis-cased paste has to fail here rather than in a user's build.
    #[test]
    fn the_pinned_digest_is_a_full_sha256() {
        assert_eq!(WIX_ARCHIVE_SHA256.len(), 64, "{WIX_ARCHIVE_SHA256}");
        assert!(
            WIX_ARCHIVE_SHA256
                .chars()
                .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)),
            "lowercase hex only: {WIX_ARCHIVE_SHA256}"
        );
    }

    /// A replaced release asset has to stop the build before anything is unpacked, and the error
    /// has to name both digests so the reader can tell tampering from an upstream change.
    #[test]
    fn a_replaced_wix_archive_is_refused() {
        let other = pacor_core::utils::crypto::sha256_hex(b"not the pinned archive");

        let error = verify_archive(b"not the pinned archive")
            .unwrap_err()
            .to_string();

        assert!(error.contains(WIX_ARCHIVE_SHA256), "{error}");
        assert!(error.contains(&other), "{error}");
    }

    /// Downloads the published archive and checks it against the pin.
    ///
    /// Ignored because it needs the network (41 MB); it is the check to run when refreshing
    /// [`WIX_ARCHIVE_SHA256`], since every other test works from the constant itself.
    #[test]
    #[ignore = "downloads the WiX toolset archive; run when refreshing the pinned digest"]
    fn the_pinned_wix_digest_matches_the_published_archive() {
        let bytes = download_file(WIX_DOWNLOAD_URL).expect("the WiX archive must download");

        verify_archive(&bytes).expect("the published archive must match the pin");
    }
    #[test]
    fn the_toolset_is_pinned_to_a_specific_wix_release() {
        assert!(
            WIX_DOWNLOAD_URL.contains("/wix3141rtm/"),
            "{WIX_DOWNLOAD_URL}"
        );
        assert!(
            WIX_DOWNLOAD_URL.ends_with("wix314-binaries.zip"),
            "{WIX_DOWNLOAD_URL}"
        );
    }

    /// Templates come from the same tag as the NSIS ones, so the two backends
    /// cannot drift onto different contracts.
    #[test]
    fn templates_are_fetched_from_the_pinned_tag() {
        for (remote, local) in WIX_TEMPLATE_FILES {
            let url = pacor_core::tools::template_url(remote);

            assert!(
                url.contains(&format!("/refs/tags/{}/", pacor_core::tools::TEMPLATE_REF)),
                "{url}"
            );
            assert!(!url.contains("/refs/heads/"), "{url}");
            assert!(url.ends_with(remote), "{url}");
            assert!(!local.is_empty() && !local.contains('/'), "{local}");
        }
    }

    #[test]
    fn a_cache_without_the_toolset_is_not_reported_as_cached() {
        let temp = tempfile::TempDir::new().unwrap();

        assert!(!is_cached(temp.path()));

        fs::write(temp.path().join("candle.exe"), "").unwrap();
        fs::write(temp.path().join("light.exe"), "").unwrap();
        assert!(
            !is_cached(temp.path()),
            "the extraction marker is still missing"
        );

        fs::write(temp.path().join(EXTRACTED_MARKER), "").unwrap();
        assert!(is_cached(temp.path()));
    }

    #[test]
    fn template_paths_live_inside_the_cache_directory() {
        let dir = Path::new("/cache/wix/1.1");

        assert_eq!(
            template_path(dir, "main.wxs"),
            Path::new("/cache/wix/1.1/main.wxs")
        );
    }
}
