use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::core::freedesktop::FreeDesktop;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LinuxPackageFormat {
    AppImage,
    Deb,
    Rpm,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct DebianConfig {
    // OS-specific settings:
    /// the list of debian dependencies.
    depends: Option<Vec<String>>,
    /// the list of debian dependencies recommendations.
    recommends: Option<Vec<String>>,
    /// the list of dependencies the package provides.
    provides: Option<Vec<String>>,
    /// the list of package conflicts.
    conflicts: Option<Vec<String>>,
    /// the list of package replaces.
    replaces: Option<Vec<String>>,
    /// List of custom files to add to the deb package.
    /// Maps the path on the debian package to the path of the file to include (relative to the current working directory).
    files: HashMap<PathBuf, PathBuf>,
    /// Define the section in Debian Control file. See : <https://www.debian.org/doc/debian-policy/ch-archive.html#s-subsections>
    section: Option<String>,
    /// Change the priority of the Debian Package. By default, it is set to `optional`.
    /// Recognized Priorities as of now are :  `required`, `important`, `standard`, `optional`, `extra`
    priority: Option<String>,
    /// Path of the uncompressed Changelog file, to be stored at /usr/share/doc/package-name/changelog.gz. See
    /// <https://www.debian.org/doc/debian-policy/ch-docs.html#changelog-files-and-release-notes>
    changelog: Option<PathBuf>,
    /// Path to script that will be executed before the package is unpacked. See
    /// <https://www.debian.org/doc/debian-policy/ch-maintainerscripts.html>
    pre_install_script: Option<PathBuf>,
    /// Path to script that will be executed after the package is unpacked. See
    /// <https://www.debian.org/doc/debian-policy/ch-maintainerscripts.html>
    post_install_script: Option<PathBuf>,
    /// Path to script that will be executed before the package is removed. See
    /// <https://www.debian.org/doc/debian-policy/ch-maintainerscripts.html>
    pre_remove_script: Option<PathBuf>,
    /// Path to script that will be executed after the package is removed. See
    /// <https://www.debian.org/doc/debian-policy/ch-maintainerscripts.html>
    post_remove_script: Option<PathBuf>,
}

#[cfg(target_os = "linux")]
impl DebianConfig {
    pub(crate) fn depends(&self) -> Option<&Vec<String>> {
        self.depends.as_ref()
    }

    pub(crate) fn recommends(&self) -> Option<&Vec<String>> {
        self.recommends.as_ref()
    }

    pub(crate) fn provides(&self) -> Option<&Vec<String>> {
        self.provides.as_ref()
    }

    pub(crate) fn conflicts(&self) -> Option<&Vec<String>> {
        self.conflicts.as_ref()
    }

    pub(crate) fn replaces(&self) -> Option<&Vec<String>> {
        self.replaces.as_ref()
    }

    // pub(crate) fn files(&self) -> &HashMap<PathBuf, PathBuf> {
    //     &self.files
    // }

    pub(crate) fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub(crate) fn priority(&self) -> Option<&str> {
        self.priority.as_deref()
    }

    // pub(crate) fn changelog(&self) -> Option<&PathBuf> {
    //     self.changelog.as_ref()
    // }

    pub(crate) fn pre_install_script(&self) -> Option<&PathBuf> {
        self.pre_install_script.as_ref()
    }

    pub(crate) fn post_install_script(&self) -> Option<&PathBuf> {
        self.post_install_script.as_ref()
    }

    pub(crate) fn pre_remove_script(&self) -> Option<&PathBuf> {
        self.pre_remove_script.as_ref()
    }

    pub(crate) fn post_remove_script(&self) -> Option<&PathBuf> {
        self.post_remove_script.as_ref()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct LinuxPackageConfig {
    pub(crate) deb: Option<DebianConfig>,
}

/// Linux platform configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct LinuxConfig {
    pub(crate) enabled: bool,
    pub(crate) package_formats: HashSet<LinuxPackageFormat>,
    pub(crate) desktop: Option<FreeDesktop>,
    pub(crate) icon_directory: Option<PathBuf>,
    pub(crate) maintainer: Option<String>,
    pub(crate) package_config: Option<LinuxPackageConfig>,
}

impl Default for LinuxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            package_formats: HashSet::from([LinuxPackageFormat::AppImage]),
            desktop: None,
            icon_directory: None,
            maintainer: None,
            package_config: None,
        }
    }
}

#[cfg(target_os = "linux")]
impl LinuxConfig {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn package_formats(&self) -> &HashSet<LinuxPackageFormat> {
        &self.package_formats
    }

    pub(crate) fn desktop(&self) -> Option<FreeDesktop> {
        self.desktop.clone()
    }

    pub(crate) fn icon_directory(&self) -> Option<&Path> {
        self.icon_directory.as_deref()
    }
}
