use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    cli::args::CommandOutputMode,
    error::{ConfigurationError, ProjectError, ValidationError},
    platforms::Platform,
    utils::command::{execute_command, flutter_executable},
};

/// Flutter project information (read from pubspec.yaml)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct FlutterProject {
    name: String,
    version: String,
    description: String,
}

impl FlutterProject {
    /// Load project info from pubspec.yaml
    pub(crate) fn load_from_pubspec(project_root: &Path) -> Result<Self> {
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

    /// Get Flutter build path for a given platform
    pub(crate) fn get_flutter_build_path(
        &self,
        project_root: &Path,
        platform: Platform,
    ) -> PathBuf {
        match platform {
            Platform::Android => project_root.join("build/app/outputs/flutter-apk"),
            #[cfg(target_os = "linux")]
            Platform::Linux => project_root.join("build/linux/x64/release/bundle"),
            #[cfg(target_os = "macos")]
            Platform::MacOS => project_root.join("build/macos/Build/Products/Release"),
            #[cfg(target_os = "windows")]
            Platform::Windows => project_root.join("build\\windows\\x64\\runner\\Release"),
        }
    }

    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    pub(crate) fn version(&self) -> &str {
        self.version.as_str()
    }

    /// Returns the project description
    ///
    /// macOS not supported
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub(crate) fn description(&self) -> &str {
        self.description.as_str()
    }
}

/// Find the project root directory (the one containing pubspec.yaml)
pub(crate) fn find_flutter_project_root() -> Result<PathBuf> {
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
pub(crate) fn validate_project_environment() -> Result<(), ProjectError> {
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
