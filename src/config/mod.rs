//! Configuration management for the Pacor cross-platform packaging tool.
//!
//! This module provides a layered configuration system that supports:
//! - Default configuration values
//! - User-specific configuration files
//! - Platform-specific configuration overrides
//! - Configuration validation and error reporting
//!
//! # Configuration Architecture
//!
//! The configuration system follows a hierarchical approach:
//! 1. **Default Configuration**: Built-in sensible defaults
//! 2. **User Configuration**: Loaded from `pacor.toml`
//! 3. **Platform Configuration**: Platform-specific overrides
//! 4. **Runtime Configuration**: Final merged and validated configuration
//!
//! # Usage
//!
//! ```rust
//! use crate::config::BundleConfig;
//!
//! // Load configuration with automatic validation
//! let config = BundleConfig::load()?;
//!
//! // Access platform-specific settings
//! if config.platforms.macos.enabled {
//!     // Handle macOS-specific configuration
//! }
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
pub(crate) use crate::config::linux::{DebianConfig, LinuxPackageFormat};
#[cfg(target_os = "windows")]
pub(crate) use crate::config::windows::InstallerFormat;
pub(crate) use crate::config::{
    android::AndroidConfig, app::AppConfig, build::{BuildOptions, FlutterBuildMode}, macos::MacOSBuildConfig,
    windows::WindowsConfig,
};
use crate::error::{ConfigurationError, PackageError, Result};
pub(crate) use layers::ConfigurationManager;

mod android;
mod app;
mod build;
pub(crate) mod layers;
mod macos;
mod windows;

/// Platform configuration collection containing settings for all supported platforms.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct PlatformsConfig {
    pub(crate) macos: Option<MacOSBuildConfig>,
    pub(crate) windows: Option<WindowsConfig>,
    pub(crate) android: Option<AndroidConfig>,
}

/// Complete bundle configuration containing all application and build settings.
///
/// This is the root configuration structure that combines application metadata,
/// build options, and platform-specific settings into a unified configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PacorConfig {
    /// Application metadata and identification settings
    pub(crate) app: AppConfig,

    /// Build process configuration and options
    pub(crate) build: BuildOptions,

    /// Platform-specific build configurations
    pub(crate) platforms: PlatformsConfig,
}

impl PacorConfig {
    /// Load configuration using the layered configuration system.
    ///
    /// This method uses the sophisticated layered configuration approach that supports:
    /// 1. Global default values
    /// 2. Platform-specific defaults  
    /// 3. User configuration file (pacor.toml)
    /// 4. Environment variable overrides (PACOR_*)
    /// 5. CLI argument overrides (when provided)
    ///
    /// Each layer can override values from lower-priority layers, providing
    /// flexible configuration management with proper fallback behavior.
    ///
    /// # Errors
    ///
    /// Returns `ConfigurationError` variants for:
    /// - Flutter project root not found
    /// - Invalid TOML format in user configuration
    /// - Configuration validation failures
    /// - File system access errors
    ///
    /// # Example
    ///
    /// ```rust
    /// let config = BundleConfig::load()?
    ///     .map_err(|e| eprintln!("Configuration error: {}", e))?;
    /// ```
    pub(crate) fn load() -> Result<Self> {
        let mut manager = ConfigurationManager::new()?;
        let config = manager.load_merged_configuration()?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration and return layer information for debugging
    ///
    /// This method provides insight into which configuration layers were loaded
    /// and their priority order. Useful for debugging configuration issues.
    ///
    /// # Returns
    ///
    /// A tuple containing the loaded configuration and a vector of layer descriptions
    pub(crate) fn load_with_layer_info() -> Result<(Self, Vec<String>)> {
        let mut manager = ConfigurationManager::new()?;
        let config = manager.load_merged_configuration()?;
        let layer_info = manager.get_layer_info();

        // Validate the final merged configuration
        config.validate()?;

        Ok((config, layer_info))
    }

    /// Validate the entire configuration for consistency and completeness.
    ///
    /// This method performs comprehensive validation including:
    /// - Application configuration validation
    /// - Platform configuration validation
    /// - Cross-platform dependency validation
    /// - File path validation
    ///
    /// # Errors
    ///
    /// Returns `ConfigurationError::ValidationFailed` if any validation checks fail.
    pub(crate) fn validate(&self) -> Result<()> {
        // Validate application configuration
        self.app.validate().map_err(|e| {
            PackageError::Configuration(ConfigurationError::ValidationFailed {
                field: "app".to_string(),
                reason: e.to_string(),
            })
        })?;

        // Validate build options
        self.validate_build_options()?;

        // Validate platform configurations
        self.validate_platform_configs()?;

        Ok(())
    }

    /// Validate build options for consistency.
    fn validate_build_options(&self) -> Result<()> {
        // Validate output directory path
        if self.build.output_dir.is_empty() {
            return Err(PackageError::Configuration(
                ConfigurationError::MissingRequired {
                    field: "build.output_dir".to_string(),
                },
            ));
        }

        Ok(())
    }

    /// Validate platform-specific configurations.
    fn validate_platform_configs(&self) -> Result<()> {
        // Check that at least one platform is enabled
        let any_enabled = self
            .platforms
            .macos
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false)
            || self
                .platforms
                .windows
                .as_ref()
                .map(|c| c.is_enabled())
                .unwrap_or(false)
            || self
                .platforms
                .android
                .as_ref()
                .map(|c| c.enabled)
                .unwrap_or(false);

        if !any_enabled {
            return Err(PackageError::Configuration(
                ConfigurationError::ValidationFailed {
                    field: "platforms".to_string(),
                    reason: "At least one platform must be enabled".to_string(),
                },
            ));
        }

        Ok(())
    }

    /// Get the project root directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the Flutter project root cannot be determined.
    pub(crate) fn project_root(&self) -> Result<PathBuf> {
        crate::core::project::find_flutter_project_root().map_err(|e| {
            PackageError::Configuration(ConfigurationError::ValidationFailed {
                field: "project_root".to_string(),
                reason: format!("Could not determine project root: {}", e),
            })
        })
    }

    /// Get the configured output directory path.
    ///
    /// Returns the absolute path to the output directory where build artifacts
    /// will be placed. The path is constructed relative to the project root.
    ///
    /// # Errors
    ///
    /// Returns an error if the project root cannot be determined.
    pub(crate) fn output_dir(&self) -> Result<PathBuf> {
        let project_root = self.project_root()?;
        Ok(project_root.join(&self.build.output_dir))
    }
}
