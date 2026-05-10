use std::{
    collections::btree_map,
    ffi::OsStr,
    fmt::Display,
    path::{Path, PathBuf},
};

use crate::{BundleError, Result, utils::fs::ensure_directory_exists};

#[derive(Debug)]
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
    type Error = BundleError;

    fn try_from(value: &OsStr) -> Result<Self> {
        match value.to_ascii_lowercase().to_string_lossy() {
            std::borrow::Cow::Borrowed("png") => Ok(IconFormat::Png),
            std::borrow::Cow::Borrowed("svg") => Ok(IconFormat::Svg),
            _ => Err(BundleError::Build("Invalid icon format".to_string())),
        }
    }
}

impl Display for IconFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug)]
pub(super) struct Icon {
    /// 宽和高相等
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

    pub(super) fn path(&self) -> &PathBuf {
        &self.path
    }

    pub(super) fn format(&self) -> &IconFormat {
        &self.format
    }
}

pub(super) fn copy_icon_to_dir(
    app_executable_name: &str,
    dir: &Path,
    icons: btree_map::Values<'_, u32, Icon>,
) -> Result<()> {
    for icon in icons {
        let size_dir = dir.join(format!("{}x{}", icon.size(), icon.size()));
        ensure_directory_exists(&size_dir)?;
        let dest = size_dir.join(format!("{}.{}", app_executable_name, icon.format()));
        std::fs::copy(&icon.path, &dest)?;
    }
    Ok(())
}
