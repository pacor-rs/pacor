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

use std::path::{Component, Path, PathBuf};

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
use crate::utils::fs::package_relative_path;

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
    ///
    /// An explicitly named file must exist: falling back to the defaults hid a
    /// mistyped `--config` behind a build that used none of the user's settings.
    /// Only the default location is optional, and its absence is logged.
    pub fn load_from(explicit_path: Option<&Path>) -> Result<Self> {
        let config = match explicit_path {
            Some(path) => Self::from_file(path)?,
            None => {
                let path = project_root()?.join(CONFIG_FILE_NAME);
                if path.exists() {
                    Self::from_file(&path)?
                } else {
                    debug!(
                        path = %path.display(),
                        "No configuration file found; using the built-in defaults"
                    );
                    Self::default()
                }
            }
        };

        let config = config.apply_environment()?;
        config.validate()?;
        Ok(config)
    }

    /// Read and parse one configuration file.
    fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::Configuration(ConfigurationError::FileNotFound {
                    path: path.to_path_buf(),
                })
            } else {
                Error::Configuration(ConfigurationError::InvalidFormat {
                    file: path.to_path_buf(),
                    reason: format!("failed to read file: {error}"),
                })
            }
        })?;

        toml::from_str(&content).map_err(|error| {
            Error::Configuration(ConfigurationError::InvalidFormat {
                file: path.to_path_buf(),
                reason: format!("invalid TOML: {error}"),
            })
        })
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

        if let Some(reason) = Self::output_dir_problem(&self.build.output_dir) {
            return Err(Error::Configuration(ConfigurationError::ValidationFailed {
                field: "build.output_dir".to_string(),
                reason: format!("`{}` {reason}", self.build.output_dir),
            }));
        }

        self.validate_desktop_entry()?;
        self.validate_debian_files()?;

        Ok(())
    }

    /// Validate `[platforms.linux.deb].files`.
    ///
    /// The targets are paths inside the package root, so a `..` component would
    /// write to wherever the build happens to run — outside the package being
    /// assembled and, for a project that came from somewhere else, into the
    /// packager's own files. Rejecting it here names the offending entry before any
    /// of the build has run.
    fn validate_debian_files(&self) -> Result<()> {
        let Some(deb) = self
            .platforms
            .linux
            .as_ref()
            .and_then(|linux| linux.deb.as_ref())
        else {
            return Ok(());
        };

        for target in deb.files().values() {
            if let Err(problem) = package_relative_path(target) {
                return Err(Error::Configuration(ConfigurationError::ValidationFailed {
                    field: "platforms.linux.deb.files".to_string(),
                    reason: format!("`{}` {problem}", target.display()),
                }));
            }
        }

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

    /// Why `output_dir` cannot be the artifact directory, if it cannot be.
    ///
    /// The directory is removed wholesale by `pacor clean`, so it has to stay
    /// inside the project: an absolute path could point at the home directory,
    /// `..` could leave the project, and `.` is the project itself.
    fn output_dir_problem(output_dir: &str) -> Option<&'static str> {
        if output_dir.trim().is_empty() {
            return Some("must not be empty");
        }

        let path = Path::new(output_dir);
        if path.is_absolute() {
            return Some("must be relative to the project root");
        }

        let mut has_name = false;
        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => {
                    return Some("must be relative to the project root");
                }
                Component::ParentDir => return Some("must not contain `..`"),
                Component::CurDir => {}
                Component::Normal(_) => has_name = true,
            }
        }

        if !has_name {
            return Some("must not be the project root itself");
        }

        None
    }

    /// Absolute output directory for produced artifacts.
    pub fn output_dir(&self) -> Result<PathBuf> {
        if let Some(reason) = Self::output_dir_problem(&self.build.output_dir) {
            return Err(Error::Configuration(ConfigurationError::ValidationFailed {
                field: "build.output_dir".to_string(),
                reason: format!("`{}` {reason}", self.build.output_dir),
            }));
        }

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

    /// `files` copies into the directory that becomes the package, so a target that
    /// walks back out of it writes wherever the build happens to run instead.
    #[test]
    fn a_deb_files_target_may_not_leave_the_package_root() {
        for target in ["../etc/passwd", "/usr/../../etc/passwd", "usr/./../../x"] {
            let config: PacorConfig = toml::from_str(&format!(
                r#"
                [app]
                default_locale = "en-US"
                url = "https://example.com"
                [app.i18n.en-US]
                publisher = "p"
                [platforms.linux.deb]
                files = {{ "LICENSE" = "{target}" }}
                "#
            ))
            .unwrap();

            let error = config.validate().unwrap_err().to_string();
            assert!(
                error.contains("platforms.linux.deb.files"),
                "{target}: {error}"
            );
            assert!(error.contains(target), "{target}: {error}");
        }
    }

    #[test]
    fn a_deb_files_target_inside_the_package_passes_validation() {
        for target in [
            "/usr/share/doc/demo/LICENSE",
            "usr/share/demo/data.json",
            "./usr/bin/tool",
        ] {
            let config: PacorConfig = toml::from_str(&format!(
                r#"
                [app]
                default_locale = "en-US"
                url = "https://example.com"
                [app.i18n.en-US]
                publisher = "p"
                [platforms.linux.deb]
                files = {{ "LICENSE" = "{target}" }}
                "#
            ))
            .unwrap();

            config
                .validate()
                .unwrap_or_else(|error| panic!("{target} must be accepted: {error}"));
        }
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

    #[test]
    fn an_absolute_output_directory_is_rejected() {
        let config = config_with_output_dir("/tmp/pacor-artifacts");

        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("build.output_dir"), "{error}");
        assert!(error.contains("relative to the project root"), "{error}");
    }

    #[test]
    fn an_output_directory_that_leaves_the_project_is_rejected() {
        for output_dir in ["..", "../dist", "artifacts/../../dist"] {
            let config = config_with_output_dir(output_dir);

            let error = config.validate().unwrap_err().to_string();
            assert!(
                error.contains("must not contain `..`"),
                "{output_dir}: {error}"
            );
        }
    }

    #[test]
    fn the_project_root_is_not_a_valid_output_directory() {
        for output_dir in [".", "./", ""] {
            let config = config_with_output_dir(output_dir);

            let error = config.validate().unwrap_err().to_string();
            assert!(error.contains("build.output_dir"), "{output_dir}: {error}");
        }
    }

    #[test]
    fn a_plain_relative_output_directory_is_accepted() {
        for output_dir in ["dist", "./dist", "artifacts/linux"] {
            config_with_output_dir(output_dir)
                .validate()
                .unwrap_or_else(|error| panic!("{output_dir} must be accepted: {error}"));
        }
    }

    #[test]
    fn output_dir_refuses_an_escaping_path_before_resolving_the_project() {
        let error = config_with_output_dir("../dist")
            .output_dir()
            .unwrap_err()
            .to_string();

        assert!(error.contains("must not contain `..`"), "{error}");
    }

    fn config_with_output_dir(output_dir: &str) -> PacorConfig {
        let mut config: PacorConfig = toml::from_str(
            r#"
            [app]
            default_locale = "en-US"
            url = "https://example.com"
            [app.i18n.en-US]
            publisher = "p"
            "#,
        )
        .expect("minimal config must parse");
        config.build.output_dir = output_dir.to_string();
        config
    }

    #[test]
    fn an_explicit_configuration_path_that_does_not_exist_is_an_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing = temp.path().join("absent").join(CONFIG_FILE_NAME);

        let error = PacorConfig::load_from(Some(&missing))
            .unwrap_err()
            .to_string();

        assert!(error.contains("Configuration file not found"), "{error}");
        assert!(error.contains(&missing.display().to_string()), "{error}");
    }

    #[test]
    fn an_explicit_configuration_path_is_read_without_a_flutter_project() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &path,
            r#"
            [app]
            default_locale = "en-US"
            url = "https://example.com"
            [app.i18n.en-US]
            publisher = "p"
            [build]
            output_dir = "artifacts"
            "#,
        )
        .unwrap();

        let config = PacorConfig::load_from(Some(&path)).unwrap();

        assert_eq!(config.build.output_dir, "artifacts");
    }

    #[test]
    fn a_directory_named_like_a_configuration_file_is_an_error() {
        let temp = tempfile::TempDir::new().unwrap();

        let error = PacorConfig::load_from(Some(temp.path()))
            .unwrap_err()
            .to_string();

        assert!(error.contains("Invalid configuration format"), "{error}");
        assert!(
            error.contains(&temp.path().display().to_string()),
            "{error}"
        );
    }
}
