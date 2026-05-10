use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Supported Windows installer formats
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InstallerFormat {
    /// Nullsoft Scriptable Install System (NSIS) installer
    Nsis,

    /// Windows Installer (MSI) package
    Msi,
}

impl InstallerFormat {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            InstallerFormat::Nsis => "nsis",
            InstallerFormat::Msi => "msi",
        }
    }
}

impl Display for InstallerFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration settings for Windows platform builds
///
/// Defines installer format, code signing options, and build preferences
/// for Windows application packaging.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct WindowsConfig {
    /// Whether Windows platform builds are enabled
    enabled: bool,

    /// The installer format to generate (NSIS or MSI)
    installer_format: InstallerFormat,

    /// Whether to build a portable (standalone) executable
    #[serde(default = "default_portable_build")]
    build_portable: bool,

    /// Path to the code signing tool executable (e.g., signtool.exe)
    signing_tool_path: Option<PathBuf>,

    /// Path to the code signing certificate file
    certificate_path: Option<PathBuf>,

    /// Password for the code signing certificate
    certificate_password: Option<String>,

    /// MSI upgrade code (GUID) for handling upgrades/downgrades
    /// Only applicable when using MSI installer format
    msi_upgrade_code: Option<String>,
}

fn default_portable_build() -> bool {
    true
}

impl WindowsConfig {
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn installer_format(&self) -> &InstallerFormat {
        &self.installer_format
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn signing_tool_path(&self) -> &Option<PathBuf> {
        &self.signing_tool_path
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn certificate_path(&self) -> &Option<PathBuf> {
        &self.certificate_path
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn certificate_password(&self) -> &Option<String> {
        &self.certificate_password
    }

    // #[cfg(target_os = "windows")]
    // pub(crate) fn msi_upgrade_code(&self) -> Option<&str> {
    //     self.msi_upgrade_code.as_deref()
    // }

    #[cfg(target_os = "windows")]
    pub(crate) fn is_portable_build_enabled(&self) -> bool {
        self.build_portable
    }
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            installer_format: InstallerFormat::Nsis,
            signing_tool_path: None,
            certificate_path: None,
            certificate_password: None,
            msi_upgrade_code: None,
            build_portable: true,
        }
    }
}
