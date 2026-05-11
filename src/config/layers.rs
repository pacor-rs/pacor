//! Layered configuration system for the Pacor packaging tool.
//!
//! This module implements a sophisticated configuration architecture that supports:
//! - Default configuration values
//! - User configuration files
//! - Environment variable overrides
//! - Platform-specific configuration
//! - Configuration validation and merging
//!
//! # Configuration Layers
//!
//! The configuration system follows a hierarchical approach where each layer
//! can override values from lower-priority layers:
//!
//! ```text
//! ┌─────────────────────┐  Highest Priority
//! │ Environment Vars    │
//! ├─────────────────────┤
//! │ CLI Arguments       │
//! ├─────────────────────┤
//! │ User Config File    │
//! ├─────────────────────┤
//! │ Platform Defaults   │
//! ├─────────────────────┤
//! │ Global Defaults     │  Lowest Priority
//! └─────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust
//! use crate::config::layers::ConfigurationManager;
//!
//! // Load and merge all configuration layers
//! let manager = ConfigurationManager::new()?;
//! let final_config = manager.load_merged_configuration()?;
//!
//! // Access configuration with validation
//! let validated_config = manager.validate_and_finalize(final_config)?;
//! ```

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::config::{
    AndroidConfig, AppConfig, BuildOptions, FlutterBuildMode, MacOSBuildConfig, PacorConfig, PlatformsConfig,
    WindowsConfig,
};
use crate::core::project::find_flutter_project_root;
use crate::error::{ConfigurationError, PackageError, Result};

/// Configuration source priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConfigurationSource {
    /// Global default values (lowest priority)
    GlobalDefaults = 0,
    /// Platform-specific defaults
    PlatformDefaults = 1,
    /// User configuration file
    UserConfig = 2,
    // /// Command-line arguments
    // CliArguments = 3,
    /// Environment variables (highest priority)
    EnvironmentVariables = 4,
}

/// Partial configuration that can be merged with other configurations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PartialBundleConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) app: Option<PartialAppConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) build: Option<PartialBuildOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) platforms: Option<PartialPlatformsConfig>,
}

/// Partial app configuration for merging
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PartialAppConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_locale: Option<crate::i18n::Locale>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) authors: Option<std::collections::HashSet<String>>,

    // Simplified for partial config - full localization merging would be more complex
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) i18n: Option<HashMap<crate::i18n::Locale, crate::config::app::AppMetadata>>,
}

/// Partial build options for merging
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PartialBuildOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) flutter_build_mode: Option<FlutterBuildMode>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_dir: Option<String>,
}

/// Partial platform configuration for merging
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PartialPlatformsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) macos: Option<crate::config::MacOSBuildConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) windows: Option<crate::config::WindowsConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) android: Option<crate::config::AndroidConfig>,
}

/// Configuration layer containing a partial configuration and its source
#[derive(Debug, Clone)]
pub(crate) struct ConfigurationLayer {
    pub(crate) source: ConfigurationSource,
    pub(crate) config: PartialBundleConfig,
    pub(crate) description: String,
}

/// Manages the layered configuration system
pub(crate) struct ConfigurationManager {
    project_root: PathBuf,
    layers: Vec<ConfigurationLayer>,
}

impl ConfigurationManager {
    /// Create a new configuration manager
    pub(crate) fn new() -> Result<Self> {
        let project_root = find_flutter_project_root().map_err(|e| {
            PackageError::Configuration(ConfigurationError::ValidationFailed {
                field: "project_root".to_string(),
                reason: format!("Could not locate Flutter project: {}", e),
            })
        })?;

        Ok(Self {
            project_root,
            layers: Vec::new(),
        })
    }

    /// Create configuration manager that works without requiring a Flutter project
    /// This is useful for commands like `init` that need to work before a project is set up
    pub(crate) fn new_standalone() -> Result<Self> {
        let current_dir = std::env::current_dir().map_err(|e| {
            PackageError::Configuration(ConfigurationError::ValidationFailed {
                field: "current_directory".to_string(),
                reason: format!("Could not get current directory: {}", e),
            })
        })?;

        Ok(Self {
            project_root: current_dir,
            layers: Vec::new(),
        })
    }

    /// Load all configuration layers and return the merged configuration
    pub(crate) fn load_merged_configuration(&mut self) -> Result<PacorConfig> {
        self.load_merged_configuration_with_cli(None)
    }

    /// Load all configuration layers with optional CLI overrides
    pub(crate) fn load_merged_configuration_with_cli(
        &mut self,
        _cli_args: Option<&crate::cli::args::Cli>,
    ) -> Result<PacorConfig> {
        info!("Loading layered configuration");

        // Load all configuration layers in priority order
        self.load_global_defaults()?;
        self.load_platform_defaults()?;
        self.load_user_configuration()?;

        // TODO: Add CLI argument overrides in future iteration
        // if let Some(cli) = cli_args {
        //     self.load_cli_overrides(cli)?;
        // }

        self.load_environment_overrides()?;

        debug!("Layers: {:?}", self.layers);

        // Merge all layers into final configuration
        let merged_config = self.merge_layers()?;

        info!(
            "Configuration merged from {} layers, final config: {:?}",
            self.layers.len(),
            merged_config
        );
        Ok(merged_config)
    }

    /// Load default configuration layers only (for init command)
    pub(crate) fn load_default_configuration(&mut self) -> Result<PacorConfig> {
        info!("Loading default configuration layers");

        // Load only default layers (no user config or environment overrides)
        self.load_global_defaults()?;
        self.load_platform_defaults()?;

        // Merge layers into final configuration
        let merged_config = self.merge_layers()?;

        info!(
            "Default configuration merged from {} layers",
            self.layers.len()
        );
        Ok(merged_config)
    }

    /// Load global default configuration
    fn load_global_defaults(&mut self) -> Result<()> {
        debug!("Loading global default configuration");

        let default_config = PartialBundleConfig {
            app: Some(PartialAppConfig::default()),
            build: Some(PartialBuildOptions {
                flutter_build_mode: Some(FlutterBuildMode::Release),
                output_dir: Some("dist".to_string()),
            }),
            platforms: Some(PartialPlatformsConfig::default()),
        };

        self.layers.push(ConfigurationLayer {
            source: ConfigurationSource::GlobalDefaults,
            config: default_config,
            description: "Global defaults".to_string(),
        });

        Ok(())
    }

    /// Load platform-specific default configuration
    fn load_platform_defaults(&mut self) -> Result<()> {
        debug!("Loading platform-specific defaults");

        let mut platform_config = PartialPlatformsConfig::default();

        // Enable current platform by default
        #[cfg(target_os = "macos")]
        {
            platform_config.macos = Some(crate::config::MacOSBuildConfig::default());
        }

        #[cfg(target_os = "windows")]
        {
            platform_config.windows = Some(crate::config::WindowsConfig::default());
        }

        #[cfg(target_os = "linux")]
        {
            platform_config.linux = Some(crate::config::LinuxConfig::default());
        }

        // Android can be built from any platform
        platform_config.android = Some(crate::config::AndroidConfig::default());

        let config = PartialBundleConfig {
            platforms: Some(platform_config),
            ..Default::default()
        };

        self.layers.push(ConfigurationLayer {
            source: ConfigurationSource::PlatformDefaults,
            config,
            description: format!("Platform defaults ({})", std::env::consts::OS),
        });

        Ok(())
    }

    /// Load user configuration from pacor.toml
    fn load_user_configuration(&mut self) -> Result<()> {
        let config_path = self.project_root.join("pacor.toml");

        if !config_path.exists() {
            debug!(
                "No user configuration file found at {}",
                config_path.display()
            );
            return Ok(());
        }

        debug!("Loading user configuration from {}", config_path.display());

        let config_content = std::fs::read_to_string(&config_path).map_err(|e| {
            PackageError::Configuration(ConfigurationError::InvalidFormat {
                file: config_path.clone(),
                reason: format!("Failed to read file: {}", e),
            })
        })?;

        // Parse as complete config first, then convert to partial
        let user_config: PacorConfig = toml::from_str(&config_content).map_err(|e| {
            PackageError::Configuration(ConfigurationError::InvalidFormat {
                file: config_path,
                reason: format!("Invalid TOML format: {}", e),
            })
        })?;

        // Convert to partial config for merging
        let partial_config = self.convert_to_partial(user_config);

        self.layers.push(ConfigurationLayer {
            source: ConfigurationSource::UserConfig,
            config: partial_config,
            description: "User configuration file (pacor.toml)".to_string(),
        });

        Ok(())
    }

    /// Load environment variable overrides
    fn load_environment_overrides(&mut self) -> Result<()> {
        debug!("Loading environment variable overrides");

        let mut env_config = PartialBundleConfig::default();
        let mut has_overrides = false;

        // Check for environment variable overrides
        if let Ok(output_dir) = env::var("PACOR_OUTPUT_DIR") {
            env_config.build = Some(PartialBuildOptions {
                output_dir: Some(output_dir),
                ..Default::default()
            });
            has_overrides = true;
            debug!("Environment override: PACOR_OUTPUT_DIR");
        }

        if let Ok(build_mode) = env::var("PACOR_BUILD_MODE") {
            let mode = match build_mode.as_str() {
                "release" => FlutterBuildMode::Release,
                "debug" => FlutterBuildMode::Debug,
                "profile" => FlutterBuildMode::Profile,
                _ => {
                    return Err(PackageError::Configuration(
                        ConfigurationError::ValidationFailed {
                            field: "PACOR_BUILD_MODE".to_string(),
                            reason: format!(
                                "Invalid build mode '{}'. Must be one of: release, debug, profile",
                                build_mode
                            ),
                        },
                    ));
                }
            };
            let build_options = env_config.build.get_or_insert_with(Default::default);
            build_options.flutter_build_mode = Some(mode);
            has_overrides = true;
            debug!("Environment override: PACOR_BUILD_MODE");
        }

        if has_overrides {
            self.layers.push(ConfigurationLayer {
                source: ConfigurationSource::EnvironmentVariables,
                config: env_config,
                description: "Environment variable overrides".to_string(),
            });
        }

        Ok(())
    }

    /// Merge all configuration layers into a single configuration
    fn merge_layers(&self) -> Result<PacorConfig> {
        debug!("Merging {} configuration layers", self.layers.len());

        // Start with empty partial config
        let mut merged = PartialBundleConfig::default();

        // Merge layers in priority order (lowest to highest)
        for layer in &self.layers {
            debug!(
                "Merging layer: {} (priority: {:?})",
                layer.description, layer.source
            );
            merged = self.merge_partial_configs(merged, layer.config.clone());
        }

        debug!("Merged partial config: {merged:?}");

        // Convert partial config to complete config
        self.convert_to_complete(merged)
    }

    /// Merge two partial configurations, with the second taking precedence
    fn merge_partial_configs(
        &self,
        mut base: PartialBundleConfig,
        override_config: PartialBundleConfig,
    ) -> PartialBundleConfig {
        // Merge app config
        if let Some(override_app) = override_config.app {
            let base_app = base.app.get_or_insert_with(Default::default);
            self.merge_partial_app_config(base_app, override_app);
        }

        // Merge build config
        if let Some(override_build) = override_config.build {
            let base_build = base.build.get_or_insert_with(Default::default);
            self.merge_partial_build_config(base_build, override_build);
        }

        // Merge platform config
        if let Some(override_platforms) = override_config.platforms {
            let base_platforms = base.platforms.get_or_insert_with(Default::default);
            self.merge_partial_platform_config(base_platforms, override_platforms);
        }

        base
    }

    /// Merge partial app configurations
    fn merge_partial_app_config(
        &self,
        base: &mut PartialAppConfig,
        override_config: PartialAppConfig,
    ) {
        if let Some(locale) = override_config.default_locale {
            base.default_locale = Some(locale);
        }
        if let Some(url) = override_config.url {
            base.url = Some(url);
        }
        if let Some(authors) = override_config.authors {
            base.authors = Some(authors);
        }
        if let Some(i18n) = override_config.i18n {
            base.i18n = Some(i18n);
        }
    }

    /// Merge partial build configurations
    fn merge_partial_build_config(
        &self,
        base: &mut PartialBuildOptions,
        override_config: PartialBuildOptions,
    ) {
        if let Some(mode) = override_config.flutter_build_mode {
            base.flutter_build_mode = Some(mode);
        }
        if let Some(dir) = override_config.output_dir {
            base.output_dir = Some(dir);
        }
    }

    /// Merge partial platform configurations
    fn merge_partial_platform_config(
        &self,
        base: &mut PartialPlatformsConfig,
        override_config: PartialPlatformsConfig,
    ) {
        if let Some(macos) = override_config.macos {
            base.macos = Some(macos);
        }
        if let Some(windows) = override_config.windows {
            base.windows = Some(windows);
        }
        if let Some(android) = override_config.android {
            base.android = Some(android);
        }
    }

    /// Convert a complete BundleConfig to PartialBundleConfig
    fn convert_to_partial(&self, config: PacorConfig) -> PartialBundleConfig {
        PartialBundleConfig {
            app: Some(PartialAppConfig {
                default_locale: Some(config.app.default_locale().clone()),
                url: Some(config.app.url().to_string()),
                authors: config.app.authors.clone(),
                i18n: Some(config.app.i18n().clone()),
            }),
            build: Some(PartialBuildOptions {
                flutter_build_mode: Some(config.build.flutter_build_mode),
                output_dir: Some(config.build.output_dir),
            }),
            platforms: Some(PartialPlatformsConfig {
                macos: config.platforms.macos,
                windows: config.platforms.windows,
                android: config.platforms.android,
            }),
        }
    }

    /// Convert a PartialBundleConfig to complete BundleConfig
    fn convert_to_complete(&self, partial: PartialBundleConfig) -> Result<PacorConfig> {
        // Extract fields without consuming partial first
        let has_app = partial.app.is_some();

        let app_config = partial.app.ok_or_else(|| {
            PackageError::Configuration(ConfigurationError::MissingRequired {
                field: "app".to_string(),
            })
        })?;

        let build_config = partial.build.ok_or_else(|| {
            PackageError::Configuration(ConfigurationError::MissingRequired {
                field: "build".to_string(),
            })
        })?;

        let platforms_config = partial.platforms.ok_or_else(|| {
            PackageError::Configuration(ConfigurationError::MissingRequired {
                field: "platforms".to_string(),
            })
        })?;

        // Convert partial app config to complete app config
        let complete_app = if has_app {
            // Create AppConfig with available partial values
            AppConfig {
                default_locale: app_config.default_locale.unwrap_or_default(),
                i18n: app_config.i18n.unwrap_or_default(),
                authors: app_config.authors,
                url: app_config.url.unwrap_or_default(),
            }
        } else {
            AppConfig::default()
        };

        let complete_build = BuildOptions {
            flutter_build_mode: build_config
                .flutter_build_mode
                .unwrap_or_default(),
            output_dir: build_config
                .output_dir
                .unwrap_or_else(|| "dist".to_string()),
        };

        let complete_platforms = PlatformsConfig {
            macos: Some(
                platforms_config
                    .macos
                    .unwrap_or_else(MacOSBuildConfig::default),
            ),
            windows: Some(
                platforms_config
                    .windows
                    .unwrap_or_else(WindowsConfig::default),
            ),
            android: Some(
                platforms_config
                    .android
                    .unwrap_or_else(AndroidConfig::default),
            ),
        };

        Ok(PacorConfig {
            app: complete_app,
            build: complete_build,
            platforms: complete_platforms,
        })
    }

    /// Get information about loaded configuration layers
    pub(crate) fn get_layer_info(&self) -> Vec<String> {
        self.layers
            .iter()
            .map(|layer| format!("{} (priority: {})", layer.description, layer.source as u8))
            .collect()
    }
}
