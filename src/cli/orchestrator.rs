//! Build orchestration for the Pacor packaging tool.
//!
//! This module contains the core build orchestration logic that coordinates
//! the build process across different platforms and manages the build lifecycle.

use anyhow::Result;

use crate::cli::args::CommandOutputMode;
use crate::cli::handlers::BuildContext;
use crate::config::PacorConfig;
use crate::core::builder::{BuilderFactory, BundleBuilder};
use crate::error::{BuildError, PackageError};
use crate::platforms::BuildArtifact;
use crate::utils::command::execute_flutter_clean_command;

/// Orchestrates the build process across different platforms.
pub(crate) struct BuildOrchestrator {
    config: PacorConfig,
    context: BuildContext,
}

impl BuildOrchestrator {
    /// Create a new build orchestrator with the provided context.
    pub(crate) fn new(context: BuildContext) -> Result<Self> {
        let config = PacorConfig::load()?;
        Ok(Self { config, context })
    }

    /// Create a new build orchestrator from an existing configuration.
    pub(crate) fn from_config(config: PacorConfig) -> Result<Self> {
        let context = BuildContext::new(CommandOutputMode::Normal, false, false);
        Ok(Self { config, context })
    }

    /// Execute build for the current platform.
    pub(crate) fn execute_current_platform_build(&self) -> Result<Vec<BuildArtifact>> {
        info!("Starting cross-platform build process");

        let platform_builder =
            BuilderFactory::create_for_current_platform(&self.config).map_err(|e| {
                error!("Failed to create platform builder: {}", e);
                e
            })?;

        self.validate_build_environment(&*platform_builder)?;

        self.log_build_configuration();

        let build_outputs = platform_builder.build(
            self.context.use_existing_flutter_build,
            self.context.command_output.clone(),
        )?;

        if !self.context.skip_cleanup {
            self.cleanup_build_artifacts(&*platform_builder)?;
        }

        Ok(build_outputs)
    }

    /// Execute Android build with optional signing.
    pub(crate) fn execute_android_build(
        &self,
        should_sign_apk: bool,
    ) -> Result<Vec<BuildArtifact>> {
        info!(
            "Starting Android build - signing: {}, use_existing_flutter_build: {}",
            should_sign_apk, self.context.use_existing_flutter_build
        );

        let android_builder = BuilderFactory::create_android(&self.config).map_err(|e| {
            error!("Failed to create android builder: {}", e);
            e
        })?;

        android_builder.validate().map_err(|e| {
            error!("Android build environment validation failed: {}", e);
            e
        })?;

        let build_outputs = android_builder.build(
            self.context.use_existing_flutter_build,
            self.context.command_output.clone(),
        )?;

        Ok(build_outputs)
    }

    /// Clean all build artifacts and temporary directories.
    pub(crate) fn clean_all_artifacts(&self) -> Result<()> {
        info!("Starting cleanup of all build artifacts");

        // Clean configured output directory
        self.clean_output_directory()?;

        // Clean Flutter's build directory
        self.clean_flutter_build_directory()?;

        Ok(())
    }

    /// Validate the build environment using the provided builder.
    fn validate_build_environment(
        &self,
        builder: &dyn BundleBuilder<Config = &PacorConfig, Output = Vec<BuildArtifact>>,
    ) -> Result<(), BuildError> {
        builder.validate().map_err(|e| {
            error!("Platform validation failed: {}", e);
            BuildError::EnvironmentSetupFailed {
                reason: format!("Platform validation failed: {}", e),
            }
        })
    }

    /// Log the current build configuration for debugging.
    fn log_build_configuration(&self) {
        info!(
            "Build configuration - use_existing_flutter_build: {}, skip_cleanup: {}, command_output: {:?}",
            self.context.use_existing_flutter_build,
            self.context.skip_cleanup,
            self.context.command_output
        );
    }

    /// Perform cleanup of build artifacts.
    fn cleanup_build_artifacts(
        &self,
        builder: &dyn BundleBuilder<Config = &PacorConfig, Output = Vec<BuildArtifact>>,
    ) -> Result<()> {
        // Clean up Flutter build artifacts to prevent errors in subsequent builds
        execute_flutter_clean_command(builder.project_root(), &self.context.command_output)?;
        debug!("Flutter build artifacts cleaned up successfully");

        // Perform platform-specific cleanup
        builder.clean().map_err(|e| {
            error!("Cleanup failed: {}", e);
            PackageError::Build(BuildError::CleanupFailed {
                reason: e.to_string(),
            })
        })?;
        debug!("Platform cleanup completed successfully");

        Ok(())
    }

    /// Clean the configured output directory.
    fn clean_output_directory(&self) -> Result<()> {
        let output_directory = self.config.output_dir()?;

        if !output_directory.exists() {
            debug!(
                "Output directory does not exist, skipping: {}",
                output_directory.display()
            );
            return Ok(());
        }

        println!("Cleaning output directory...");
        info!("Removing output directory: {}", output_directory.display());

        std::fs::remove_dir_all(&output_directory).map_err(|e| {
            error!("Failed to remove output directory: {}", e);
            PackageError::Build(BuildError::CleanupFailed {
                reason: format!("Failed to remove output directory: {}", e),
            })
        })?;

        println!("✓ Output directory cleaned: {}", output_directory.display());

        Ok(())
    }

    /// Clean Flutter's build directory in project root.
    fn clean_flutter_build_directory(&self) -> Result<()> {
        let project_root = self.config.project_root()?;
        let flutter_build_dir = project_root.join("build");

        if !flutter_build_dir.exists() {
            debug!(
                "Flutter build directory does not exist, skipping: {}",
                flutter_build_dir.display()
            );
            return Ok(());
        }

        println!("Cleaning Flutter build directory...");
        info!(
            "Removing Flutter build directory: {}",
            flutter_build_dir.display()
        );

        std::fs::remove_dir_all(&flutter_build_dir).map_err(|e| {
            error!("Failed to remove Flutter build directory: {}", e);
            PackageError::Build(BuildError::CleanupFailed {
                reason: format!("Failed to remove Flutter build directory: {}", e),
            })
        })?;

        println!(
            "✓ Flutter build directory cleaned: {}",
            flutter_build_dir.display()
        );

        Ok(())
    }
}
