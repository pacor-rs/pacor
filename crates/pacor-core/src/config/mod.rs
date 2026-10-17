//! Configuration model and loader for `pacor`.
//!
//! A single `pacor.toml` file describes every supported platform. All platform
//! sections are always compiled and parsed, regardless of the host, so the same
//! file can be used on macOS, Linux and Windows; `pacor build --target <os>`
//! selects which section is consumed.

pub mod android;
pub mod app;
pub mod build;
pub mod linux;
pub mod macos;
pub mod windows;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use android::{AndroidConfig, AndroidPackageFormat};
pub use app::{AppConfig, AppMetadata};
pub use build::{BuildOptions, FlutterBuildMode};
pub use linux::{DebianConfig, LinuxConfig, LinuxPackageFormat};
pub use macos::MacOSBuildConfig;
pub use windows::{InstallerFormat, WindowsConfig};

use crate::core::freedesktop::CategoryProblem;
use crate::core::project::find_flutter_project_root;
use crate::error::{ConfigurationError, Error, Result};

/// The default configuration file name looked up in the Flutter project root.
pub const CONFIG_FILE_NAME: &str = "pacor.toml";

/// Commented template written by `pacor init`.
pub const CONFIG_TEMPLATE: &str = include_str!("template.toml");

/// Serde default used by platform sections that are implicitly enabled.
pub(crate) fn default_enabled() -> bool {
    true
}

/// Platform-specific configuration sections.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PlatformsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<MacOSBuildConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<WindowsConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android: Option<AndroidConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<LinuxConfig>,
}

fn project_root() -> Result<PathBuf> {
    find_flutter_project_root().map_err(|error| {
        Error::Configuration(ConfigurationError::ValidationFailed {
            field: "project_root".to_string(),
            reason: format!("could not locate the Flutter project: {error}"),
        })
    })
}

/// Root configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PacorConfig {
    pub app: AppConfig,
    #[serde(default)]
    pub build: BuildOptions,
    #[serde(default)]
    pub platforms: PlatformsConfig,
}

impl PacorConfig {
    /// Load the configuration from `pacor.toml` in the Flutter project root.
    pub fn load() -> Result<Self> {
        Self::load_from(None)
    }

    /// Load the configuration from an explicit path, or from the project root.
    pub fn load_from(explicit_path: Option<&Path>) -> Result<Self> {
        let path = match explicit_path {
            Some(path) => path.to_path_buf(),
            None => project_root()?.join(CONFIG_FILE_NAME),
        };

        let config = if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(|error| {
                Error::Configuration(ConfigurationError::InvalidFormat {
                    file: path.clone(),
                    reason: format!("failed to read file: {error}"),
                })
            })?;

            toml::from_str(&content).map_err(|error| {
                Error::Configuration(ConfigurationError::InvalidFormat {
                    file: path.clone(),
                    reason: format!("invalid TOML: {error}"),
                })
            })?
        } else {
            Self::default()
        };

        let config = config.apply_environment()?;
        config.validate()?;
        Ok(config)
    }

    /// Apply the small, explicit set of environment overrides.
    ///
    /// Secrets are intentionally *not* handled here: they are resolved lazily by
    /// the platform accessors so they never end up in a serialized config.
    pub fn apply_environment(mut self) -> Result<Self> {
        if let Ok(output_dir) = std::env::var("PACOR_OUTPUT_DIR") {
            self.build.output_dir = output_dir;
        }

        if let Ok(mode) = std::env::var("PACOR_BUILD_MODE") {
            self.build.flutter_build_mode = mode.parse().map_err(|reason| {
                Error::Configuration(ConfigurationError::ValidationFailed {
                    field: "PACOR_BUILD_MODE".to_string(),
                    reason,
                })
            })?;
        }

        Ok(self)
    }

    /// Validate the configuration for consistency and completeness.
    pub fn validate(&self) -> Result<()> {
        self.app.validate()?;

        if self.build.output_dir.trim().is_empty() {
            return Err(Error::Configuration(ConfigurationError::MissingRequired {
                field: "build.output_dir".to_string(),
            }));
        }

        self.validate_desktop_entry()?;

        Ok(())
    }

    /// Validate `[platforms.linux.desktop]`.
    ///
    /// A desktop entry without a main category cannot be placed in any menu, so
    /// that is fatal. Missing *related* categories are only deviations from the
    /// registry's suggestions, so they are reported as warnings.
    fn validate_desktop_entry(&self) -> Result<()> {
        let Some(desktop) = self
            .platforms
            .linux
            .as_ref()
            .and_then(|linux| linux.desktop.as_ref())
        else {
            return Ok(());
        };

        for problem in desktop.category_problems() {
            match problem {
                CategoryProblem::MissingMainCategory { .. } => {
                    return Err(Error::Configuration(ConfigurationError::ValidationFailed {
                        field: "platforms.linux.desktop.categories".to_string(),
                        reason: problem.to_string(),
                    }));
                }
                CategoryProblem::UnrelatedAdditional { .. } => {
                    warn!("`platforms.linux.desktop.categories`: {problem}");
                }
            }
        }

        Ok(())
    }

    /// Directory that contains the Flutter project (`pubspec.yaml`).
    pub fn project_root(&self) -> Result<PathBuf> {
        project_root()
    }

    /// Absolute output directory for produced artifacts.
    pub fn output_dir(&self) -> Result<PathBuf> {
        Ok(self.project_root()?.join(&self.build.output_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_template_parses_and_validates() {
        let config: PacorConfig =
            toml::from_str(CONFIG_TEMPLATE).expect("pacor.toml template must be valid TOML");
        config
            .validate()
            .expect("pacor.toml template must be valid");
    }

    #[test]
    fn shipped_template_round_trips() {
        let config: PacorConfig = toml::from_str(CONFIG_TEMPLATE).unwrap();
        let serialized =
            toml::to_string_pretty(&config).expect("effective config must be serializable");
        let reparsed: PacorConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(
            reparsed.app.default_locale, config.app.default_locale,
            "round-trip must preserve the default locale"
        );
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let config: PacorConfig = toml::from_str(
            r#"
            [app]
            default_locale = "en-US"
            url = "https://example.com"
            [app.i18n.en-US]
            publisher = "p"
            "#,
        )
        .expect("minimal config must parse");
        config.validate().expect("minimal config must be valid");
        assert!(config.platforms.linux.is_none());
    }

    #[test]
    fn a_desktop_entry_without_a_main_category_fails_validation() {
        let config: PacorConfig = toml::from_str(
            r#"
            [app]
            default_locale = "en-US"
            url = "https://example.com"
            [app.i18n.en-US]
            publisher = "p"
            [platforms.linux.desktop]
            categories = [{ Additional = "FileTools" }]
            "#,
        )
        .unwrap();

        let error = config.validate().unwrap_err().to_string();
        assert!(
            error.contains("platforms.linux.desktop.categories"),
            "{error}"
        );
        assert!(error.contains("main category"), "{error}");
    }

    #[test]
    fn a_paired_desktop_entry_passes_validation() {
        let config: PacorConfig = toml::from_str(
            r#"
            [app]
            default_locale = "en-US"
            url = "https://example.com"
            [app.i18n.en-US]
            publisher = "p"
            [platforms.linux.desktop]
            categories = [{ Main = "Utility" }, { Additional = "FileTools" }]
            "#,
        )
        .unwrap();

        config.validate().unwrap();
    }

    #[test]
    fn all_platform_sections_parse_on_any_host() {
        let config: PacorConfig = toml::from_str(
            r#"
            [app]
            default_locale = "en-US"
            url = "https://example.com"
            [app.i18n.en-US]
            publisher = "p"
            [platforms.linux]
            formats = ["deb"]
            [platforms.windows]
            installer_format = "nsis"
            [platforms.macos]
            minimum_system_version = "10.14"
            [platforms.android]
            target_abi = ["arm64-v8a"]
            "#,
        )
        .expect("every platform section must be parseable on any host");
        assert!(config.platforms.linux.is_some());
        assert!(config.platforms.windows.is_some());
        assert!(config.platforms.macos.is_some());
        assert!(config.platforms.android.is_some());
    }
}
