//! Command handlers for the Pacor CLI.
//!
//! This module contains individual command handlers that implement the business logic
//! for each CLI subcommand. Each handler is responsible for:
//! - Input validation and preprocessing
//! - Calling appropriate business logic functions
//! - Error handling and user feedback
//! - Result formatting and output

use std::path::Path;

use anyhow::Result;

use crate::cli::args::CommandOutputMode;
use crate::cli::orchestrator::BuildOrchestrator;
use crate::cli::output::OutputFormatter;
use crate::config::{ConfigurationManager, PacorConfig};
use crate::core::builder::BundleBuilder;
use crate::core::project::{find_flutter_project_root, validate_project_environment};
use crate::error::{ConfigurationError, PackageError, ValidationError};
use crate::platforms::android::AndroidBuilder;

/// Configuration context for build operations.
#[derive(Debug, Clone)]
pub(crate) struct BuildContext {
    pub(crate) command_output: CommandOutputMode,
    pub(crate) use_existing_flutter_build: bool,
    pub(crate) skip_cleanup: bool,
}

impl BuildContext {
    /// Create a new build context with the specified options.
    pub(crate) fn new(
        command_output: CommandOutputMode,
        use_existing_flutter_build: bool,
        skip_cleanup: bool,
    ) -> Self {
        Self {
            command_output,
            use_existing_flutter_build,
            skip_cleanup,
        }
    }
}

/// Handler for the build command - builds for the current platform.
pub(crate) fn handle_build_command(context: BuildContext) -> Result<()> {
    let formatter = OutputFormatter::new(context.command_output.clone());
    formatter.print_section_header("Building for Current Platform");

    let orchestrator = BuildOrchestrator::new(context)?;
    let build_outputs = orchestrator.execute_current_platform_build()?;

    formatter.display_build_success("Build Completed Successfully", &build_outputs);
    Ok(())
}

/// Handler for the Android build command.
pub(crate) fn handle_android_command(context: BuildContext, should_sign_apk: bool) -> Result<()> {
    let formatter = OutputFormatter::new(context.command_output.clone());
    formatter.print_section_header("Building Android APK");

    info!(
        "Starting Android build - signing: {}, use_existing_flutter_build: {}",
        should_sign_apk, context.use_existing_flutter_build
    );

    let bundle_config = PacorConfig::load()?;
    validate_android_signing_requirements(&bundle_config, should_sign_apk)?;

    let orchestrator = BuildOrchestrator::new(context)?;
    let build_outputs = orchestrator.execute_android_build(should_sign_apk)?;

    formatter.display_build_success("Android Build Completed Successfully", &build_outputs);
    Ok(())
}

/// Handler for the clean command - removes all build artifacts.
pub(crate) fn handle_clean_command() -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);

    info!("Starting cleanup of all build artifacts");

    let bundle_config = PacorConfig::load()?;
    let orchestrator = BuildOrchestrator::from_config(bundle_config)?;

    orchestrator.clean_all_artifacts()?;

    formatter.display_success("✓ All build artifacts cleaned successfully");
    info!("Build cleanup completed successfully");

    Ok(())
}

/// Handler for the check command - validates build environment.
pub(crate) fn handle_check_command() -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);
    formatter.print_section_header("Environment Validation");

    info!("Starting comprehensive build environment validation");

    // Validate basic project environment
    validate_project_structure(&formatter)?;

    // Load and validate configuration
    let bundle_config = load_and_validate_configuration(&formatter)?;

    // Validate platform-specific environments
    validate_platform_environments(&bundle_config, &formatter)?;

    formatter.print_section_header("Environment Validation Complete");
    info!("All environment validations passed successfully");

    Ok(())
}

/// Handler for the init command - creates project configuration.
pub(crate) fn handle_init_command(should_force_overwrite: bool) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);

    info!("Initializing new project configuration");

    // Try to find Flutter project root, but fall back to current directory if not found
    let project_root = find_flutter_project_root().unwrap_or_else(|_| {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    });

    let config_file_path = project_root.join("pacor.toml");

    // Validate that we can create the configuration file
    validate_configuration_file_creation(&config_file_path, should_force_overwrite)?;

    // Create and write default configuration using layered system defaults
    create_and_write_layered_default_configuration(&config_file_path)?;

    formatter.display_success(&format!(
        "✓ Configuration file created: {}",
        config_file_path.display()
    ));
    formatter.display_info("Please edit the configuration file to customize your build settings");
    formatter.display_info("");
    formatter.display_info("The configuration file uses a layered system that supports:");
    formatter.display_info("  - Global and platform-specific defaults");
    formatter.display_info("  - Environment variable overrides (PACOR_*)");
    formatter.display_info("  - Command-line argument overrides (future)");

    info!(
        "Project configuration initialized successfully at: {}",
        config_file_path.display()
    );

    Ok(())
}

/// Handler for the config command - displays current configuration.
pub(crate) fn handle_config_command(should_show_full_config: bool) -> Result<()> {
    let formatter = OutputFormatter::new(CommandOutputMode::Normal);

    // Load configuration with layer information for debugging
    let (bundle_config, layer_info) = PacorConfig::load_with_layer_info()?;

    formatter.print_section_header("Current Configuration");

    // Display configuration layers used
    formatter.display_info("Configuration layers loaded (priority order):");
    for (index, layer) in layer_info.iter().enumerate() {
        formatter.display_info(&format!("  {}. {}", index + 1, layer));
    }
    formatter.display_info(""); // Empty line for separation

    if should_show_full_config {
        display_full_configuration(&bundle_config, &formatter)?;
    } else {
        display_configuration_summary(&bundle_config, &formatter);
    }

    Ok(())
}

/// Validate Android signing configuration when signing is requested.
fn validate_android_signing_requirements(
    config: &PacorConfig,
    should_sign: bool,
) -> Result<(), ConfigurationError> {
    if should_sign
        && config
            .platforms
            .android
            .as_ref()
            .map(|c| c.keystore_file.is_none())
            .unwrap_or(false)
    {
        return Err(ConfigurationError::MissingRequired {
            field: "platforms.android.keystore_file".to_string(),
        });
    }

    if should_sign {
        info!("APK will be signed using configured keystore");
    } else {
        info!("APK will be built unsigned");
    }

    Ok(())
}

/// Validate basic project structure and environment.
fn validate_project_structure(formatter: &OutputFormatter) -> Result<()> {
    formatter.display_info("Validating project environment...");

    validate_project_environment().map_err(|e| {
        PackageError::Validation(ValidationError::ProjectValidation {
            reason: e.to_string(),
        })
    })?;

    let project_root = find_flutter_project_root().map_err(|_e| {
        PackageError::Validation(ValidationError::FlutterProjectNotFound {
            path: std::env::current_dir().unwrap_or_default(),
        })
    })?;

    formatter.display_success("✓ Project environment is valid");
    formatter.display_success(&format!(
        "✓ Project root directory: {}",
        project_root.display()
    ));

    info!("Project structure validation completed");
    Ok(())
}

/// Load configuration and perform basic validation.
fn load_and_validate_configuration(formatter: &OutputFormatter) -> Result<PacorConfig> {
    formatter.display_info("Loading and validating configuration...");

    let config = PacorConfig::load()?;

    formatter.display_success("✓ Configuration loaded and validated successfully");
    info!("Bundle configuration validation completed");

    Ok(config)
}

/// Validate all enabled platform build environments.
fn validate_platform_environments(config: &PacorConfig, formatter: &OutputFormatter) -> Result<()> {
    formatter.display_info("Validating platform build environments...");

    validate_macos_environment(config, formatter);
    validate_android_environment(config, formatter);
    // Additional platforms can be added here

    info!("Platform environment validation completed");
    Ok(())
}

/// Validate macOS build environment if enabled.
#[cfg(target_os = "macos")]
fn validate_macos_environment(config: &PacorConfig, formatter: &OutputFormatter) {
    if !config.platforms.macos.enabled {
        return;
    }

    match crate::platforms::macos::MacOSAppBuilder::new(config.clone()) {
        Ok(builder) => match builder.validate() {
            Ok(()) => {
                formatter.display_success("✓ macOS build environment is valid");
                info!("macOS build environment validation passed");
            }
            Err(e) => {
                formatter.display_error(&format!("✗ macOS build environment issue: {}", e));
                error!("macOS build environment validation failed: {}", e);
            }
        },
        Err(e) => {
            formatter.display_error(&format!("✗ Failed to create macOS builder: {}", e));
            error!("Failed to initialize macOS builder: {}", e);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn validate_macos_environment(_config: &PacorConfig, _formatter: &OutputFormatter) {
    // macOS validation not available on non-macOS platforms
}

/// Validate Android build environment if enabled.
fn validate_android_environment(config: &PacorConfig, formatter: &OutputFormatter) {
    if !config
        .platforms
        .android
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false)
    {
        return;
    }

    match AndroidBuilder::new(config) {
        Ok(builder) => match builder.validate() {
            Ok(()) => {
                formatter.display_success("✓ Android build environment is valid");
                info!("Android build environment validation passed");
            }
            Err(e) => {
                formatter.display_error(&format!("✗ Android build environment issue: {}", e));
                error!("Android build environment validation failed: {}", e);
            }
        },
        Err(e) => {
            formatter.display_error(&format!("✗ Failed to create Android builder: {}", e));
            error!("Failed to initialize Android builder: {}", e);
        }
    }
}

/// Validate that configuration file can be created.
fn validate_configuration_file_creation(
    config_path: &Path,
    force_overwrite: bool,
) -> Result<(), ConfigurationError> {
    if config_path.exists() && !force_overwrite {
        return Err(ConfigurationError::FileAlreadyExists {
            path: config_path.to_path_buf(),
        });
    }

    if config_path.exists() {
        info!("Overwriting existing configuration file");
    }

    Ok(())
}

/// Create default configuration using the layered system and write to file.
fn create_and_write_layered_default_configuration(config_path: &Path) -> Result<()> {
    // Use the layered configuration system to generate defaults
    let mut manager = ConfigurationManager::new_standalone()?;

    // Load only the default layers (no user config since we're creating it)
    let default_config = manager.load_default_configuration()?;

    let config_content = toml::to_string_pretty(&default_config).map_err(|e| {
        PackageError::Configuration(ConfigurationError::SerializationFailed {
            reason: e.to_string(),
        })
    })?;

    std::fs::write(config_path, config_content).map_err(|e| {
        PackageError::Configuration(ConfigurationError::WriteFailed {
            path: config_path.to_path_buf(),
            reason: e.to_string(),
        })
    })?;

    info!("Layered default configuration written successfully");
    Ok(())
}

/// Display complete configuration in TOML format.
fn display_full_configuration(config: &PacorConfig, formatter: &OutputFormatter) -> Result<()> {
    let config_toml = toml::to_string_pretty(config).map_err(|e| {
        PackageError::Configuration(ConfigurationError::SerializationFailed {
            reason: e.to_string(),
        })
    })?;

    formatter.display_info(&config_toml);
    info!("Full configuration displayed");

    Ok(())
}

/// Display configuration summary with key information.
fn display_configuration_summary(config: &PacorConfig, formatter: &OutputFormatter) {
    // Display application information
    formatter.display_info(&config.app.to_string());

    // Display enabled platforms
    display_enabled_platforms(config, formatter);

    info!("Configuration summary displayed");
}

/// Display list of enabled build platforms.
fn display_enabled_platforms(config: &PacorConfig, formatter: &OutputFormatter) {
    formatter.display_info("Enabled Platforms:");

    if config
        .platforms
        .macos
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false)
    {
        formatter.display_success("  ✓ macOS");
    }
    if config
        .platforms
        .windows
        .as_ref()
        .map(|c| c.is_enabled())
        .unwrap_or(false)
    {
        formatter.display_success("  ✓ Windows");
    }

    if config
        .platforms
        .android
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false)
    {
        formatter.display_success("  ✓ Android");
    }
}
