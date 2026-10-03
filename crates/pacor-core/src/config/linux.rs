//! Linux platform configuration (AppImage / Debian packages).

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::core::freedesktop::FreeDesktop;

/// Supported Linux package formats.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum LinuxPackageFormat {
    AppImage,
    Deb,
    Rpm,
}

impl std::str::FromStr for LinuxPackageFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "appimage" => Ok(Self::AppImage),
            "deb" => Ok(Self::Deb),
            "rpm" => Ok(Self::Rpm),
            other => Err(format!(
                "unsupported Linux format `{other}` (expected one of: appimage, deb, rpm)"
            )),
        }
    }
}

impl std::fmt::Display for LinuxPackageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppImage => write!(f, "appimage"),
            Self::Deb => write!(f, "deb"),
            Self::Rpm => write!(f, "rpm"),
        }
    }
}

/// Debian-specific package metadata.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DebianConfig {
    pub depends: Option<Vec<String>>,
    pub recommends: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub replaces: Option<Vec<String>>,
    pub files: HashMap<PathBuf, PathBuf>,
    pub section: Option<String>,
    pub priority: Option<String>,
    pub changelog: Option<PathBuf>,
    pub pre_install_script: Option<PathBuf>,
    pub post_install_script: Option<PathBuf>,
    pub pre_remove_script: Option<PathBuf>,
    pub post_remove_script: Option<PathBuf>,
}

impl DebianConfig {
    pub fn depends(&self) -> Option<&Vec<String>> {
        self.depends.as_ref()
    }

    pub fn recommends(&self) -> Option<&Vec<String>> {
        self.recommends.as_ref()
    }

    pub fn provides(&self) -> Option<&Vec<String>> {
        self.provides.as_ref()
    }

    pub fn conflicts(&self) -> Option<&Vec<String>> {
        self.conflicts.as_ref()
    }

    pub fn replaces(&self) -> Option<&Vec<String>> {
        self.replaces.as_ref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub fn priority(&self) -> Option<&str> {
        self.priority.as_deref()
    }

    pub fn pre_install_script(&self) -> Option<&PathBuf> {
        self.pre_install_script.as_ref()
    }

    pub fn post_install_script(&self) -> Option<&PathBuf> {
        self.post_install_script.as_ref()
    }

    pub fn pre_remove_script(&self) -> Option<&PathBuf> {
        self.pre_remove_script.as_ref()
    }

    pub fn post_remove_script(&self) -> Option<&PathBuf> {
        self.post_remove_script.as_ref()
    }

    pub fn files(&self) -> &HashMap<PathBuf, PathBuf> {
        &self.files
    }

    pub fn changelog(&self) -> Option<&PathBuf> {
        self.changelog.as_ref()
    }
}

/// Linux platform configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LinuxConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Package formats to produce, e.g. `["appimage", "deb"]`.
    #[serde(rename = "formats", alias = "package_formats")]
    pub formats: HashSet<LinuxPackageFormat>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<FreeDesktop>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_directory: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainer: Option<String>,

    /// Debian package metadata, used when `deb` is part of `formats`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deb: Option<DebianConfig>,
}

fn default_enabled() -> bool {
    true
}

impl Default for LinuxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            formats: HashSet::from([LinuxPackageFormat::AppImage]),
            desktop: None,
            icon_directory: None,
            maintainer: None,
            deb: None,
        }
    }
}

impl LinuxConfig {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn formats(&self) -> &HashSet<LinuxPackageFormat> {
        &self.formats
    }

    pub fn desktop(&self) -> Option<FreeDesktop> {
        self.desktop.clone()
    }

    pub fn icon_directory(&self) -> Option<&std::path::Path> {
        self.icon_directory.as_deref()
    }

    pub fn maintainer(&self) -> Option<&str> {
        self.maintainer.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linux_package_format_display() {
        assert_eq!(LinuxPackageFormat::AppImage.to_string(), "appimage");
        assert_eq!(LinuxPackageFormat::Deb.to_string(), "deb");
        assert_eq!(LinuxPackageFormat::Rpm.to_string(), "rpm");
    }

    #[test]
    fn test_linux_config_default() {
        let config = LinuxConfig::default();
        assert!(config.enabled);
        assert!(config.formats.contains(&LinuxPackageFormat::AppImage));
        assert!(!config.formats.contains(&LinuxPackageFormat::Deb));
        assert!(config.deb.is_none());
    }

    #[test]
    fn test_linux_config_serde_formats_alias() {
        let config: LinuxConfig = toml::from_str("package_formats = [\"deb\"]").unwrap();
        assert!(config.formats.contains(&LinuxPackageFormat::Deb));
        let config: LinuxConfig = toml::from_str("formats = [\"appimage\"]").unwrap();
        assert!(config.formats.contains(&LinuxPackageFormat::AppImage));
    }

    #[test]
    fn test_debian_config_accessors() {
        let config = DebianConfig {
            depends: Some(vec!["libc6".to_string()]),
            recommends: Some(vec!["libgtk3".to_string()]),
            section: Some("utils".to_string()),
            priority: Some("optional".to_string()),
            pre_install_script: Some(PathBuf::from("/scripts/preinst")),
            ..Default::default()
        };

        assert_eq!(config.depends(), Some(&vec!["libc6".to_string()]));
        assert_eq!(config.recommends(), Some(&vec!["libgtk3".to_string()]));
        assert_eq!(config.section(), Some("utils"));
        assert_eq!(config.priority(), Some("optional"));
        assert_eq!(
            config.pre_install_script(),
            Some(&PathBuf::from("/scripts/preinst"))
        );
        assert!(config.post_install_script().is_none());
    }
}
