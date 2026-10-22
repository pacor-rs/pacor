use std::{
    collections::btree_map,
    ffi::OsStr,
    fmt::Display,
    path::{Path, PathBuf},
};

use pacor_core::error::{BuildError, Error};
use pacor_core::utils::fs::ensure_directory_exists;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IconFormat {
    Png,
    Svg,
}

impl IconFormat {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            IconFormat::Png => "png",
            IconFormat::Svg => "svg",
        }
    }
}

impl TryFrom<&OsStr> for IconFormat {
    type Error = pacor_core::error::Error;

    fn try_from(value: &OsStr) -> std::result::Result<Self, Self::Error> {
        match value.to_ascii_lowercase().to_string_lossy() {
            std::borrow::Cow::Borrowed("png") => Ok(IconFormat::Png),
            std::borrow::Cow::Borrowed("svg") => Ok(IconFormat::Svg),
            _ => Err(Error::Build(BuildError::ArtifactGeneration {
                artifact: "icon".to_string(),
                reason: "Invalid icon format".to_string(),
            })),
        }
    }
}

impl Display for IconFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub(super) struct Icon {
    size: u32,
    path: PathBuf,
    format: IconFormat,
}

impl Icon {
    pub(super) fn new(size: u32, path: PathBuf, format: IconFormat) -> Self {
        Self { size, path, format }
    }

    pub(super) fn size(&self) -> u32 {
        self.size
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn format(&self) -> IconFormat {
        self.format
    }
}

/// Directory of the hicolor theme an icon of this format belongs in.
///
/// A theme only searches the directories its `index.theme` lists, context included:
/// an icon written straight into `<size>x<size>/` is never found, and the launcher
/// that reads the desktop entry falls back to a placeholder. Vectors live in
/// `scalable/apps` whatever size they claim, which is what the theme expects.
pub(super) fn hicolor_relative_dir(icon: &Icon) -> PathBuf {
    // Built component by component: a separator baked into a string keeps the
    // host's convention out of it, and the result is joined again by the callers.
    match icon.format() {
        IconFormat::Svg => PathBuf::from("scalable").join("apps"),
        IconFormat::Png => PathBuf::from(format!("{}x{}", icon.size(), icon.size())).join("apps"),
    }
}

pub(super) fn install_icons_to_hicolor_dir(
    app_name: &str,
    hicolor_dir: &Path,
    icons: btree_map::Values<'_, u32, Icon>,
) -> anyhow::Result<()> {
    for icon in icons {
        let dest_dir = hicolor_dir.join(hicolor_relative_dir(icon));
        ensure_directory_exists(&dest_dir)?;
        let dest = dest_dir.join(format!("{}.{}", app_name, icon.format()));
        std::fs::copy(icon.path(), &dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_icon_format_as_str() {
        assert_eq!(IconFormat::Png.as_str(), "png");
        assert_eq!(IconFormat::Svg.as_str(), "svg");
    }

    #[test]
    fn test_icon_format_display() {
        assert_eq!(format!("{}", IconFormat::Png), "png");
        assert_eq!(format!("{}", IconFormat::Svg), "svg");
    }

    #[test]
    fn test_icon_format_try_from_valid() {
        assert_eq!(
            IconFormat::try_from(OsStr::new("png")).unwrap(),
            IconFormat::Png
        );
        assert_eq!(
            IconFormat::try_from(OsStr::new("svg")).unwrap(),
            IconFormat::Svg
        );
        assert_eq!(
            IconFormat::try_from(OsStr::new("PNG")).unwrap(),
            IconFormat::Png
        );
        assert_eq!(
            IconFormat::try_from(OsStr::new("SVG")).unwrap(),
            IconFormat::Svg
        );
    }

    #[test]
    fn test_icon_format_try_from_invalid() {
        assert!(IconFormat::try_from(OsStr::new("gif")).is_err());
        assert!(IconFormat::try_from(OsStr::new("jpeg")).is_err());
    }

    /// The context directory is part of the theme's lookup path: without it the
    /// installed icon is invisible to every icon theme.
    #[test]
    fn icons_land_in_the_apps_context_of_their_size() {
        let png = Icon::new(32, PathBuf::from("/icons/32x32.png"), IconFormat::Png);
        assert_eq!(
            hicolor_relative_dir(&png),
            Path::new("32x32").join("apps"),
            "raster icons belong in <size>x<size>/apps"
        );

        let svg = Icon::new(48, PathBuf::from("/icons/48x48.svg"), IconFormat::Svg);
        assert_eq!(
            hicolor_relative_dir(&svg),
            Path::new("scalable").join("apps"),
            "vectors belong in scalable/apps whatever size they claim"
        );
    }

    #[test]
    fn test_icon_new() {
        let icon = Icon::new(32, PathBuf::from("/icons/32x32.png"), IconFormat::Png);
        assert_eq!(icon.size(), 32);
        assert_eq!(icon.path(), Path::new("/icons/32x32.png"));
        assert_eq!(icon.format(), IconFormat::Png);
    }

    #[test]
    fn test_icon_format_equality() {
        assert_eq!(IconFormat::Png, IconFormat::Png);
        assert_ne!(IconFormat::Png, IconFormat::Svg);
    }
}