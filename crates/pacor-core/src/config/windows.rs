//! Windows platform configuration (NSIS or MSI installer / portable archive).

use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Locale;

/// Supported Windows installer formats.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallerFormat {
    /// Nullsoft Scriptable Install System (NSIS) installer.
    #[default]
    Nsis,

    /// Windows Installer (MSI) package. Installs per-machine (below
    /// `%ProgramFiles%`) and needs administrator rights to install.
    Msi,
}

impl InstallerFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nsis => "nsis",
            Self::Msi => "msi",
        }
    }
}

impl std::str::FromStr for InstallerFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "nsis" => Ok(Self::Nsis),
            "msi" => Ok(Self::Msi),
            other => Err(format!(
                "unsupported Windows installer format `{other}` (expected one of: nsis, msi)"
            )),
        }
    }
}

impl Display for InstallerFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for Windows builds.
///
/// All fields are always deserializable, regardless of the host, so a single
/// `pacor.toml` can describe a Windows target when building from any OS.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowsConfig {
    /// Whether Windows builds are enabled.
    #[serde(default = "super::default_enabled")]
    pub enabled: bool,

    /// Installer format to generate.
    #[serde(default)]
    pub installer_format: InstallerFormat,

    /// Also produce a portable (standalone) ZIP archive.
    #[serde(default = "default_portable_build")]
    pub build_portable: bool,

    /// Path to the code signing tool executable (e.g. `signtool.exe`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_tool_path: Option<PathBuf>,

    /// Path to the code signing certificate file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<PathBuf>,

    /// Certificate password. Prefer the `PACOR_WINDOWS_CERT_PASSWORD` env var.
    #[serde(default, skip_serializing)]
    pub certificate_password: Option<String>,

    /// RFC 3161 timestamp server used while signing.
    ///
    /// Without a timestamp an Authenticode signature stops being valid when the
    /// certificate expires; set this to an empty string to sign without one.
    #[serde(default = "default_timestamp_url")]
    pub timestamp_url: Option<String>,

    /// MSI upgrade code (GUID). Only relevant when `installer_format = "msi"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msi_upgrade_code: Option<String>,

    /// Code page of the MSI package, e.g. `65001` for UTF-8.
    ///
    /// Only relevant when `installer_format = "msi"`. Windows Installer stores
    /// every string in a single code page per package and pacor defaults to the
    /// one belonging to the locale. Setting this is the way out when a localized
    /// string uses a character that code page cannot represent — a `©` in a
    /// `zh-CN` package fails to build with `LGHT0311`, because code page 936 has
    /// no `©`. Rewording is usually the better fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msi_codepage: Option<u32>,
}

fn default_portable_build() -> bool {
    true
}

fn default_timestamp_url() -> Option<String> {
    Some("http://timestamp.digicert.com".to_string())
}

impl WindowsConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn installer_format(&self) -> &InstallerFormat {
        &self.installer_format
    }

    pub fn signing_tool_path(&self) -> &Option<PathBuf> {
        &self.signing_tool_path
    }

    pub fn certificate_path(&self) -> &Option<PathBuf> {
        &self.certificate_path
    }

    /// Certificate password, with the environment variable taking precedence.
    pub fn certificate_password(&self) -> Option<String> {
        std::env::var("PACOR_WINDOWS_CERT_PASSWORD")
            .ok()
            .or_else(|| self.certificate_password.clone())
    }

    pub fn is_portable_build_enabled(&self) -> bool {
        self.build_portable
    }

    /// Timestamp server for Authenticode signing, or `None` when disabled.
    ///
    /// An empty value turns timestamping off, which is occasionally needed in
    /// networks without access to a timestamp authority.
    pub fn timestamp_url(&self) -> Option<&str> {
        self.timestamp_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
    }

    /// MSI `UpgradeCode`, derived from the application when not configured.    ///
    /// The `UpgradeCode` decides which earlier products an installation may
    /// replace, so it has to stay identical across releases. Deriving it from
    /// the name and publisher means a project does not have to invent a GUID,
    /// while `msi_upgrade_code` still pins it explicitly when the app is renamed
    /// or handed to another publisher.
    pub fn resolved_upgrade_code(&self, app_name: &str, publisher: &str) -> String {
        if let Some(code) = self.msi_upgrade_code.as_deref()
            && !code.trim().is_empty()
        {
            return code.trim().to_string();
        }

        crate::packaging::deterministic_uuid_v5(&format!(
            "pacor:windows:msi:{app_name}:{publisher}"
        ))
    }

    /// Code page to write into an MSI localization for `locale`.
    ///
    /// Defaults to the locale's own code page, which is what WiX would infer;
    /// [`WindowsConfig::msi_codepage`] overrides it for projects whose text does
    /// not fit.
    pub fn resolved_msi_codepage(&self, locale: &Locale) -> u32 {
        self.msi_codepage
            .unwrap_or_else(|| locale.as_msi_codepage())
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
            timestamp_url: default_timestamp_url(),
            msi_upgrade_code: None,
            msi_codepage: None,
            build_portable: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamping_defaults_to_a_public_authority() {
        let config = WindowsConfig::default();

        assert_eq!(
            config.timestamp_url(),
            Some("http://timestamp.digicert.com")
        );
    }

    #[test]
    fn an_empty_timestamp_url_disables_timestamping() {
        let config: WindowsConfig = toml::from_str("timestamp_url = \"\"").unwrap();

        assert_eq!(config.timestamp_url(), None);
    }

    #[test]
    fn an_absent_timestamp_url_keeps_the_default() {
        let config: WindowsConfig = toml::from_str("build_portable = false").unwrap();

        assert!(config.timestamp_url().is_some());
        assert!(!config.is_portable_build_enabled());
    }

    /// The code page is what explains `LGHT0311`, so its default has to follow
    /// the locale and the override has to win.
    #[test]
    fn the_msi_codepage_follows_the_locale_unless_overridden() {
        let config = WindowsConfig::default();

        assert_eq!(config.resolved_msi_codepage(&Locale::EnUs), 1252);
        assert_eq!(config.resolved_msi_codepage(&Locale::ZhCn), 936);
        assert_eq!(config.msi_codepage, None);

        let config: WindowsConfig = toml::from_str("msi_codepage = 65001").unwrap();
        assert_eq!(config.resolved_msi_codepage(&Locale::ZhCn), 65001);
        assert_eq!(config.resolved_msi_codepage(&Locale::EnUs), 65001);
    }

    /// A project that never configures a GUID still gets a stable one, and the
    /// name and publisher are what it is derived from.
    #[test]
    fn the_upgrade_code_is_derived_and_stable() {
        let config = WindowsConfig::default();

        let derived = config.resolved_upgrade_code("my_app", "My Company");
        assert_eq!(
            derived,
            config.resolved_upgrade_code("my_app", "My Company")
        );
        assert_ne!(
            derived,
            config.resolved_upgrade_code("my_app", "Other Company")
        );
        assert_ne!(
            derived,
            config.resolved_upgrade_code("other_app", "My Company")
        );
        assert!(
            derived.starts_with('{') && derived.ends_with('}'),
            "{derived}"
        );

        let configured: WindowsConfig =
            toml::from_str("msi_upgrade_code = \"{11223344-5566-7788-99AA-BBCCDDEEFF00}\"")
                .unwrap();
        assert_eq!(
            configured.resolved_upgrade_code("my_app", "My Company"),
            "{11223344-5566-7788-99AA-BBCCDDEEFF00}"
        );

        // An empty value means "derive it", not "use an empty GUID".
        let empty: WindowsConfig = toml::from_str("msi_upgrade_code = \"  \"").unwrap();
        assert!(
            empty
                .resolved_upgrade_code("my_app", "My Company")
                .starts_with('{')
        );
    }
}
