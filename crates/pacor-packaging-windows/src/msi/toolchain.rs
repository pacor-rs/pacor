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
    utils::{archive::extract_zip, downloader::download_file, fs::ensure_directory_exists},
};

/// The WiX release pacor is written against.
///
/// WiX v3.14.1 is the last v3 release (2024-03) and needs nothing but a .NET
/// Framework runtime, which every supported Windows ships. WiX v4 and later only
/// ship as an installed tool or a NuGet package, which would break the
/// "everything in the cache" model.
const WIX_DOWNLOAD_URL: &str =
    "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip";

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
        fs::write(&destination, &data)
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
