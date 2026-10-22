//! Icon discovery and provisioning for Linux packaging.
//!
//! Icons are resolved once per build and shared by every configured package
//! format, so the AppImage and the DEB agree on the icon set.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use pacor_core::utils::archive::extract_zip;
use pacor_core::utils::downloader::download_file;
use pacor_core::utils::fs::ensure_directory_exists;

use crate::icon::{Icon, IconFormat};

/// Icon sizes pacor looks for, per the freedesktop icon theme conventions.
pub(crate) const STANDARD_ICON_SIZES: [u32; 9] = [16, 24, 32, 48, 64, 128, 256, 512, 1024];

/// Size preferred as the primary icon when it is available.
pub(crate) const PRIMARY_ICON_SIZE: u32 = 32;

/// URL of the fallback icon set, used when no `icon_directory` is configured.
///
/// Fetched from the tag pacor pins, like the build templates: the icons belong
/// to the same contract and must not drift onto a newer branch.
fn default_icons_url() -> String {
    pacor_core::tools::template_url("linux/icons.zip")
}

/// Collect the icons available for the build.
///
/// `configured_dir` is `platforms.linux.icon_directory`; when it is not set the
/// bundled default icon set is downloaded into `tools_dir/icons/<TEMPLATE_REF>`
/// on first use, so a cached set cannot outlive the tag it came from.
/// Relative directories are resolved against `project_root`.
pub(crate) fn search_icons(
    configured_dir: Option<&Path>,
    tools_dir: &Path,
    project_root: &Path,
) -> Result<BTreeMap<u32, Icon>> {
    let icon_dir = match configured_dir {
        Some(dir) => dir.to_path_buf(),
        None => {
            let default_dir = tools_dir
                .join("icons")
                .join(pacor_core::tools::TEMPLATE_REF);
            if !holds_entries(&default_dir) {
                download_default_icons(&default_dir)?;
            }
            default_dir
        }
    };

    let icon_dir = if icon_dir.is_absolute() {
        icon_dir
    } else {
        project_root.join(icon_dir)
    };

    let mut icons = BTreeMap::new();
    for size in STANDARD_ICON_SIZES {
        // SVG scales better, so prefer it when both are present.
        for format in [IconFormat::Svg, IconFormat::Png] {
            let path = icon_dir.join(format!("{size}x{size}.{format}"));
            if path.exists() {
                icons.insert(size, Icon::new(size, path, format));
                break;
            }
        }
    }

    debug!(directory = %icon_dir.display(), count = icons.len(), "Icons found");
    Ok(icons)
}

/// Pick the icon used to represent the application.
///
/// Prefers [`PRIMARY_ICON_SIZE`]; otherwise the largest available icon.
pub(crate) fn resolve_primary_icon(icons: &BTreeMap<u32, Icon>) -> Result<&Icon> {
    icons
        .get(&PRIMARY_ICON_SIZE)
        .or_else(|| icons.iter().next_back().map(|(_, icon)| icon))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no icons found; set `platforms.linux.icon_directory` to a directory containing PNG or SVG icons named \
                 {sizes}",
                sizes = STANDARD_ICON_SIZES
                    .iter()
                    .map(|size| format!("{size}x{size}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Whether `directory` holds at least one entry.
///
/// The default icon set is a directory, so "cached" cannot be `exists()`: the directory used to
/// be created before the download, and a transfer that failed left an empty one behind that
/// satisfied every later check.
fn holds_entries(directory: &Path) -> bool {
    fs::read_dir(directory)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}
/// Download and unpack the default icon set into `directory`.
fn download_default_icons(directory: &Path) -> Result<()> {
    let url = default_icons_url();
    debug!(url = %url, "Downloading default icons");

    // A directory that exists but holds nothing is a download that never finished, left by a
    // version that created it first. `remove_dir` refuses a populated directory, so a real icon
    // set is never deleted here.
    if directory.exists() {
        fs::remove_dir(directory).with_context(|| {
            format!(
                "failed to clear the empty icon cache at {}; remove it and build again",
                directory.display()
            )
        })?;
    }

    let data = download_file(&url)?;
    unpack_icons(&data, directory)?;

    info!(directory = %directory.display(), "Default icons downloaded");
    Ok(())
}

/// Unpack an icon archive into `directory` through a temporary sibling.
///
/// Extracting straight into the destination made one failed download permanent: the
/// half-written directory satisfied the cache check on the next run, the download was skipped,
/// and the build failed with "no icons found; set `platforms.linux.icon_directory`" — advice
/// pointing at the configuration while the real cause was a transfer that never completed.
fn unpack_icons(data: &[u8], directory: &Path) -> Result<()> {
    let parent = directory
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", directory.display()))?;
    ensure_directory_exists(parent)?;

    let staging = tempfile::TempDir::with_prefix_in("pacor-icons", parent)?;
    extract_zip(data, staging.path())?;

    // The staging directory is a sibling, so the rename stays on one filesystem and either
    // happens completely or not at all.
    let staged = staging.keep();
    fs::rename(&staged, directory)
        .with_context(|| format!("failed to move the icon set into {}", directory.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_icon(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), "icon").unwrap();
    }

    /// A failed download must leave nothing behind. The destination used to be created before
    /// the transfer, so the next run found a directory that existed, skipped the download and
    /// reported "no icons found; set `platforms.linux.icon_directory`" for good.
    #[test]
    fn a_failed_icon_download_leaves_no_cache_behind() {
        let temp = TempDir::new().unwrap();
        let directory = temp.path().join("icons").join("tag");
        std::fs::create_dir_all(directory.parent().unwrap()).unwrap();

        let error = unpack_icons(b"not an archive", &directory).unwrap_err();

        assert!(!directory.exists(), "{error}");
        assert!(
            !holds_entries(directory.parent().unwrap()),
            "staging left behind"
        );
    }

    /// A cache directory that exists but holds nothing is a download that never finished, from
    /// a version that created it first; it must not count as cached.
    #[test]
    fn an_empty_cache_directory_is_not_cached() {
        let temp = TempDir::new().unwrap();
        let directory = temp.path().join("icons").join("tag");
        std::fs::create_dir_all(&directory).unwrap();

        assert!(!holds_entries(&directory));

        write_icon(&directory, "32x32.png");
        assert!(holds_entries(&directory));
    }
    #[test]
    fn standard_icon_sizes_are_unique_and_ascending() {
        assert_eq!(STANDARD_ICON_SIZES.len(), 9);
        assert_eq!(STANDARD_ICON_SIZES[0], 16);
        assert_eq!(STANDARD_ICON_SIZES[8], 1024);

        for pair in STANDARD_ICON_SIZES.windows(2) {
            assert!(pair[1] > pair[0], "sizes must ascend: {pair:?}");
        }
    }

    #[test]
    fn search_reads_the_configured_directory() {
        let temp = TempDir::new().unwrap();
        let icons_dir = temp.path().join("icons");
        write_icon(&icons_dir, "32x32.png");
        write_icon(&icons_dir, "256x256.png");

        let icons = search_icons(Some(&icons_dir), temp.path(), temp.path()).unwrap();

        assert_eq!(icons.len(), 2);
        assert!(icons.contains_key(&32));
        assert!(icons.contains_key(&256));
    }

    #[test]
    fn search_prefers_svg_over_png_for_the_same_size() {
        let temp = TempDir::new().unwrap();
        let icons_dir = temp.path().join("icons");
        write_icon(&icons_dir, "32x32.png");
        write_icon(&icons_dir, "32x32.svg");

        let icons = search_icons(Some(&icons_dir), temp.path(), temp.path()).unwrap();

        assert_eq!(icons.get(&32).unwrap().format(), IconFormat::Svg);
    }

    #[test]
    fn search_resolves_relative_directories_against_the_project_root() {
        let temp = TempDir::new().unwrap();
        write_icon(&temp.path().join("assets/icons"), "48x48.png");

        let icons =
            search_icons(Some(Path::new("assets/icons")), temp.path(), temp.path()).unwrap();

        assert!(icons.contains_key(&48));
    }

    #[test]
    fn search_ignores_non_standard_sizes() {
        let temp = TempDir::new().unwrap();
        let icons_dir = temp.path().join("icons");
        write_icon(&icons_dir, "32x32.png");
        write_icon(&icons_dir, "33x33.png");
        write_icon(&icons_dir, "256x256.png");

        let icons = search_icons(Some(&icons_dir), temp.path(), temp.path()).unwrap();

        assert_eq!(icons.keys().copied().collect::<Vec<_>>(), vec![32, 256]);
    }

    #[test]
    fn primary_icon_prefers_32_pixels() {
        let temp = TempDir::new().unwrap();
        let icons_dir = temp.path().join("icons");
        write_icon(&icons_dir, "32x32.png");
        write_icon(&icons_dir, "1024x1024.png");

        let icons = search_icons(Some(&icons_dir), temp.path(), temp.path()).unwrap();
        let primary = resolve_primary_icon(&icons).unwrap();

        assert_eq!(primary.size(), 32);
    }

    #[test]
    fn primary_icon_falls_back_to_the_largest() {
        let temp = TempDir::new().unwrap();
        let icons_dir = temp.path().join("icons");
        write_icon(&icons_dir, "256x256.png");
        write_icon(&icons_dir, "512x512.png");

        let icons = search_icons(Some(&icons_dir), temp.path(), temp.path()).unwrap();
        let primary = resolve_primary_icon(&icons).unwrap();

        assert_eq!(primary.size(), 512);
    }

    #[test]
    fn primary_icon_error_names_the_expected_sizes() {
        let icons = BTreeMap::new();
        let error = resolve_primary_icon(&icons).unwrap_err();

        assert!(error.to_string().contains("16x16"));
        assert!(error.to_string().contains("1024x1024"));
        assert!(error.to_string().contains("platforms.linux.icon_directory"));
    }

    #[test]
    fn the_default_icon_set_comes_from_the_pinned_tag() {
        let url = default_icons_url();

        assert!(url.contains(pacor_core::tools::TEMPLATE_REF), "{url}");
        assert!(!url.contains("/refs/heads/"), "{url}");
        assert!(url.ends_with("/linux/icons.zip"), "{url}");
    }
}
