use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    config::FlutterBuildMode,
    core::arch::Arch,
    error::{ConfigurationError, ProjectError, ValidationError},
    platforms::Platform,
    utils::command::CommandOutputMode,
    utils::command::{execute_command, flutter_executable},
};

/// Flutter project information (read from pubspec.yaml)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlutterProject {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
}

impl FlutterProject {
    /// Load project info from pubspec.yaml
    pub fn load_from_pubspec(project_root: &Path) -> Result<Self> {
        let pubspec_path = project_root.join("pubspec.yaml");

        if !pubspec_path.exists() {
            return Err(ValidationError::InvalidProjectStructure {
                reason: "pubspec.yaml not found".to_string(),
            }
            .into());
        }

        let pubspec_content = std::fs::read_to_string(&pubspec_path)?;

        let pubspec: FlutterProject = serde_yaml::from_str(&pubspec_content).map_err(|e| {
            ConfigurationError::DeserializationFailed {
                reason: e.to_string(),
            }
        })?;

        Ok(pubspec)
    }

    /// Get the Flutter build path for a release build.
    ///
    /// Shorthand for [`Self::get_flutter_build_path_for_mode`] with
    /// [`FlutterBuildMode::Release`], for backends that only ever build release.
    pub fn get_flutter_build_path(
        &self,
        project_root: &Path,
        platform: Platform,
        arch: &Arch,
    ) -> PathBuf {
        self.get_flutter_build_path_for_mode(
            project_root,
            platform,
            arch,
            FlutterBuildMode::Release,
        )
    }

    /// Directory Flutter writes Android APKs to.
    ///
    /// APKs land in a single directory because the ABI and the build mode are
    /// part of the file name, so this needs neither [`Arch`] nor a build mode.
    /// Keeping it free of the host architecture is what lets an Android build
    /// run on any host pacor itself supports.
    pub fn android_build_path(project_root: &Path) -> PathBuf {
        project_root.join("build/app/outputs/flutter-apk")
    }

    /// Directory Flutter writes Android App Bundles to.
    ///
    /// Unlike APKs, the bundle lives in a mode-named subdirectory
    /// (`bundle/release/`), and with a product flavor in
    /// `bundle/<flavor><Mode>/`; see [`crate`]-level notes in the Android
    /// backend's `bundle` module for the lookup rules.
    pub fn android_bundle_path(project_root: &Path) -> PathBuf {
        project_root.join("build/app/outputs/bundle")
    }

    /// Get Flutter build path for a given platform, architecture and build mode.
    ///
    /// Flutter writes desktop bundles into a mode-named directory (`Release`,
    /// `Debug`, `Profile`); Android APKs land in a single directory because the
    /// mode is part of the file name instead.
    pub fn get_flutter_build_path_for_mode(
        &self,
        project_root: &Path,
        platform: Platform,
        arch: &Arch,
        mode: FlutterBuildMode,
    ) -> PathBuf {
        match platform {
            Platform::Android => Self::android_build_path(project_root),
            Platform::Linux => project_root.join(format!(
                "build/linux/{}/{}/bundle",
                arch.as_str(),
                mode.as_str()
            )),
            Platform::MacOS => {
                project_root.join(format!("build/macos/Build/Products/{}", mode.dir_name()))
            }
            Platform::Windows => project_root
                .join("build")
                .join("windows")
                .join(arch.as_str())
                .join("runner")
                .join(mode.dir_name()),
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn version(&self) -> &str {
        self.version.as_str()
    }

    /// Returns the project description
    ///
    /// macOS not supported
    pub fn description(&self) -> &str {
        self.description.as_str()
    }
}

/// Find the project root directory (the one containing pubspec.yaml)
pub fn find_flutter_project_root() -> Result<PathBuf> {
    let start_dir = std::env::current_dir().context("Failed to get current directory")?;

    let mut current = Some(start_dir.as_path());
    while let Some(dir) = current {
        if dir.join("pubspec.yaml").exists() {
            return Ok(dir.to_path_buf());
        }
        current = dir.parent();
    }

    Err(ProjectError::RootNotFound.into())
}

/// Validate project environment
pub fn validate_project_environment() -> Result<(), ProjectError> {
    // Check if Flutter is installed
    if !is_flutter_installed() {
        return Err(ProjectError::FlutterNotInstalled);
    }

    Ok(())
}

/// Check if Flutter is installed
fn is_flutter_installed() -> bool {
    execute_command(
        flutter_executable(),
        &["--version"],
        None,
        None::<&[(&str, &str)]>,
        &CommandOutputMode::default(),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> FlutterProject {
        serde_yaml::from_str("name: my_app\nversion: 1.0.0+1\n").unwrap()
    }

    #[test]
    fn windows_bundles_live_in_a_mode_named_directory() {
        let root = Path::new("/project");

        assert_eq!(
            project().get_flutter_build_path(root, Platform::Windows, &Arch::X64),
            Path::new("/project/build/windows/x64/runner/Release")
        );
        assert_eq!(
            project().get_flutter_build_path_for_mode(
                root,
                Platform::Windows,
                &Arch::ARM64,
                FlutterBuildMode::Debug
            ),
            Path::new("/project/build/windows/arm64/runner/Debug")
        );
    }

    #[test]
    fn release_paths_are_unchanged_for_the_other_desktop_targets() {
        let root = Path::new("/project");

        assert_eq!(
            project().get_flutter_build_path(root, Platform::Linux, &Arch::X64),
            Path::new("/project/build/linux/x64/release/bundle")
        );
        assert_eq!(
            project().get_flutter_build_path(root, Platform::MacOS, &Arch::ARM64),
            Path::new("/project/build/macos/Build/Products/Release")
        );
        assert_eq!(
            project().get_flutter_build_path_for_mode(
                root,
                Platform::MacOS,
                &Arch::ARM64,
                FlutterBuildMode::Profile
            ),
            Path::new("/project/build/macos/Build/Products/Profile")
        );
    }

    #[test]
    fn android_ignores_the_build_mode() {
        let root = Path::new("/project");

        assert_eq!(
            FlutterProject::android_build_path(root),
            Path::new("/project/build/app/outputs/flutter-apk"),
            "the APK directory is independent of the host architecture"
        );
        assert_eq!(
            FlutterProject::android_bundle_path(root),
            Path::new("/project/build/app/outputs/bundle"),
            "the App Bundle directory holds one mode/flavor subdirectory each"
        );

        for mode in [
            FlutterBuildMode::Release,
            FlutterBuildMode::Debug,
            FlutterBuildMode::Profile,
        ] {
            assert_eq!(
                project().get_flutter_build_path_for_mode(
                    root,
                    Platform::Android,
                    &Arch::ARM64,
                    mode
                ),
                Path::new("/project/build/app/outputs/flutter-apk")
            );
        }
    }
}
